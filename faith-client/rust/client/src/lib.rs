//! Owned, thread-confined access to the native Faith client.
//!
//! Storage callbacks run on the native reader thread. Returning success permits
//! automatic ACK, so implementations must durably commit before returning.
use faith_client_sys as sys;
use std::{
    ffi::{CString, c_void},
    fmt,
    marker::PhantomData,
    os::fd::{AsRawFd, BorrowedFd},
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, OnceLock},
};

pub const GROUP_PACKET_EVENT: u32 = sys::FAITH_EVENT_GROUP_PACKET;

pub const MAX_MEMBERS: usize = sys::FAITH_GROUP_MAX_MEMBERS as usize;
pub const MAX_TEXT: usize = sys::FAITH_GROUP_MAX_TEXT as usize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(pub [u8; 16]);

impl Id {
    pub fn random() -> Result<Self> {
        let mut id = Self::default();
        status(unsafe { sys::faith_random_bytes(id.0.as_mut_ptr(), 16) })?;
        Ok(id)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl std::str::FromStr for Id {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        if s.len() != 32 || !s.is_ascii() {
            return Err(Error::Invalid("expected a 32-character hexadecimal ID"));
        }
        let mut id = Self::default();
        for (i, byte) in id.0.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|_| Error::Invalid("invalid hexadecimal ID"))?;
        }
        Ok(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Native(u32),
    Invalid(&'static str),
    CreateFailed,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(s) => write!(f, "Faith status {s}"),
            Self::Invalid(s) => f.write_str(s),
            Self::CreateFailed => {
                f.write_str("could not create Faith client (check identity and configuration)")
            }
        }
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn status(s: sys::faith_status_code_t) -> Result<()> {
    if s == sys::FAITH_OK {
        Ok(())
    } else {
        Err(Error::Native(s))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Create,
    Update,
    Message,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPacket {
    pub kind: GroupKind,
    pub revision: u64,
    pub conversation: Id,
    pub message: Id,
    pub members: Vec<Id>,
    pub text: Vec<u8>,
}

impl GroupPacket {
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut raw = sys::faith_group_packet_t::default();
        status(unsafe { sys::faith_group_decode(data.as_ptr(), data.len(), &mut raw) })?;
        let kind = match raw.kind {
            sys::FAITH_GROUP_CREATE => GroupKind::Create,
            sys::FAITH_GROUP_UPDATE => GroupKind::Update,
            sys::FAITH_GROUP_MESSAGE => GroupKind::Message,
            _ => return Err(Error::Invalid("unknown group kind")),
        };
        Ok(Self {
            kind,
            revision: raw.revision,
            conversation: Id(raw.conversation_id),
            message: Id(raw.message_id),
            members: raw.members[..raw.n_members as usize]
                .iter()
                .map(|m| Id(m.bytes))
                .collect(),
            text: unsafe { std::slice::from_raw_parts(raw.text, raw.text_size as usize) }.to_vec(),
        })
    }

    fn raw(&self) -> Result<sys::faith_group_packet_t> {
        if self.members.is_empty() || self.members.len() > MAX_MEMBERS || self.text.len() > MAX_TEXT
        {
            return Err(Error::Invalid("group packet exceeds protocol limits"));
        }
        let mut raw = sys::faith_group_packet_t {
            format: sys::FAITH_GROUP_PLAINTEXT_V1 as u8,
            kind: match self.kind {
                GroupKind::Create => sys::FAITH_GROUP_CREATE,
                GroupKind::Update => sys::FAITH_GROUP_UPDATE,
                GroupKind::Message => sys::FAITH_GROUP_MESSAGE,
            },
            revision: self.revision,
            conversation_id: self.conversation.0,
            message_id: self.message.0,
            n_members: self.members.len() as u16,
            text: self.text.as_ptr(),
            text_size: self.text.len() as u32,
            ..Default::default()
        };
        for (dst, src) in raw.members.iter_mut().zip(&self.members) {
            dst.bytes = src.0;
        }
        Ok(raw)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let raw = self.raw()?;
        let mut data = vec![0; sys::FAITH_GROUP_MAX_PACKET as usize];
        let mut size = 0;
        status(unsafe { sys::faith_group_encode(data.as_mut_ptr(), data.len(), &mut size, &raw) })?;
        data.truncate(size);
        Ok(data)
    }
}

pub struct StoredEvent<'a> {
    pub auth: Id,
    pub device: Id,
    pub sequence: u64,
    pub kind: u32,
    pub data: &'a [u8],
}

pub trait EventStore: Send + Sync + 'static {
    fn store(&self, event: StoredEvent<'_>) -> Result<()>;
}

struct StoreContext(Arc<dyn EventStore>);
unsafe extern "C" fn store_event(
    context: *mut c_void,
    auth: *const sys::faith_auth_id_t,
    device: *const sys::faith_device_id_t,
    sequence: u64,
    kind: u32,
    data: *const u8,
    size: usize,
) -> sys::faith_status_code_t {
    // Native client owns this callback's invocation; Client retains the box until
    // destroy has joined its threads. A panic must never unwind across C.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if context.is_null() || auth.is_null() || device.is_null() || (size > 0 && data.is_null()) {
            return Err(Error::Invalid("invalid callback arguments"));
        }
        let context = unsafe { &*context.cast::<StoreContext>() };
        let data = if size == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(data, size) }
        };
        context.0.store(StoredEvent {
            auth: Id(unsafe { (*auth).bytes }),
            device: Id(unsafe { (*device).bytes }),
            sequence,
            kind,
            data,
        })
    }));
    match outcome {
        Ok(Ok(())) => sys::FAITH_OK,
        Ok(Err(Error::Native(s))) => s,
        _ => sys::FAITH_ERR_IO,
    }
}

