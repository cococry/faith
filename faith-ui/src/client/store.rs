use faith_client::{EventStore, GroupKind, GroupPacket, Id, StoredEvent};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::Mutex,
};

#[derive(Clone, Debug)]
pub struct Conversation {
    pub id: Id,
    pub revision: u64,
    pub members: Vec<Id>,
    pub active: bool,
}

pub struct Store {
    db: Mutex<Connection>,
    _profile_lock: File,
}

fn id(bytes: Vec<u8>) -> rusqlite::Result<Id> {
    bytes
        .try_into()
        .map(Id)
        .map_err(|_| rusqlite::Error::InvalidQuery)
}

fn members(bytes: Vec<u8>) -> rusqlite::Result<Vec<Id>> {
    if bytes.is_empty()
        || !bytes.len().is_multiple_of(16)
        || bytes.len() > 16 * faith_client::MAX_MEMBERS
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(bytes.as_chunks::<16>().0.iter().map(|b| Id(*b)).collect())
}

fn pack(ids: &[Id]) -> Vec<u8> {
    ids.iter().flat_map(|id| id.0).collect()
}
fn conversation(db: &Connection, group: Id) -> rusqlite::Result<Option<Conversation>> {
    db.query_row(
        "SELECT revision,members,active FROM conversations WHERE id=?",
        [group.0.as_slice()],
        |r| {
            Ok(Conversation {
                id: group,
                revision: r.get(0)?,
                members: members(r.get(1)?)?,
                active: r.get(2)?,
            })
        },
    )
    .optional()
}

