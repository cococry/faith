use faith_client_sys as sys;
use std::ffi::NulError;

pub struct Client {
    raw: NonNull<sys::faith_client_t>,
}

#[derive(Debug)]
pub enum Error {
    CreateFailed,
    InvalidCString(NulError),
    Faith(u32),
}

impl From<NulError> for Error {
    fn from(err: NulError) -> Self {
        Error::InvalidCString(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

impl Client {
    pub fn new(cfg: &sys::faith_client_config_t) -> Result<Self, Error> {
        let ptr = unsafe {
            sys::faith_client_create(cfg)
        };

        let raw = NonNull::new(ptr)
            .ok_or(Error::CreateFailed)?;

        Ok(Self { raw })
    }
}

pub enum Event {
    Error,
    Connecting,
    Connected,
    Authorized,
    ServerDisconnect,
    Pong,
    MessageReceived,
    DeviceAuthPending,
    DeviceLinkRequest,
    DeviceAuthResponseAck,
    DeviceAuthResponseFailed,
    DeviceLinkCancelled,
    CommandResult,
    CommandFailed
}
