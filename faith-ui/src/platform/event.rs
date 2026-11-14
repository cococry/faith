use super::mouse::{CursorPosition, MouseButton, ScrollDelta};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Escape,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    CloseRequested,
    RedrawRequested,
    Resized {
        width: u32,
        height: u32,
    },
    /// Committed UTF-8, after keyboard layout and compose processing.
    TextInput(String),
    KeyPressed {
        key: Key,
        control: bool,
    },
    FocusChanged(bool),
    MouseEntered {
        position: CursorPosition,
    },
    MouseLeft,
    MouseMoved {
        position: CursorPosition,
    },
    MouseButtonPressed {
        button: MouseButton,
    },
    MouseButtonReleased {
        button: MouseButton,
    },
    MouseScrolled {
        delta: ScrollDelta,
    },
    /// Pointer device removed or pointer interaction cancelled.
    MouseCancelled,
}
