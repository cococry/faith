use super::mouse::{CursorPosition, MouseButton, ScrollDelta};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    SelectAll,
    Copy,
    Paste,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Escape,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    CloseRequested,
    /// UI zoom gesture steps; positive values zoom in.
    Zoom(f64),
    RedrawRequested,
    QuickShellRequested,
    Resized {
        width: u32,
        height: u32,
    },
    /// Committed UTF-8, after keyboard layout and compose processing.
    TextInput(String),
    ClipboardText {
        target: String,
        text: String,
    },
    KeyPressed {
        key: Key,
        control: bool,
    },
    FocusChanged(bool),
    FocusNext {
        backwards: bool,
    },
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
