mod store;

use crate::{platform::window::PlatformWaker, profiles::UsernameSettings};
use faith_client::{Client, Config, EventKind, GroupKind, GroupPacket, Id};
use nix::{
    poll::{PollFd, PollFlags, PollTimeout, poll},
    sys::eventfd::{EfdFlags, EventFd},
};

use std::{
    collections::{HashMap, HashSet, VecDeque},
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
pub use store::{legacy_aliases, profile_conversations, profile_usernames};

// These values are defined by the Faith wire protocol.
const REQUEST_EVENT: u32 = 2;
const RESPONSE_EVENT: u32 = 3;
const USERNAMES_EVENT: u32 = 4;
const REQUEST_COMMAND: u64 = 2;
const RESPONSE_COMMAND: u64 = 3;
const COMMAND_ACCEPTED: u64 = 2;
const RECONNECT_FORBIDDEN: u64 = 1;

#[derive(Clone)]
pub struct Request {
    pub conversation: Id,
    pub requester: Id,
    pub required: bool,
    pub username: String,
}

#[derive(Clone)]
pub struct PendingInvite {
    pub conversation: Id,
    pub title: String,
    pub title_members: Option<Arc<[Id]>>,
    pub recipient: Id,
    pub recipient_name: String,
    pub accepted: Option<bool>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Metadata {
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title_from: Option<[u8; 16]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    automatic_title: bool,
    partners: Vec<([u8; 16], String)>,
}

#[derive(Clone)]
pub struct Chat {
    pub id: Id,
    pub title: String,
    pub needs_name: bool,
    pub last: String,
    pub members: Vec<Id>,
    pub own_auth: Option<Id>,
    pub name_members: bool,
}

#[derive(Clone)]
pub struct Message {
    pub row_id: i64,
    pub sender: Id,
    pub author: String,
    pub body: String,
}

#[derive(Clone, Copy, Default)]
pub struct InvitationPolicy {
    pub required: bool,
    pub disclose: bool,
}

pub enum Command {
    Select(Id),
    Rename {
        id: Id,
        title: String,
    },
    Create {
        title: String,
        partners: Vec<Id>,
        policy: InvitationPolicy,
        automatic_title: bool,
    },
    Send {
        id: Id,
        body: String,
    },
    Respond {
        conversation: Id,
        accept: bool,
        disclose: bool,
    },
    Retry,
    Discard,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub auth: Option<Id>,
    pub authorized: bool,
    pub username: Option<Arc<str>>,
    pub auto_disclose: bool,
    pub reject_required: bool,
    pub auth_error: Option<String>,
    pub claim_completed: bool,
    pub status: String,
    pub selected: Option<Id>,
    pub conversations: Arc<[Chat]>,
    pub messages: Arc<[Message]>,
    pub disclosed: Arc<[(Id, String)]>,
    pub notices: Arc<[String]>,
    pub requests: Arc<[Request]>,
    pub pending_invites: Arc<[PendingInvite]>,
    pub responding: Option<Id>,
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
    pub fn start(
        mut config: Config,
        claim: Option<UsernameSettings>,
        waker: PlatformWaker,
    ) -> anyhow::Result<Self> {
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
                    let username = match &claim {
                        Some(settings) => {
                            use std::{io::Write, os::unix::fs::OpenOptionsExt};

                            anyhow::ensure!(
                                !thread_profile.join("username.json").exists(),
                                "This profile already has a username."
                            );
                            let mut pending = std::fs::OpenOptions::new()
                                .write(true)
                                .truncate(true)
                                .custom_flags(nix::libc::O_NOFOLLOW)
                                .open(thread_profile.join(".username-pending"))?;
                            pending.write_all(&serde_json::to_vec(settings)?)?;
                            pending.sync_all()?;
                            settings.clone()
                        }

                        None => UsernameSettings::load(&thread_profile)?,
                    };
                    let client = Client::new_with_username(
                        config,
                        claim
                            .as_ref()
                            .and_then(|settings| settings.username.as_deref()),
                    )?;
                    let (auth, _) = client.identity()?;
                    store.remember_auth(auth)?;
                    let disclosed = store.usernames()?.into();
                    let mut state = Worker {
                        disclosed,
                        client,
                        store,
                        auth,
                        authorized: false,
                        username,
                        sent_invites: HashSet::new(),
                        responding: None,
                        responding_requester: None,
                        responding_accept: false,
                        automatic_attempts: HashSet::new(),
                        claim_path: claim.map(|_| thread_profile.clone()),
                        auth_error: None,
                        claim_completed: false,
                        claim_deadline: None,
                        selected: None,
                        inflight: None,
                        status: "Connecting…".into(),
                        notices: VecDeque::new(),
                        generation: 0,
                        submission: None,
                    };

                    state.selected = state.store.selected()?.or(state
                        .store
                        .chats(auth)?
                        .first()
                        .map(|c| c.id));
                    state.publish(&thread_snapshot, &waker)?;

                    if thread_stop.load(Ordering::Acquire) {
                        return Ok(());
                    }

                    state.client.start()?;
                    if state.claim_path.is_some() {
                        state.claim_deadline =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
                    }

                    state.run(rx, &thread_wake, &thread_stop, &thread_snapshot, &waker)
                })();

                if let Err(error) = result {
                    if let Ok(mut snapshot) = thread_snapshot.lock() {
                        snapshot.authorized = false;
                        snapshot.status = format!("Client stopped: {error}");
                        snapshot.auth_error = Some(error.to_string());
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
    disclosed: Arc<[(Id, String)]>,
    client: Client,
    store: Arc<Store>,
    auth: Id,
    authorized: bool,
    username: UsernameSettings,
    claim_path: Option<PathBuf>,
    sent_invites: HashSet<Id>,
    responding: Option<Id>,
    responding_requester: Option<Id>,
    responding_accept: bool,
    automatic_attempts: HashSet<Id>,
    auth_error: Option<String>,
    claim_completed: bool,
    claim_deadline: Option<std::time::Instant>,
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
        let conversations = self.store.chats(self.auth)?;
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
            username: self.username.username.as_deref().map(Arc::from),
            auto_disclose: self.username.auto_disclose,
            reject_required: self.username.reject_required,
            auth_error: self.auth_error.clone(),
            claim_completed: self.claim_completed,
            status: self.status.clone(),
            selected: self.selected,
            conversations: conversations.into(),
            messages: messages.into(),
            disclosed: self.disclosed.clone(),
            notices: self.notices.iter().cloned().collect(),
            requests: self.store.requests()?.into(),
            pending_invites: self.store.pending_invites(self.auth)?.into(),
            responding: self.responding,
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
        if let Some(batch) = self.store.invitations(packet.conversation)? {
            if batch.responses.iter().any(|(_, accepted)| !accepted) {
                self.status = "A message request was declined. Discard the pending conversation to start over.".into();
                return Ok(());
            }

            let mut remaining = 0;

            for target in targets.iter().filter(|&&target| target != self.auth) {
                if batch.responses.iter().any(|(id, _)| id == target) {
                    continue;
                }

                remaining += 1;

                if !self.sent_invites.contains(target) {
                    self.client.request_conversation_with_disclosure(
                        packet.conversation,
                        *target,
                        batch.policy.required,
                        batch.policy.disclose,
                    )?;
                    self.sent_invites.insert(*target);
                }
            }

            if remaining != 0 {
                self.status = format!("Waiting for {remaining} message request response(s)");
                return Ok(());
            }
        }

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
        invite: Option<InvitationPolicy>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.authorized, "not authorized yet");
        anyhow::ensure!(self.inflight.is_none(), "A delivery is still in progress");
        self.store.enqueue(&packet, &targets, metadata, invite)?;

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

            Command::Create {
                title,
                partners,
                policy,
                automatic_title,
            } => {
                let not_accepted = self
                    .store
                    .pending_invites(self.auth)?
                    .iter()
                    .filter(|invite| invite.accepted != Some(true))
                    .count();
                anyhow::ensure!(
                    not_accepted == 0,
                    "Cannot create group, {not_accepted} participant(s) did not accept your previous request yet"
                );
                anyhow::ensure!(
                    !(policy.required || policy.disclose) || self.username.username.is_some(),
                    "Claim a username before choosing username disclosure"
                );
                anyhow::ensure!(!title.trim().is_empty(), "Enter a conversation name");
                anyhow::ensure!(
                    (1..faith_client::MAX_MEMBERS).contains(&partners.len()),
                    "Invalid number of chat partners"
                );
                let mut members = vec![self.auth];

                for id in partners {
                    anyhow::ensure!(
                        !members.contains(&id),
                        "Each partner needs a different Auth ID, excluding your own"
                    );
                    members.push(id);
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
                        title: if automatic_title {
                            String::new()
                        } else {
                            title
                        },
                        title_from: None,
                        automatic_title,
                        partners: Vec::new(),
                    }),
                    Some(policy),
                )?;
                self.selected = Some(conversation);
                return Ok(Some(conversation));
            }

            Command::Rename { id, title } => {
                anyhow::ensure!(!title.trim().is_empty(), "Enter a conversation name");
                self.store.rename(id, title)?;
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
                self.deliver(packet, chat.members, None, None)?;
            }

            Command::Respond {
                conversation,
                accept,
                disclose,
            } => {
                let request = self
                    .store
                    .requests()?
                    .into_iter()
                    .find(|request| request.conversation == conversation)
                    .ok_or_else(|| anyhow::anyhow!("Message request is unavailable"))?;
                self.respond(&request, accept, disclose)?;
            }

            Command::Retry => {
                self.sent_invites.clear();
                self.send_pending()?;
            }

            Command::Discard => {
                anyhow::ensure!(self.inflight.is_none(), "Wait for delivery to finish");
                self.store.discard_pending()?;
                self.sent_invites.clear();
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

    fn respond(&mut self, request: &Request, accept: bool, disclose: bool) -> anyhow::Result<()> {
        anyhow::ensure!(self.authorized, "Connect to respond to message requests");
        anyhow::ensure!(
            self.responding.is_none(),
            "Waiting for a message request response to finish"
        );
        anyhow::ensure!(
            !accept || !request.required || self.username.username.is_some(),
            "A username is required to accept this request"
        );
        anyhow::ensure!(
            !accept || !request.required || !self.username.reject_required,
            "This profile automatically rejects requests that require username disclosure"
        );
        let disclose = accept && !request.required && self.username.username.is_some() && disclose;
        self.client.respond_conversation_with_policy(
            request.conversation,
            request.requester,
            accept,
            request.required,
            disclose,
        )?;
        self.responding = Some(request.conversation);
        self.responding_requester = Some(request.requester);
        self.responding_accept = accept;
        Ok(())
    }

    fn reject_required_requests(&mut self) -> anyhow::Result<()> {
        if !self.authorized || !self.username.reject_required || self.responding.is_some() {
            return Ok(());
        }

        for request in self.store.requests()? {
            if request.required && self.automatic_attempts.insert(request.conversation) {
                match self.respond(&request, false, false) {
                    Ok(()) => break,
                    Err(error) => self.notice(format!("Could not reject message request: {error}")),
                }
            }
        }

        Ok(())
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
            let deadline = self
                .inflight
                .as_ref()
                .map(|pending| pending.deadline)
                .or(self.claim_deadline);
            let timeout = deadline.map_or(PollTimeout::NONE, |deadline| {
                let millis = deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis()
                    .saturating_add(1)
                    .min(i32::MAX as u128) as u32;
                PollTimeout::try_from(millis).unwrap_or(PollTimeout::MAX)
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
                        if let Some(path) = self.claim_path.take() {
                            std::fs::rename(
                                path.join(".username-pending"),
                                path.join("username.json"),
                            )?;
                            self.claim_completed = true;
                            self.claim_deadline = None;
                        }

                        self.auth_error = None;
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

                    EventKind::ServerDisconnect if event.value0 == RECONNECT_FORBIDDEN => {
                        self.authorized = false;
                        self.inflight = None;
                        self.claim_deadline = None;
                        let error = if event.message.is_empty() {
                            "Authentication failed.".to_owned()
                        } else {
                            event.message
                        };
                        self.auth_error = Some(error.clone());
                        self.status = error;
                    }

                    EventKind::Disconnected | EventKind::ServerDisconnect => {
                        self.authorized = false;
                        self.inflight = None;
                        self.sent_invites.clear();
                        self.automatic_attempts.clear();
                        self.responding = None;
                        self.responding_requester = None;
                        self.responding_accept = false;

                        if self.auth_error.is_none() {
                            self.status = "Disconnected; awaiting reconnect".into();
                        }
                    }

                    EventKind::ConversationUsernames | EventKind::ConversationRequest => {
                        self.disclosed = self.store.usernames()?.into();
                    }

                    EventKind::ConversationResponse => {
                        if self.authorized
                            && self.inflight.is_none()
                            && self
                                .store
                                .pending()?
                                .is_some_and(|(packet, _)| packet.conversation == event.ids[0])
                            && let Err(error) = self.send_pending()
                        {
                            self.notice(format!("Could not finish conversation creation: {error}"));
                        }
                    }

                    EventKind::CommandResult
                        if event.value0 == REQUEST_COMMAND
                            && event.value1 != COMMAND_ACCEPTED
                            && !self.sent_invites.is_empty()
                            && self.inflight.is_none() =>
                    {
                        self.status = "A message request could not be sent.".into();
                        self.notice(event.message);
                    }

                    EventKind::CommandResult if event.value0 == RESPONSE_COMMAND => {
                        if let Some(conversation) = self.responding.take() {
                            if event.value1 == COMMAND_ACCEPTED {
                                if self.responding_accept
                                    && let Some(requester) = self.responding_requester.take()
                                {
                                    self.store.accept_request(
                                        conversation,
                                        requester,
                                        self.auth,
                                    )?;
                                    if self.selected.is_none() {
                                        self.selected = Some(conversation);
                                        self.store.select(conversation)?;
                                    }
                                } else {
                                    self.responding_requester = None;
                                    self.store.remove_request(conversation)?;
                                }
                                self.responding_accept = false;
                            } else {
                                self.responding_requester = None;
                                self.responding_accept = false;
                                self.notice(event.message);
                            }
                        }
                    }

                    EventKind::GroupReceived => {
                        if self.selected.is_none() {
                            self.selected = self.store.chats(self.auth)?.first().map(|c| c.id);

                            if let Some(id) = self.selected {
                                self.store.select(id)?;
                            }
                        }

                        if self.store.pending()?.is_none()
                            && let Some(inflight) = &mut self.inflight
                        {
                            inflight.echo = true;
                            self.sent_invites.clear();
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

            if self
                .claim_deadline
                .is_some_and(|deadline| std::time::Instant::now() >= deadline)
            {
                anyhow::bail!("Authentication timed out. Try again.");
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
                self.reject_required_requests()?;
                self.publish(shared, waker)?;
            }
        }
    }
}
