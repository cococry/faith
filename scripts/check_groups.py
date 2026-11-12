#!/usr/bin/env python3
"""Exercise real TUI processes against two servers and a disposable PostgreSQL DB."""
from pathlib import Path
import os, re, socket, sqlite3, subprocess, tempfile, time, uuid, contextlib, shutil, sys, struct
ROOT = Path(__file__).resolve().parents[1]

def wait(predicate, label, timeout=20):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            result = predicate()
            if result: return result
        except (OSError, sqlite3.Error): pass
        time.sleep(.05)
    raise AssertionError(label)

def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0)); return sock.getsockname()[1]

def main():
    for project in ['faith-proto','faith-client','faith-server','faith-client/test']:
        for command in [['cc','-o','nob','nob.c'],['./nob']]:
            subprocess.run(command, cwd=ROOT/project, check=True, stdout=subprocess.DEVNULL)
    production = '--production-compose' in sys.argv
    compose = '--compose' in sys.argv or production
    compose_command = ['docker','compose','--env-file',str(ROOT/'.local/system.env')]
    if production:
        os.environ.setdefault('POSTGRES_IMAGE','postgres@sha256:665dc7b37ac52a17e298511414c21d23ffa4e6a6ef6225b3e5d59c7afb35e994')
        compose_command += ['--project-name','faith-prod-check','-f',str(ROOT/'compose.yaml'),'-f',str(ROOT/'compose.production.yaml')]
    container = subprocess.check_output(compose_command + ['ps','-q','db'],cwd=ROOT,text=True).strip() if compose else 'faith-group-check-' + uuid.uuid4().hex[:10]
    if not container: raise RuntimeError('Run scripts/boot_system.sh 2 first')
    if not compose: subprocess.run(['docker','run','-d','--name',container,'-e','POSTGRES_PASSWORD=check',
                    '-p','127.0.0.1::5432','postgres:17-bookworm'], check=True, stdout=subprocess.DEVNULL)
    processes = []; logs = []
    temporary = tempfile.mkdtemp(prefix="faith-group-check-")
    try:
        wait(lambda: subprocess.run(['docker','exec',container,'pg_isready','-h','127.0.0.1','-U','faith_admin' if compose else 'postgres'],
             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0, 'database startup')
        def sql(query):
            return subprocess.check_output(['docker','exec','-i',container,'psql','-U','faith_admin' if compose else 'postgres',
                   '-d','faith' if compose else 'postgres','-v','ON_ERROR_STOP=1','-Atc',query],text=True).strip()
        if not compose:
            for migration in sorted((ROOT/'faith-db/migrations').glob('*.sql')): sql(migration.read_text())
        dbport = '' if compose else subprocess.check_output(['docker','port',container,'5432'],text=True).strip().split(':')[-1]
        with contextlib.nullcontext(temporary):
            work = Path(temporary)
            if not compose: subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(work/'pkey.pem'),
                '-out',str(work/'chain.pem'),'-days','1','-subj','/CN=localhost','-addext','subjectAltName=DNS:localhost'],
                check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            def start(args, name, env=None):
                log = open(work/(name+'.log'),'a'); logs.append(log)
                proc = subprocess.Popen(args,cwd=work,env=env,stdin=subprocess.PIPE,stdout=log,stderr=log,text=True)
                processes.append(proc); return proc
            if compose:
                shutil.copyfile(ROOT/'.local/system/cert.pem',work/'chain.pem')
                def endpoint(index):
                    return int(subprocess.check_output(compose_command + ['port','--index',str(1 if production else index),'gateway' if production else 'api','4433'],cwd=ROOT,text=True).strip().split(':')[-1])
                p1,p2 = endpoint(1),endpoint(2)
            else: p1,p2 = port(),port()
            env = {**os.environ,'FAITH_DATABASE_URL':f'postgresql://postgres:check@127.0.0.1:{dbport}/postgres'}
            for number,p in enumerate([] if compose else [p1,p2]):
                start([str(ROOT/'faith-server/build/faith-server'),'-p',str(p)],'server'+str(number),env)
            def tui(name, endpoint):
                log_path = work/(name+'.log')
                offset = log_path.stat().st_size if log_path.exists() else 0
                proc = start([str(ROOT/'faith-client/test/build/faith-tui'),str(work/name),'127.0.0.1',str(endpoint),
                             str(work/'chain.pem'),'localhost'],name)
                wait(lambda: 'FAITH_EVENT_AUTHORIZED' in (work/(name+'.log')).read_text()[offset:], name+' authorization')
                return proc
            def query(name, statement):
                with sqlite3.connect(work/name/'conversations.sqlite',timeout=2) as db:
                    return db.execute(statement).fetchall()
            def send(proc,line): proc.stdin.write(line+'\n'); proc.stdin.flush()
            def identity(name): return re.findall(r'Identity: ([a-f0-9]{32})',(work/(name+'.log')).read_text())[-1]
            a,b,c = tui('alice',p1),tui('bob',p2),tui('carol',p1)
            aid,bid,cid = identity('alice'),identity('bob'),identity('carol')
            send(c,'/quit'); c.wait(timeout=10)
            send(a,'/new '+bid)
            wait(lambda: query('alice','SELECT id FROM conversations'),'create local conversation')
            wait(lambda: query('bob','SELECT id FROM conversations'),'cross-server create delivery')
            conversation = query('alice','SELECT hex(id) FROM conversations')[0][0]
            send(a,'hello-two')
            wait(lambda: query('bob',"SELECT body FROM messages WHERE body='hello-two'"),'cross-server message')
            send(a,'x'*4096)
            wait(lambda: query('bob','SELECT id FROM messages WHERE length(body)=4096'),'maximum-size conversation message')
            send(a,'/add '+cid)
            wait(lambda: query('bob','SELECT revision FROM conversations')[0][0] == 1,'membership update')
            send(a,'offline-carol')
            wait(lambda: int(sql("SELECT count(*) FROM device_events WHERE auth_id=decode('"+cid+"','hex')")) >= 2,'offline queue')
            c = tui('carol',p1)
            wait(lambda: query('carol',"SELECT body FROM messages WHERE body='offline-carol'"),'offline delivery after profile restart')
            assert identity('carol') == cid
            assert len(query('carol','SELECT members FROM conversations')[0][0]) == 48
            send(b,'/use '+conversation); send(b,'from-bob')
            wait(lambda: query('carol',"SELECT body FROM messages WHERE body='from-bob'"),'non-owner send')
            send(a,'/remove '+bid)
            wait(lambda: query('bob','SELECT active FROM conversations')[0][0] == 0,'removed participant notified')
            wait(lambda: query('carol','SELECT revision FROM conversations')[0][0] == 2,'remaining membership updated')
            send(a,'after-removal')
            wait(lambda: query('carol',"SELECT body FROM messages WHERE body='after-removal'"),'remaining participant delivery')
            time.sleep(1.2)
            assert not query('bob',"SELECT body FROM messages WHERE body='after-removal'")
            # A removed sender can forge its own local snapshot, but recipients
            # must reject it using their own durable membership state.
            send(b,'/quit'); b.wait(timeout=10)
            with sqlite3.connect(work/'bob'/'conversations.sqlite') as db:
                db.execute('UPDATE conversations SET members=?,active=1',
                           (bytes.fromhex(aid+bid+cid),))
            b = tui('bob',p2)
            send(b,'/use '+conversation); send(b,'forged-from-removed')
            wait(lambda: query('bob',"SELECT body FROM messages WHERE body='forged-from-removed'"),'forged sender echo')
            time.sleep(1.2)
            assert not query('alice',"SELECT body FROM messages WHERE body='forged-from-removed'")
            assert not query('carol',"SELECT body FROM messages WHERE body='forged-from-removed'")
            send(c,'/quit'); c.wait(timeout=10)
            row = query('carol',"SELECT id,body FROM messages WHERE body='after-removal'")[0]
            members = query('carol','SELECT members FROM conversations')[0][0]
            text = row[1].encode()
            packet = bytes.fromhex(aid) + struct.pack('!BBHQ16s16sI',1,3,len(members)//16,2,
                       bytes.fromhex(conversation),row[0],len(text)) + members + text
            predicate = "auth_id=decode('"+cid+"','hex')"
            sql("BEGIN; INSERT INTO device_events(auth_id,device_id,seq_num,event_type,event_data) "
                "SELECT auth_id,device_id,next_seq,1,decode('"+packet.hex()+"','hex') "
                "FROM device_event_inboxes WHERE "+predicate+"; "
                "UPDATE device_event_inboxes SET next_seq=next_seq+1 WHERE "+predicate+"; COMMIT;")
            c = tui('carol',p1)
            wait(lambda: sql('SELECT count(*) FROM device_events WHERE '+predicate)=='0','duplicate message ACK')
            assert len(query('carol',"SELECT id FROM messages WHERE body='after-removal'")) == 1
            send(a,'/quit'); a.wait(timeout=10)
            a = tui('alice',p1)
            assert identity('alice') == aid
            assert query('alice',"SELECT body FROM messages WHERE body='after-removal'")
            account_filter = "auth_id IN (decode('"+aid+"','hex'),decode('"+bid+"','hex'),decode('"+cid+"','hex'))"
            wait(lambda: sql('SELECT count(*) FROM device_events WHERE ' + account_filter) == '0','durable ACK cleanup')
            # Exercise the advertised wire-size/member limits with registered
            # offline fixtures, rather than launching 29 extra TUI processes.
            extra = [uuid.uuid4().hex for _ in range(29)]
            statements = []
            for identity_id in extra:
                value = "decode('"+identity_id+"','hex')"
                statements.append('INSERT INTO users(auth_id) VALUES ('+value+');')
                statements.append('INSERT INTO devices(auth_id,device_id,public_key) VALUES ('+value+
                                  ",decode('"+uuid.uuid4().hex+"','hex'),decode(repeat('00',32),'hex'));")
            sql('BEGIN;'+''.join(statements)+'COMMIT;')
            send(a,'/new '+' '.join([bid,cid]+extra))
            wait(lambda: query('bob','SELECT id FROM conversations WHERE length(members)=512'),'32-member conversation')
            send(a,'z'*4096)
            wait(lambda: query('carol',"SELECT id FROM messages WHERE length(body)=4096 AND substr(body,1,1)='z'"),'32-member maximum-size message')
            wait(lambda: sql('SELECT count(*) FROM device_events WHERE ' + account_filter) == '0','large-message ACK cleanup')
            account_filter = 'auth_id IN ('+','.join("decode('"+value+"','hex')" for value in [aid,bid,cid]+extra)+')'
            for proc in [a,b,c]: send(proc,'/quit'); proc.wait(timeout=10)
            if compose: sql('DELETE FROM users WHERE ' + account_filter)
            print('PASS: two API instances; dynamic groups; offline durable messages; stable identities; local history; removal; ACK cleanup')
    except Exception:
        for log in logs:
            log.flush()
            print(Path(log.name).read_text()[-5000:])
        raise
    finally:
        for proc in reversed(processes):
            if proc.poll() is None: proc.terminate()
        for proc in processes:
            try: proc.wait(timeout=10)
            except subprocess.TimeoutExpired: proc.kill(); proc.wait()
        for log in logs: log.close()
        shutil.rmtree(temporary)
        if not compose: subprocess.run(['docker','rm','-f',container],check=True,stdout=subprocess.DEVNULL)

if __name__ == '__main__': main()
