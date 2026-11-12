mod store;

use crate::platform::window::PlatformWaker;
use faith_client::{Client, Config, EventKind, GroupKind, GroupPacket, Id};
use nix::{
    poll::{PollFd, PollFlags, PollTimeout, poll},
    sys::eventfd::{EfdFlags, EventFd},
};
use std::{
    collections::VecDeque,
    os::fd::AsFd,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
};
use store::Store;

#[derive(Clone, Default)]
pub struct Snapshot {
    pub auth: Option<Id>,
    pub status: String,
    pub selected: Option<Id>,
    pub members: Vec<Id>,
    pub active: bool,
    pub messages: Vec<String>,
    pub notices: Vec<String>,
    pub generation: u64,
    pub submission: Option<(u64, bool)>,
}

/// Network, filesystem and SQLite work stay on the worker/native reader threads.
/// The UI only sends bounded commands and reads an immutable display snapshot.
pub struct Session {
    commands: SyncSender<(u64, String)>,
    next_submission: AtomicU64,
    wake: Arc<EventFd>,
    stopping: Arc<AtomicBool>,
    snapshot: Arc<Mutex<Snapshot>>,
    worker: Option<JoinHandle<()>>,
}

impl Session {
    pub fn start(mut config: Config, waker: PlatformWaker) -> anyhow::Result<Self> {
        let profile = config
            .identity_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("identity needs a profile directory"))?;
        let store = Arc::new(Store::open(profile)?);
        config.store = Some(store.clone());
        let (commands, rx) = mpsc::sync_channel(32);
        let wake = Arc::new(EventFd::from_value_and_flags(
            0,
            EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_CLOEXEC,
        )?);
        let stopping = Arc::new(AtomicBool::new(false));
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let (thread_wake, thread_stop, thread_snapshot) =
            (wake.clone(), stopping.clone(), snapshot.clone());
        let worker = std::thread::Builder::new()
            .name("faith-session".into())
            .spawn(move || {
                let result = (|| -> anyhow::Result<()> {
                    let client = Client::new(config)?;
                    let (auth, _) = client.identity()?;
                    let mut state = Worker {
                        client,
                        store,
                        auth,
                        authorized: false,
                        selected: None,
                        offset: 0,
                        inflight: None,
                        status: "Connecting…".into(),
                        notices: VecDeque::new(),
                        generation: 0,
                        submission: None,
                    };
                    state.selected = state.store.selected()?.or(state
                        .store
                        .conversations()?
                        .first()
                        .map(|c| c.id));
                    state.publish(&thread_snapshot, &waker)?;
                    state.client.start()?;
                    state.run(rx, &thread_wake, &thread_stop, &thread_snapshot, &waker)
                })();
                if let Err(error) = result {
                    if let Ok(mut snapshot) = thread_snapshot.lock() {
                        snapshot.status = format!("Client stopped: {error}");
                        snapshot.generation += 1;
                    }
                    let _ = waker.request_redraw();
                }
            })?;
        Ok(Self {
            commands,
            next_submission: AtomicU64::new(1),
            wake,
            stopping,
            snapshot,
            worker: Some(worker),
        })
    }
    pub fn submit(&self, line: String) -> anyhow::Result<u64> {
        let id = self.next_submission.fetch_add(1, Ordering::Relaxed);
        self.commands
            .try_send((id, line))
            .map_err(|e| anyhow::anyhow!("client command queue unavailable: {e}"))?;
        self.wake.write(1)?;
        Ok(id)
    }
    pub fn snapshot_since(&self, generation: u64) -> Option<Snapshot> {
        let snapshot = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        (snapshot.generation != generation).then(|| snapshot.clone())
    }
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        let _ = self.wake.write(1);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Inflight {
    deadline: std::time::Instant,
    result: bool,
    echo: bool,
}
struct Worker {
    client: Client,
    store: Arc<Store>,
    auth: Id,
    authorized: bool,
    selected: Option<Id>,
    offset: usize,
    inflight: Option<Inflight>,
    status: String,
    notices: VecDeque<String>,
    generation: u64,
    submission: Option<(u64, bool)>,
}
impl Worker {
    fn notice(&mut self, line: impl Into<String>) {
        self.notices.push_back(line.into());
        while self.notices.len() > 8 {
            self.notices.pop_front();
        }
    }
    fn publish(&mut self, shared: &Mutex<Snapshot>, waker: &PlatformWaker) -> anyhow::Result<()> {
        let conversation = self
            .selected
            .map(|id| self.store.conversation(id))
            .transpose()?
            .flatten();
        self.generation += 1;
        let snapshot = Snapshot {
            auth: Some(self.auth),
            status: self.status.clone(),
            selected: self.selected,
            active: conversation.as_ref().is_some_and(|c| c.active),
            members: conversation.map_or_else(Vec::new, |c| c.members),
            messages: self
                .selected
                .map(|id| self.store.history(id, self.offset))
                .transpose()?
                .unwrap_or_default(),
            notices: self.notices.iter().cloned().collect(),
            generation: self.generation,
            submission: self.submission,
        };
        *shared
            .lock()
            .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))? = snapshot;
        waker.request_redraw()
    }
    fn send_pending(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(self.authorized, "not authorized yet");
        anyhow::ensure!(
            self.inflight.is_none(),
            "waiting for delivery result and durable echo"
        );
        let (packet, targets) = self
            .store
            .pending()?
            .ok_or_else(|| anyhow::anyhow!("no pending delivery"))?;
        self.client.deliver_group(&packet, &targets)?;
        self.inflight = Some(Inflight {
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
            result: false,
            echo: false,
        });
        self.status = "Delivery queued; waiting for server commit and local echo".into();
        Ok(())
    }
    fn deliver(&mut self, packet: GroupPacket, targets: Vec<Id>) -> anyhow::Result<()> {
        anyhow::ensure!(self.authorized, "not authorized yet");
        anyhow::ensure!(self.inflight.is_none(), "a delivery is still in progress");
        self.store.enqueue(&packet, &targets)?;
        if let Err(error) = self.send_pending() {
            self.status = "Delivery saved locally; use /retry when connected".into();
            self.notice(error.to_string());
        }
        Ok(())
    }
    fn command(&mut self, line: &str) -> anyhow::Result<()> {
        let mut words = line.split_whitespace();
        let command = words.next().unwrap_or("");
        match command {
            "/help" => self.notice("/new ID… | /list | /use ID | /members | /add ID | /remove ID | /history [offset] | /retry | /discard | /approve | /deny | /reconnect"),
            "/list" => {
                let offset=words.next().unwrap_or("0").parse::<usize>()?;
                self.notices.clear();
                for group in self.store.conversations()?.into_iter().skip(offset).take(8) { self.notice(format!("{} — {} members{}",group.id,group.members.len(),if group.active {""}else{" (removed)"})); }
            }
            "/use" => {
                let id=words.next().ok_or_else(||anyhow::anyhow!("usage: /use ID"))?.parse()?;
                anyhow::ensure!(self.store.conversation(id)?.is_some(),"unknown conversation");
                self.store.select(id)?; self.selected=Some(id); self.offset=0;
            }
            "/members" => {
                let group=self.group()?;
                for (i,id) in group.members.iter().enumerate() { self.notice(format!("{id}{}",if i==0 {" (creator)"}else{""})); }
            }
            "/history" => { self.offset=words.next().unwrap_or("0").parse::<usize>()?.min(i64::MAX as usize); }
            "/approve" => self.client.approve_pending_device()?,
            "/deny" => self.client.deny_pending_device()?,
            "/reconnect" => self.client.reconnect()?,
            "/retry" => self.send_pending()?,
            "/discard" => {
                anyhow::ensure!(self.inflight.is_none(),"wait for the current operation before discarding");
                self.store.discard_pending()?;
                self.notice("Pending retry discarded. A delivery already committed by the server cannot be recalled.");
            }
            "/new" => {
                let mut members=vec![self.auth];
                for word in words { let id=word.parse()?; anyhow::ensure!(!members.contains(&id),"duplicate member"); members.push(id); }
                anyhow::ensure!(members.len()<=faith_client::MAX_MEMBERS,"too many members");
                let packet=GroupPacket {kind:GroupKind::Create,revision:0,conversation:Id::random()?,message:Id::random()?,members:members.clone(),text:Vec::new()};
                let group=packet.conversation;
                self.deliver(packet,members)?;
                self.store.select(group)?; self.selected=Some(group); self.offset=0;
            }
            "/add" | "/remove" => {
                let mut group=self.group()?;
                anyhow::ensure!(group.active && group.members[0]==self.auth,"only the active group's creator can change membership");
                let id:Id=words.next().ok_or_else(||anyhow::anyhow!("expected participant ID"))?.parse()?;
                let mut targets=group.members.clone();
                if command=="/add" {
                    anyhow::ensure!(!group.members.contains(&id) && group.members.len()<faith_client::MAX_MEMBERS,"already a member or group is full");
                    group.members.push(id); targets.push(id);
                } else {
                    anyhow::ensure!(id!=self.auth && group.members.contains(&id),"cannot remove creator or unknown member");
                    group.members.retain(|member|*member!=id);
                }
                let revision=group.revision.checked_add(1).filter(|r|*r<=i64::MAX as u64).ok_or_else(||anyhow::anyhow!("revision overflow"))?;
                self.deliver(GroupPacket {kind:GroupKind::Update,revision,conversation:group.id,message:Id::random()?,members:group.members,text:Vec::new()},targets)?;
            }
            word if word.starts_with('/') => anyhow::bail!("unknown command; use /help"),
            _ => {
                anyhow::ensure!(!line.trim().is_empty() && line.len()<=faith_client::MAX_TEXT,"message must contain 1–4096 bytes");
                let group=self.group()?;
                anyhow::ensure!(group.active,"you were removed from this conversation");
                self.deliver(GroupPacket {kind:GroupKind::Message,revision:group.revision,conversation:group.id,message:Id::random()?,members:group.members.clone(),text:line.as_bytes().to_vec()},group.members)?;
                self.offset=0;
            }
        }
        Ok(())
    }
    fn group(&self) -> anyhow::Result<store::Conversation> {
        let id = self.selected.ok_or_else(|| {
            anyhow::anyhow!("select a conversation with /use or create one with /new")
        })?;
        self.store
            .conversation(id)?
            .ok_or_else(|| anyhow::anyhow!("conversation is not durably created yet"))
    }
    fn run(
        &mut self,
        rx: Receiver<(u64, String)>,
        wake: &EventFd,
        stopping: &AtomicBool,
        shared: &Mutex<Snapshot>,
        waker: &PlatformWaker,
    ) -> anyhow::Result<()> {
        loop {
            let mut fds = [
                PollFd::new(self.client.event_fd(), PollFlags::POLLIN),
                PollFd::new(wake.as_fd(), PollFlags::POLLIN),
            ];
            let timeout = self.inflight.as_ref().map_or(PollTimeout::NONE, |pending| {
                PollTimeout::try_from(
                    pending
                        .deadline
                        .saturating_duration_since(std::time::Instant::now())
                        .as_millis()
                        .saturating_add(1)
                        .min(i32::MAX as u128) as u32,
                )
                .unwrap_or(PollTimeout::MAX)
            });
            match poll(&mut fds, timeout) {
                Ok(_) => {}
                Err(nix::errno::Errno::EINTR) => continue,
                Err(e) => return Err(e.into()),
            }
            if stopping.load(Ordering::Acquire) {
                return Ok(());
            }
            for fd in &fds {
                anyhow::ensure!(
                    !fd.revents()
                        .unwrap_or(PollFlags::empty())
                        .intersects(PollFlags::POLLERR | PollFlags::POLLHUP | PollFlags::POLLNVAL),
                    "client event descriptor closed"
                );
            }
            self.client.clear_event_signal()?;
            let mut changed = false;
            if self
                .inflight
                .as_ref()
                .is_some_and(|pending| std::time::Instant::now() >= pending.deadline)
            {
                self.inflight = None;
                self.status = "Delivery unconfirmed; use /retry or /discard".into();
                self.notice("No valid durable echo and result within 30 seconds. The pending operation remains on disk.");
                changed = true;
            }
            while let Some(event) = self.client.next_event()? {
                if matches!(
                    event.kind,
                    EventKind::Pong | EventKind::None | EventKind::Connected
                ) {
                    continue;
                }
                changed = true;
                match event.kind {
                    EventKind::Authorized => {
                        self.authorized = true;
                        self.status = "Connected".into();
                        if self.store.pending()?.is_some()
                            && self.inflight.is_none()
                            && let Err(error) = self.send_pending()
                        {
                            self.notice(format!("Pending retry: {error}"));
                        }
                    }
                    EventKind::Connecting => {
                        self.authorized = false;
                        self.status = "Connecting…".into();
                    }
                    EventKind::Disconnected | EventKind::ServerDisconnect => {
                        self.authorized = false;
                        self.inflight = None;
                        self.status = "Disconnected; awaiting reconnect".into();
                    }
                    EventKind::GroupReceived => {
                        if self.selected.is_none() {
                            self.selected = self.store.conversations()?.first().map(|c| c.id);
                            if let Some(id) = self.selected {
                                self.store.select(id)?;
                            }
                        }
                        if self.store.pending()?.is_none()
                            && let Some(inflight) = &mut self.inflight
                        {
                            inflight.echo = true;
                        }
                    }
                    EventKind::CommandResult if event.group_result().is_some() => {
                        if event.group_result() == Some(true) {
                            if let Some(inflight) = &mut self.inflight {
                                inflight.result = true;
                            }
                        } else {
                            self.inflight = None;
                            self.status = "Delivery rejected; use /retry or /discard".into();
                            self.notice(event.message);
                        }
                    }
                    EventKind::CommandFailed | EventKind::Error => {
                        self.inflight = None;
                        self.notice(event.message);
                        self.status = "Operation failed; pending deliveries can be retried".into();
                    }
                    EventKind::DeviceAuthPending | EventKind::DeviceLinkRequest => self.notice(
                        format!("{:?}: {} — /approve or /deny", event.kind, event.message),
                    ),
                    _ => {}
                }
                if self.inflight.as_ref().is_some_and(|p| p.result && p.echo) {
                    self.inflight = None;
                    self.status = "Delivered to server queue and stored locally".into();
                }
            }
            match wake.read() {
                Ok(_) => {}
                Err(nix::errno::Errno::EAGAIN) => {}
                Err(e) => return Err(e.into()),
            }
            while let Ok((id, line)) = rx.try_recv() {
                changed = true;
                match self.command(&line) {
                    Ok(()) => self.submission = Some((id, true)),
                    Err(error) => {
                        self.submission = Some((id, false));
                        self.notice(error.to_string());
                    }
                }
            }
            if changed {
                self.publish(shared, waker)?;
            }
        }
    }
}