pub struct Config {
    pub host: String,
    pub port: u16,
    pub server_name: Option<String>,
    pub ca_file: Option<PathBuf>,
    pub identity_path: PathBuf,
    pub store: Option<Arc<dyn EventStore>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    None,
    Connecting,
    Connected,
    Disconnected,
    Pong,
    Error,
    MessageReceived,
    DeviceAuthPending,
    DeviceLinkRequest,
    DeviceAuthResponseAck,
    DeviceAuthResponseFailed,
    DeviceLinkCancelled,
    Authorized,
    ServerDisconnect,
    CommandResult,
    CommandFailed,
    ConversationReady,
    GroupReceived,
    Unknown(u32),
}

#[derive(Debug, Clone)]
pub struct Event {
    pub kind: EventKind,
    pub value0: u64,
    pub value1: u64,
    pub ids: [Id; 3],
    pub message: String,
    pub data: Vec<u8>,
}

impl Event {
    pub fn group_result(&self) -> Option<bool> {
        (self.kind == EventKind::CommandResult
            && self.value0 == u64::from(sys::FAITH_COMMAND_GROUP_DELIVER))
        .then_some(self.value1 == u64::from(sys::FAITH_COMMAND_RESULT_ACCEPTED))
    }
}

pub struct Client {
    raw: NonNull<sys::faith_client_t>,
    _store: Option<Box<StoreContext>>,
    // Keep native API use on its owner thread; callbacks receive only EventStore.
    _thread: PhantomData<Rc<()>>,
}

impl Client {
    pub fn new(config: Config) -> Result<Self> {
        static INIT: OnceLock<sys::faith_status_code_t> = OnceLock::new();
        status(*INIT.get_or_init(|| unsafe { sys::faith_client_init_global(0) }))?;
        fn string(bytes: &[u8]) -> Result<CString> {
            CString::new(bytes).map_err(|_| Error::Invalid("configuration contains NUL"))
        }
        let host = string(config.host.as_bytes())?;
        if host.as_bytes().len() >= 256 || config.port == 0 {
            return Err(Error::Invalid("invalid host or port"));
        }
        let name = config
            .server_name
            .as_deref()
            .map(|s| string(s.as_bytes()))
            .transpose()?;
        if name.as_ref().is_some_and(|s| s.as_bytes().len() >= 256) {
            return Err(Error::Invalid("server name too long"));
        }
        let ca = config
            .ca_file
            .as_ref()
            .map(|p| string(p.as_os_str().as_bytes()))
            .transpose()?;
        if ca.as_ref().is_some_and(|s| s.as_bytes().len() >= 512) {
            return Err(Error::Invalid("CA path too long"));
        }
        let identity = string(config.identity_path.as_os_str().as_bytes())?;
        let mut store = config.store.map(|s| Box::new(StoreContext(s)));
        let raw_config = sys::faith_client_config_t {
            host: host.as_ptr(),
            port: config.port,
            identity_path: identity.as_ptr(),
            server_name: name.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            ca_file: ca.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            store_event: store
                .as_ref()
                .map(|_| store_event as unsafe extern "C" fn(_, _, _, _, _, _, _) -> _),
            store_event_context: store.as_mut().map_or(std::ptr::null_mut(), |s| {
                (&mut **s as *mut StoreContext).cast()
            }),
            ..Default::default()
        };
        let raw = NonNull::new(unsafe { sys::faith_client_create(&raw_config) })
            .ok_or(Error::CreateFailed)?;
        Ok(Self {
            raw,
            _store: store,
            _thread: PhantomData,
        })
    }

