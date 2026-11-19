mod store;

use crate::platform::window::PlatformWaker;
use faith_client::{Client, Config, EventKind, GroupKind, GroupPacket, Id};
use nix::{
    poll::{PollFd, PollFlags, PollTimeout, poll},
    sys::eventfd::{EfdFlags, EventFd},
};

use std::{
    collections::{HashMap, VecDeque},
    os::fd::AsFd,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
};

use store::Store;
pub use store::conversation_titles;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Metadata {
    title: String,
    partners: Vec<([u8; 16], String)>,
}

#[derive(Clone)]
pub struct Chat {
    pub id: Id,
    pub title: String,
    pub needs_name: bool,
    pub last: String,
    metadata: Option<Metadata>,
}

#[derive(Clone)]
pub struct Message {
    pub row_id: i64,
    pub sender: Id,
    pub author: String,
    pub body: String,
}

pub enum Command {
    Select(Id),
    Rename {
        id: Id,
        title: String,
    },
    Create {
        title: String,
        partners: Vec<(Id, String)>,
    },
    Send {
        id: Id,
        body: String,
    },
    Retry,
    Discard,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub auth: Option<Id>,
    pub authorized: bool,
    pub status: String,
    pub selected: Option<Id>,
    pub conversations: Arc<[Chat]>,
    pub messages: Arc<[Message]>,
    pub notices: Arc<[String]>,
    pub generation: u64,
    pub submission: Option<(u64, bool, Option<Id>)>,
    pub pending: bool,
    pub delivering: bool,
}

/// Network, filesystem and SQLite work stay on the worker/native reader threads.
/// The UI only sends bounded commands and reads an immutable display snapshot.
pub struct Session {
    commands: SyncSender<(u64, Command)>,
    next_submission: AtomicU64,
    wake: Arc<EventFd>,
    stopping: Arc<AtomicBool>,
    snapshot: Arc<Mutex<Snapshot>>,
    worker: Option<JoinHandle<()>>,
    profile: PathBuf,
}

impl Session {
    pub fn start(mut config: Config, waker: PlatformWaker) -> anyhow::Result<Self> {
        let profile = std::path::absolute(
            config
                .identity_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("identity needs a profile directory"))?,
        )?;
        let previous = {
            let mut retiring = retiring_sessions()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            retiring.retain(|_, worker| !worker.is_finished());
            retiring.remove(&profile)
        };

        let thread_profile = profile.clone();
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
                    if let Some(previous) = previous {
                        let _ = previous.join();
                    }

                    if thread_stop.load(Ordering::Acquire) {
                        return Ok(());
                    }

                    let store = Arc::new(Store::open(&thread_profile)?);
                    config.store = Some(store.clone());
                    let client = Client::new(config)?;
                    let (auth, _) = client.identity()?;
                    let mut state = Worker {
                        client,
                        store,
                        auth,
                        authorized: false,
                        selected: None,
                        inflight: None,
                        status: "Connecting…".into(),
                        notices: VecDeque::new(),
                        generation: 0,
                        submission: None,
                    };

                    state.selected = state.store.selected()?.or(state
                        .store
                        .chats()?
                        .first()
                        .map(|c| c.id));
                    state.publish(&thread_snapshot, &waker)?;

                    if thread_stop.load(Ordering::Acquire) {
                        return Ok(());
                    }

                    state.client.start()?;
                    state.run(rx, &thread_wake, &thread_stop, &thread_snapshot, &waker)
                })();

                if let Err(error) = result {
                    if let Ok(mut snapshot) = thread_snapshot.lock() {
                        snapshot.authorized = false;
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
            profile,
        })
    }

    pub fn submit(&self, command: Command) -> anyhow::Result<u64> {
        let id = self.next_submission.fetch_add(1, Ordering::Relaxed);
        self.commands
            .try_send((id, command))
            .map_err(|e| anyhow::anyhow!("client command queue unavailable: {e}"))?;
        self.wake.write(1)?;
        Ok(id)
    }

    pub fn snapshot_since(&self, generation: u64) -> Option<Snapshot> {
        let snapshot = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        (snapshot.generation != generation).then(|| snapshot.clone())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        let _ = self.wake.write(1);

        if let Some(worker) = self.worker.take() {
            let mut retiring = retiring_sessions()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            retiring.retain(|_, worker| !worker.is_finished());
            retiring.insert(self.profile.clone(), worker);
        }
    }
}

fn retiring_sessions() -> &'static Mutex<HashMap<PathBuf, JoinHandle<()>>> {
    static RETIRING: OnceLock<Mutex<HashMap<PathBuf, JoinHandle<()>>>> = OnceLock::new();
    RETIRING.get_or_init(|| Mutex::new(HashMap::new()))
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
    inflight: Option<Inflight>,
    status: String,
    notices: VecDeque<String>,
    generation: u64,
    submission: Option<(u64, bool, Option<Id>)>,
}

impl Worker {
    fn notice(&mut self, line: impl Into<String>) {
        self.notices.push_back(line.into());

        while self.notices.len() > 8 {
            self.notices.pop_front();
        }
    }