impl Store {
    pub fn open(profile: &Path) -> anyhow::Result<Self> {
        if !profile.exists() {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(profile)?;
        }
        let metadata = fs::symlink_metadata(profile)?;
        anyhow::ensure!(
            metadata.is_dir()
                && metadata.uid() == unsafe { nix::libc::geteuid() }
                && metadata.mode() & 0o077 == 0,
            "profile must be an owned directory with permissions 0700"
        );
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(profile.join("profile.lock"))?;
        lock.try_lock()
            .map_err(|_| anyhow::anyhow!("profile is already open by another client"))?;
        let database = profile.join("conversations.sqlite");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&database)?;
        anyhow::ensure!(
            file.metadata()?.is_file() && file.metadata()?.uid() == metadata.uid(),
            "database must be an owned regular file"
        );
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        let db = Connection::open(database)?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS conversations(id BLOB PRIMARY KEY,revision INTEGER NOT NULL,members BLOB NOT NULL,active INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS messages(conversation BLOB NOT NULL,sender BLOB NOT NULL,id BLOB NOT NULL,body TEXT NOT NULL,PRIMARY KEY(conversation,sender,id));
            CREATE TABLE IF NOT EXISTS cursors(device BLOB PRIMARY KEY,seq INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS outbox(id BLOB PRIMARY KEY,packet BLOB NOT NULL,targets BLOB NOT NULL);
            CREATE INDEX IF NOT EXISTS messages_by_conversation ON messages(conversation);")?;
        Ok(Self {
            db: Mutex::new(db),
            _profile_lock: lock,
        })
    }

    fn db(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Connection>> {
        self.db
            .lock()
            .map_err(|_| anyhow::anyhow!("local storage lock poisoned"))
    }
    pub fn conversation(&self, group: Id) -> anyhow::Result<Option<Conversation>> {
        Ok(conversation(&*self.db()?, group)?)
    }
    pub fn conversations(&self) -> anyhow::Result<Vec<Conversation>> {
        let db = self.db()?;
        let mut stmt = db.prepare(
            "SELECT id,revision,members,active FROM conversations ORDER BY rowid DESC LIMIT 128",
        )?;
        Ok(stmt
            .query_map([], |r| {
                Ok(Conversation {
                    id: id(r.get(0)?)?,
                    revision: r.get(1)?,
                    members: members(r.get(2)?)?,
                    active: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn selected(&self) -> anyhow::Result<Option<Id>> {
        let db = self.db()?;
        let bytes: Option<Vec<u8>> = db
            .query_row("SELECT value FROM settings WHERE key='selected'", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(bytes.map(id).transpose()?)
    }
    pub fn select(&self, group: Id) -> anyhow::Result<()> {
        self.db()?.execute("INSERT INTO settings(key,value) VALUES('selected',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[group.0.as_slice()])?;
        Ok(())
    }
    pub fn history(&self, group: Id, offset: usize) -> anyhow::Result<Vec<String>> {
        let db = self.db()?;
        let mut stmt=db.prepare("SELECT sender,body FROM messages WHERE conversation=? ORDER BY rowid DESC LIMIT 100 OFFSET ?")?;
        let mut messages = stmt
            .query_map(params![group.0.as_slice(), offset as i64], |r| {
                let sender = id(r.get(0)?)?;
                let body: String = r.get(1)?;
                // Received bodies are untrusted text, never terminal/control commands.
                let body: String = body
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\n')
                    .collect();
                Ok(format!("{sender}: {body}"))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        messages.reverse();
        Ok(messages)
    }
    pub fn pending(&self) -> anyhow::Result<Option<(GroupPacket, Vec<Id>)>> {
        let db = self.db()?;
        let row: Option<(Vec<u8>, Vec<u8>)> = db
            .query_row("SELECT packet,targets FROM outbox LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        row.map(|(packet, targets)| Ok((GroupPacket::decode(&packet)?, members_targets(targets)?)))
            .transpose()
    }
    pub fn enqueue(&self, packet: &GroupPacket, targets: &[Id]) -> anyhow::Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        anyhow::ensure!(
            !tx.query_row("SELECT EXISTS(SELECT 1 FROM outbox)", [], |r| r
                .get::<_, bool>(0))?,
            "a delivery is pending; use /retry or /discard"
        );
        tx.execute(
            "INSERT INTO outbox(id,packet,targets) VALUES(?,?,?)",
            params![packet.message.0.as_slice(), packet.encode()?, pack(targets)],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn discard_pending(&self) -> anyhow::Result<()> {
        self.db()?.execute("DELETE FROM outbox", [])?;
        Ok(())
    }

    fn apply(&self, event: StoredEvent<'_>) -> anyhow::Result<()> {
        anyhow::ensure!(
            event.sequence <= i64::MAX as u64,
            "invalid delivery sequence"
        );
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let previous: Option<u64> = tx
            .query_row(
                "SELECT seq FROM cursors WHERE device=?",
                [event.device.0.as_slice()],
                |r| r.get(0),
            )
            .optional()?;
        if previous.is_some_and(|seq| event.sequence <= seq) {
            return Ok(());
        }
        if event.kind == faith_client::GROUP_PACKET_EVENT {
            anyhow::ensure!(event.data.len() >= 16, "truncated group event");
            let sender = Id(event.data[..16].try_into()?);
            let packet = GroupPacket::decode(&event.data[16..])?;
            let old = conversation(&tx, packet.conversation)?;
            let accepted = match packet.kind {
                GroupKind::Message => old.as_ref().is_some_and(|old| {
                    old.active
                        && old.revision == packet.revision
                        && old.members == packet.members
                        && old.members.contains(&sender)
                        && old.members.contains(&event.auth)
                }),
                _ => {
                    packet.members[0] == sender
                        && match old.as_ref() {
                            None => packet.members.contains(&event.auth),
                            Some(old) => old.members[0] == sender && packet.revision > old.revision,
                        }
                }
            };
            if accepted
                && (packet.kind != GroupKind::Message || std::str::from_utf8(&packet.text).is_ok())
            {
                if packet.kind == GroupKind::Message {
                    let text = std::str::from_utf8(&packet.text)?;
                    tx.execute("INSERT OR IGNORE INTO messages(conversation,sender,id,body) VALUES(?,?,?,?)",
                        params![packet.conversation.0.as_slice(),sender.0.as_slice(),packet.message.0.as_slice(),text])?;
                } else {
                    tx.execute("INSERT INTO conversations(id,revision,members,active) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,members=excluded.members,active=excluded.active",
                        params![packet.conversation.0.as_slice(),packet.revision,pack(&packet.members),packet.members.contains(&event.auth)])?;
                }
                if sender == event.auth {
                    tx.execute(
                        "DELETE FROM outbox WHERE id=?",
                        [packet.message.0.as_slice()],
                    )?;
                }
            }
            // Rejected membership/forged sender is durably handled, never applied.
        }
        tx.execute("INSERT INTO cursors(device,seq) VALUES(?,?) ON CONFLICT(device) DO UPDATE SET seq=excluded.seq",params![event.device.0.as_slice(),event.sequence])?;
        tx.commit()?;
        Ok(())
    }
}
fn members_targets(bytes: Vec<u8>) -> anyhow::Result<Vec<Id>> {
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len().is_multiple_of(16) && bytes.len() <= 64 * 16,
        "invalid outbox targets"
    );
    Ok(bytes.as_chunks::<16>().0.iter().map(|b| Id(*b)).collect())
}
impl EventStore for Store {
    fn store(&self, event: StoredEvent<'_>) -> faith_client::Result<()> {
        self.apply(event).map_err(|error| {
            tracing::error!("Durable event storage failed: {error}");
            faith_client::Error::Native(6)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Profile(std::path::PathBuf);
    impl Profile {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("faith-store-{}", Id::random().unwrap())))
        }
    }
    impl Drop for Profile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn apply(store: &Store, auth: Id, device: Id, seq: u64, sender: Id, packet: &GroupPacket) {
        let mut data = sender.0.to_vec();
        data.extend(packet.encode().unwrap());
        store
            .store(StoredEvent {
                auth,
                device,
                sequence: seq,
                kind: 1,
                data: &data,
            })
            .unwrap();
    }
    #[test]
    fn durable_membership_rejects_forgery_and_survives_restart() {
        let profile = Profile::new();
        let store = Store::open(&profile.0).unwrap();
        let (alice, bob, eve, device) = (
            Id::random().unwrap(),
            Id::random().unwrap(),
            Id::random().unwrap(),
            Id::random().unwrap(),
        );
        let mut p = GroupPacket {
            kind: GroupKind::Create,
            revision: 0,
            conversation: Id::random().unwrap(),
            message: Id::random().unwrap(),
            members: vec![alice, bob],
            text: Vec::new(),
        };
        apply(&store, bob, device, 0, alice, &p);
        store.select(p.conversation).unwrap();
        p.kind = GroupKind::Message;
        p.message = Id::random().unwrap();
        p.text = "hello 世界".as_bytes().to_vec();
        apply(&store, bob, device, 1, alice, &p);
        apply(&store, bob, device, 2, alice, &p); // Same message at a new sequence.
        assert_eq!(store.history(p.conversation, 0).unwrap().len(), 1);
        p.message = Id::random().unwrap();
        p.members = vec![bob, eve];
        apply(&store, bob, device, 3, eve, &p); // Forged membership snapshot.
        assert_eq!(store.history(p.conversation, 0).unwrap().len(), 1);
        p.kind = GroupKind::Update;
        p.revision = 1;
        p.members = vec![alice, eve];
        p.text.clear();
        apply(&store, bob, device, 4, eve, &p); // Noncreator cannot change state.
        assert!(store.conversation(p.conversation).unwrap().unwrap().active);
        apply(&store, bob, device, 5, alice, &p); // Genuine removal reaches removed Bob.
        assert!(!store.conversation(p.conversation).unwrap().unwrap().active);
        p.kind = GroupKind::Message;
        p.text = b"after removal".to_vec();
        apply(&store, bob, device, 6, alice, &p);
        assert_eq!(store.history(p.conversation, 0).unwrap().len(), 1);
        drop(store);
        let store = Store::open(&profile.0).unwrap();
        assert_eq!(store.selected().unwrap(), Some(p.conversation));
        assert_eq!(store.history(p.conversation, 0).unwrap().len(), 1);
        assert!(!store.conversation(p.conversation).unwrap().unwrap().active);
        let seq: u64 = store
            .db()
            .unwrap()
            .query_row("SELECT seq FROM cursors", [], |r| r.get(0))
            .unwrap();
        assert_eq!(seq, 6); // Rejected packets are handled durably, allowing ACK.
    }
    #[test]
    fn outbox_is_durable_until_valid_self_echo_and_profile_is_exclusive() {
        let profile = Profile::new();
        let store = Store::open(&profile.0).unwrap();
        assert!(Store::open(&profile.0).is_err());
        let auth = Id::random().unwrap();
        let device = Id::random().unwrap();
        let p = GroupPacket {
            kind: GroupKind::Create,
            revision: 0,
            conversation: Id::random().unwrap(),
            message: Id::random().unwrap(),
            members: vec![auth],
            text: Vec::new(),
        };
        store.enqueue(&p, &p.members).unwrap();
        assert!(store.enqueue(&p, &p.members).is_err());
        drop(store);
        let store = Store::open(&profile.0).unwrap();
        assert_eq!(store.pending().unwrap().unwrap().0, p);
        apply(&store, auth, device, 0, auth, &p);
        assert!(store.pending().unwrap().is_none());
        // Replaying sequence 0 must not revert revision or repeat effects.
        let mut update = p.clone();
        update.kind = GroupKind::Update;
        update.revision = 1;
        apply(&store, auth, device, 1, auth, &update);
        apply(&store, auth, device, 0, auth, &p);
        assert_eq!(
            store
                .conversation(p.conversation)
                .unwrap()
                .unwrap()
                .revision,
            1
        );
    }
}
