pub enum WindowEvent {
    CloseRequested,
    RedrawRequested,
    Resized {
        width: u32,
        height: u32,
    },
    KeyPressed {
        keycode: u32
    },
    KeyReleased {
        keycode: u32
    }
}
