pub enum WindowEvent {
    CloseRequested,
    RedrawRequested,
    Resized {
        width: u32,
        height: u32,
    },
    #[allow(dead_code)]
    KeyPressed {
        keycode: u32,
    },
    #[allow(dead_code)]
    KeyReleased {
        keycode: u32,
    },
}
