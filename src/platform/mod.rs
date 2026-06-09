pub mod event;
mod wayland;
pub mod window;
mod x11;

pub use self::event::WindowEvent;
pub use self::window::{Platform, WindowConfig};
