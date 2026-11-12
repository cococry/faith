use super::event::{Key, WindowEvent};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};
use xkbcommon::xkb::{self, compose};

/// Shared layout/compose translation. Platform backends supply keymaps and
/// modifier masks. The ui framework never interprets hardware keycodes.
pub struct Keyboard {
    pub state: xkb::State,
    pub keymap: xkb::Keymap,
    compose: Option<compose::State>,
}

impl Keyboard {
    pub fn new(context: &xkb::Context, keymap: xkb::Keymap, state: xkb::State) -> Self {
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .filter_map(std::env::var_os)
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| OsString::from("C.UTF-8"));
        let compose = compose::Table::new_from_locale(context, &locale, compose::COMPILE_NO_FLAGS)
            .ok()
            .map(|t| compose::State::new(&t, compose::STATE_NO_FLAGS));
        Self {
            state,
            keymap,
            compose,
        }
    }

    pub fn reset(&mut self) {
        if let Some(compose) = &mut self.compose {
            compose.reset();
        }
    }

    pub fn press(&mut self, code: u32) -> Vec<WindowEvent> {
        let code = xkb::Keycode::new(code);
        let sym = self.state.key_get_one_sym(code).raw();
        let control = self
            .state
            .mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
        let alt = self
            .state
            .mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE);
        let key = match sym {
            0xff0d | 0xff8d => Some(Key::Enter),
            0xff08 => Some(Key::Backspace),
            0xffff | 0xff9f => Some(Key::Delete),
            0xff51 | 0xff96 => Some(Key::Left),
            0xff53 | 0xff98 => Some(Key::Right),
            0xff50 | 0xff95 => Some(Key::Home),
            0xff57 | 0xff9c => Some(Key::End),
            0xff1b => Some(Key::Escape),
            0xff55 => Some(Key::PageUp),
            0xff56 => Some(Key::PageDown),
            _ => None,
        };
        if let Some(key) = key {
            self.reset();
            return vec![WindowEvent::KeyPressed { key, control }];
        }
        if control || alt {
            return Vec::new();
        }
        let text = if let Some(compose) = &mut self.compose {
            compose.feed(xkb::Keysym::new(sym));
            match compose.status() {
                compose::Status::Composing => return Vec::new(),
                compose::Status::Cancelled => {
                    compose.reset();
                    return Vec::new();
                }
                compose::Status::Composed => {
                    let text = compose.utf8().unwrap_or_default();
                    compose.reset();
                    text
                }
                compose::Status::Nothing => self.state.key_get_utf8(code),
            }
        } else {
            self.state.key_get_utf8(code)
        };
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if text.is_empty() {
            Vec::new()
        } else {
            vec![WindowEvent::TextInput(text)]
        }
    }
}

/// Deadlines exist only while a repeatable key is held. No idle timer ticks.
#[derive(Default)]
pub struct KeyRepeat {
    rate: u32,
    delay: u32,
    held: Option<(u32, Instant)>,
}
impl KeyRepeat {
    pub fn configure(&mut self, rate: i32, delay: i32) {
        self.rate = rate.clamp(0, 1000) as u32;
        self.delay = delay.max(0) as u32;
        self.cancel();
    }
    pub fn press(&mut self, key: u32, repeats: bool) {
        if !repeats {
            return;
        }
        self.held = if self.rate > 0 {
            Some((
                key,
                Instant::now() + Duration::from_millis(self.delay as u64),
            ))
        } else {
            None
        };
    }
    pub fn release(&mut self, key: u32) {
        if self.held.is_some_and(|(k, _)| k == key) {
            self.cancel();
        }
    }
    pub fn cancel(&mut self) {
        self.held = None;
    }
    pub fn timeout(&self) -> nix::poll::PollTimeout {
        self.held.map_or(nix::poll::PollTimeout::NONE, |(_, at)| {
            nix::poll::PollTimeout::try_from(
                at.saturating_duration_since(Instant::now())
                    .as_millis()
                    .saturating_add(1)
                    .min(i32::MAX as u128) as u32,
            )
            .unwrap_or(nix::poll::PollTimeout::MAX)
        })
    }
    pub fn due(&mut self) -> Option<u32> {
        let (key, at) = self.held?;
        if Instant::now() < at {
            return None;
        }
        self.held = Some((
            key,
            Instant::now() + Duration::from_micros(1_000_000 / self.rate as u64),
        ));
        Some(key)
    }
}