    fn publish(&mut self, shared: &Mutex<Snapshot>, waker: &PlatformWaker) -> anyhow::Result<()> {
        let conversations = self.store.chats()?;
        let messages = conversations
            .iter()
            .find(|chat| Some(chat.id) == self.selected)
            .map(|chat| self.store.history(chat))
            .transpose()?
            .unwrap_or_default();
        self.generation += 1;
        let snapshot = Snapshot {
            auth: Some(self.auth),
            authorized: self.authorized,
            status: self.status.clone(),
            selected: self.selected,
            conversations: conversations.into(),
            messages: messages.into(),
            notices: self.notices.iter().cloned().collect(),
            generation: self.generation,
            submission: self.submission,
            pending: self.store.pending()?.is_some(),
            delivering: self.inflight.is_some(),
        };

        *shared
            .lock()
            .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))? = snapshot;
        waker.request_redraw()
    }

    fn send_pending(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(self.authorized, "not authorized yet");
        anyhow::ensure!(self.inflight.is_none(), "Waiting for delivery confirmation");
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

    fn deliver(
        &mut self,
        packet: GroupPacket,
        targets: Vec<Id>,
        metadata: Option<&Metadata>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.authorized, "not authorized yet");
        anyhow::ensure!(self.inflight.is_none(), "A delivery is still in progress");
        self.store.enqueue(&packet, &targets, metadata)?;

        if let Err(error) = self.send_pending() {
            self.status = "Delivery saved locally".into();
            self.notice(error.to_string());
        }

        Ok(())
    }

    fn command(&mut self, command: Command) -> anyhow::Result<Option<Id>> {
        match command {
            Command::Select(id) => {
                anyhow::ensure!(
                    self.store.conversation(id)?.is_some_and(|chat| chat.active),
                    "Conversation is unavailable"
                );
                self.store.select(id)?;
                self.selected = Some(id);
            }

            Command::Create { title, partners } => {
                anyhow::ensure!(!title.trim().is_empty(), "Enter a conversation name");
                anyhow::ensure!(
                    (1..faith_client::MAX_MEMBERS).contains(&partners.len()),
                    "Invalid number of chat partners"
                );
                let mut members = vec![self.auth];

                for (id, _) in &partners {
                    anyhow::ensure!(
                        !members.contains(id),
                        "Each partner needs a different Auth ID, excluding your own"
                    );
                    members.push(*id);
                }

                let conversation = Id::random()?;
                let packet = GroupPacket {
                    kind: GroupKind::Create,
                    revision: 0,
                    conversation,
                    message: Id::random()?,
                    members: members.clone(),
                    text: Vec::new(),
                };
                self.deliver(
                    packet,
                    members,
                    Some(&Metadata {
                        title,
                        partners: partners
                            .into_iter()
                            .filter(|(_, name)| !name.is_empty())
                            .map(|(id, name)| (id.0, name))
                            .collect(),
                    }),
                )?;
                self.selected = Some(conversation);
                return Ok(Some(conversation));
            }

            Command::Rename { id, title } => {
                anyhow::ensure!(!title.trim().is_empty(), "Enter a conversation name");
                self.store.rename(id, self.auth, title)?;
            }

            Command::Send { id, body } => {
                anyhow::ensure!(
                    !body.trim().is_empty() && body.len() <= faith_client::MAX_TEXT,
                    "Message must contain 1–4096 bytes"
                );
                let chat = self
                    .store
                    .conversation(id)?
                    .filter(|chat| chat.active)
                    .ok_or_else(|| anyhow::anyhow!("Conversation is unavailable"))?;
                let packet = GroupPacket {
                    kind: GroupKind::Message,
                    revision: chat.revision,
                    conversation: id,
                    message: Id::random()?,
                    members: chat.members.clone(),
                    text: body.into_bytes(),
                };
                self.deliver(packet, chat.members, None)?;
            }

            Command::Retry => self.send_pending()?,
            Command::Discard => {
                anyhow::ensure!(self.inflight.is_none(), "Wait for delivery to finish");
                self.store.discard_pending()?;
                self.status = if self.authorized {
                    "Connected"
                } else {
                    "Disconnected"
                }
                .into();
            }
        }

        Ok(None)
    }

    fn run(
        &mut self,
        rx: Receiver<(u64, Command)>,
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
                self.status = "Delivery unconfirmed".into();
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
                            self.selected = self.store.chats()?.first().map(|c| c.id);

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
                            self.status = "Delivery rejected".into();
                            self.notice(event.message);
                        }
                    }

                    EventKind::CommandFailed | EventKind::Error => {
                        self.inflight = None;
                        self.notice(event.message);
                        self.status = "Connection Failed.".into();
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

            while let Ok((id, command)) = rx.try_recv() {
                changed = true;

                let tracked = matches!(command, Command::Create { .. } | Command::Send { .. });
                let result = self.command(command);

                if tracked {
                    self.submission =
                        Some((id, result.is_ok(), result.as_ref().ok().copied().flatten()));
                }

                if let Err(error) = result {
                    self.notice(error.to_string());
                }
            }

            if changed {
                self.publish(shared, waker)?;
            }
        }
    }
}