    pub fn start(&self) -> Result<()> {
        status(unsafe { sys::faith_client_start(self.raw.as_ptr()) })
    }
    pub fn stop(&self) -> Result<()> {
        status(unsafe { sys::faith_client_stop(self.raw.as_ptr()) })
    }
    pub fn reconnect(&self) -> Result<()> {
        status(unsafe { sys::faith_client_reconnect(self.raw.as_ptr()) })
    }
    pub fn approve_pending_device(&self) -> Result<()> {
        status(unsafe { sys::faith_client_approve_pending_device_auth(self.raw.as_ptr()) })
    }
    pub fn deny_pending_device(&self) -> Result<()> {
        status(unsafe { sys::faith_client_deny_pending_device_auth(self.raw.as_ptr()) })
    }
    /// Manual ACK is for callers without a storage callback, after durable commit.
    pub fn ack_events(&self, through: u64) -> Result<()> {
        status(unsafe { sys::faith_client_ack_events(self.raw.as_ptr(), through) })
    }
    pub fn event_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(sys::faith_client_event_fd(self.raw.as_ptr())) }
    }
    pub fn identity(&self) -> Result<(Id, Id)> {
        let mut auth = sys::faith_auth_id_t::default();
        let mut device = sys::faith_device_id_t::default();
        status(unsafe { sys::faith_client_identity(self.raw.as_ptr(), &mut auth, &mut device) })?;
        Ok((Id(auth.bytes), Id(device.bytes)))
    }
    pub fn deliver_group(&self, packet: &GroupPacket, targets: &[Id]) -> Result<()> {
        if targets.is_empty() || targets.len() > sys::FAITH_GROUP_MAX_TARGETS as usize {
            return Err(Error::Invalid("invalid delivery targets"));
        }
        let raw = packet.raw()?;
        let targets: Vec<_> = targets
            .iter()
            .map(|id| sys::faith_auth_id_t { bytes: id.0 })
            .collect();
        status(unsafe {
            sys::faith_client_deliver_group(
                self.raw.as_ptr(),
                &raw,
                targets.as_ptr(),
                targets.len(),
            )
        })
    }
    /// Consume the eventfd counter before draining next_event(). Reading does not
    /// remove events; a racing producer signals again, avoiding lost wakeups.
    pub fn clear_event_signal(&self) -> std::io::Result<()> {
        let mut counter = [0u8; 8];
        loop {
            let n =
                unsafe { libc_read(self.event_fd().as_raw_fd(), counter.as_mut_ptr().cast(), 8) };
            if n >= 0 {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(());
            }
            return Err(error);
        }
    }
    pub fn next_event(&self) -> Result<Option<Event>> {
        let mut raw = sys::faith_event_t::default();
        let rc = unsafe { sys::faith_client_next_event(self.raw.as_ptr(), &mut raw) };
        if rc == sys::FAITH_ERR_UNDERFLOW {
            return Ok(None);
        }
        status(rc)?;
        struct Free(sys::faith_event_t);
        impl Drop for Free {
            fn drop(&mut self) {
                unsafe {
                    sys::faith_client_free_event(&mut self.0);
                }
            }
        }
        let owned = Free(raw);
        let raw = &owned.0;
        let kind = match raw.type_ {
            sys::FAITH_EVENT_NONE => EventKind::None,
            sys::FAITH_EVENT_CONNECTING => EventKind::Connecting,
            sys::FAITH_EVENT_CONNECTED => EventKind::Connected,
            sys::FAITH_EVENT_DISCONNECTED => EventKind::Disconnected,
            sys::FAITH_EVENT_PONG => EventKind::Pong,
            sys::FAITH_EVENT_ERROR => EventKind::Error,
            sys::FAITH_EVENT_MESSAGE_RECEIVED => EventKind::MessageReceived,
            sys::FAITH_EVENT_DEVICE_AUTH_PENDING => EventKind::DeviceAuthPending,
            sys::FAITH_EVENT_DEVICE_LINK_REQUEST => EventKind::DeviceLinkRequest,
            sys::FAITH_EVENT_DEVICE_AUTH_RESPONSE_ACK => EventKind::DeviceAuthResponseAck,
            sys::FAITH_EVENT_DEVICE_AUTH_RESPONSE_FAILED => EventKind::DeviceAuthResponseFailed,
            sys::FAITH_EVENT_DEVICE_LINK_CANCELLED => EventKind::DeviceLinkCancelled,
            sys::FAITH_EVENT_AUTHORIZED => EventKind::Authorized,
            sys::FAITH_EVENT_SERVER_DISCONNECT => EventKind::ServerDisconnect,
            sys::FAITH_EVENT_COMMAND_RESULT => EventKind::CommandResult,
            sys::FAITH_EVENT_COMMAND_FAILED => EventKind::CommandFailed,
            sys::FAITH_EVENT_CONVERSATION_READY => EventKind::ConversationReady,
            sys::FAITH_EVENT_GROUP_RECEIVED => EventKind::GroupReceived,
            n => EventKind::Unknown(n),
        };
        let bytes: Vec<_> = raw
            .message
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as u8)
            .collect();
        let data = if raw.chat_message_size == 0 {
            Vec::new()
        } else {
            if raw.chat_message.is_null() {
                return Err(Error::Invalid("native event has null payload"));
            }
            unsafe {
                std::slice::from_raw_parts(raw.chat_message.cast::<u8>(), raw.chat_message_size)
            }
            .to_vec()
        };
        Ok(Some(Event {
            kind,
            value0: raw.value0,
            value1: raw.value1,
            ids: [Id(raw.value0_128), Id(raw.value1_128), Id(raw.value2_128)],
            message: String::from_utf8_lossy(&bytes).into_owned(),
            data,
        }))
    }
}

unsafe extern "C" {
    #[link_name = "read"]
    fn libc_read(fd: i32, buf: *mut c_void, size: usize) -> isize;
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            sys::faith_client_destroy(self.raw.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_and_packets_roundtrip() {
        let id = Id::random().unwrap();
        assert_eq!(id.to_string().parse::<Id>().unwrap(), id);
        assert!("é".repeat(16).parse::<Id>().is_err());
        let p = GroupPacket {
            kind: GroupKind::Message,
            revision: 3,
            conversation: id,
            message: Id::random().unwrap(),
            members: vec![id],
            text: vec![b'x'; MAX_TEXT],
        };
        let bytes = p.encode().unwrap();
        assert_eq!(GroupPacket::decode(&bytes).unwrap(), p);
        assert!(GroupPacket::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut duplicate = p;
        duplicate.members.push(id);
        assert!(duplicate.encode().is_err());
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    #[test]
    fn identity_persists_and_unstarted_client_drops_cleanly() {
        let root = std::env::temp_dir().join(format!("faith-ffi-{}", Id::random().unwrap()));
        std::fs::create_dir(&root).unwrap();
        let config = || Config {
            host: "127.0.0.1".into(),
            port: 1,
            server_name: None,
            ca_file: None,
            identity_path: root.join("identity.bin"),
            store: None,
        };
        let client = Client::new(config()).unwrap();
        let original = client.identity().unwrap();
        assert!(client.next_event().unwrap().is_none());
        assert!(client.ack_events(0).is_err());
        drop(client);
        let client = Client::new(config()).unwrap();
        assert_eq!(client.identity().unwrap(), original);
        drop(client);
        std::fs::write(root.join("identity.bin"), b"broken identity").unwrap();
        assert!(Client::new(config()).is_err());
        assert_eq!(
            std::fs::read(root.join("identity.bin")).unwrap(),
            b"broken identity"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    struct Panics;
    impl EventStore for Panics {
        fn store(&self, _: StoredEvent<'_>) -> Result<()> {
            panic!("simulated storage panic")
        }
    }
    #[test]
    fn callback_panics_fail_closed_without_crossing_ffi() {
        let mut context = Box::new(StoreContext(Arc::new(Panics)));
        let auth = sys::faith_auth_id_t::default();
        let device = sys::faith_device_id_t::default();
        let result = unsafe {
            store_event(
                (&mut *context as *mut StoreContext).cast(),
                &auth,
                &device,
                0,
                0,
                std::ptr::null(),
                0,
            )
        };
        assert_eq!(result, sys::FAITH_ERR_IO);
    }
}
