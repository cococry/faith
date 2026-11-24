use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    platform::window::PlatformWaker,
    ui::{animation::Curve, theme::DEFAULT_ID},
};

const VISIBLE: Duration = Duration::from_secs(2);
const FADE: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub font: Option<crate::system_fonts::Family>,
    pub rounded_corners: bool,
    #[serde(default = "animations_default")]
    pub animations: bool,
    pub layout_morphing: bool,
    pub morph_duration_ms: u32,
    pub morph_curve: Curve,
    pub morph_strength: u32,
    pub animation_curve: Curve,
    pub smooth_scrolling: bool,
    pub zoom_step: u32,
    pub default_zoom: u32,
    pub message_time_24h: bool,
    pub last_profile: Option<String>,
}

impl Settings {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                let mut settings: Self = serde_json::from_slice(&bytes)
                    .with_context(|| format!("Could not read settings from {}", path.display()))?;
                settings.morph_strength = settings.morph_strength.min(100);
                settings.morph_duration_ms = settings.morph_duration_ms.clamp(20, 1000);
                settings.zoom_step = settings.zoom_step.clamp(1, 50);
                settings.default_zoom = crate::ui::widgets::slider::Options {
                    min: 50,
                    max: 200,
                    step: 10,
                    suffix: "%",
                }
                .snap(settings.default_zoom as f64);
                Ok(settings)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).context("Could not load settings"),
        }
    }

    fn save(self, path: &Path) -> anyhow::Result<()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let temporary =
            path.with_file_name(format!(".settings-{}-{nonce}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;

        let result = (|| {
            serde_json::to_writer_pretty(&mut file, &self)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            fs::File::open(path.parent().context("Settings path has no parent")?)?.sync_all()?;
            Ok(())
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }

        result
    }
}

pub struct Saved {
    pub revision: u64,
    pub result: Result<Instant, String>,
}

pub struct Store {
    sender: Option<Sender<(u64, Settings)>>,
    results: Receiver<Saved>,
    worker: Option<JoinHandle<()>>,
    revision: u64,
    confirm: bool,
}

impl Store {
    pub fn start(path: PathBuf, waker: PlatformWaker) -> anyhow::Result<Self> {
        let (sender, receiver) = mpsc::channel::<(u64, Settings)>();
        let (results, updates) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("settings".into())
            .spawn(move || {
                loop {
                    let command = receiver.recv().ok();

                    let Some(mut command) = command else {
                        break;
                    };

                    while let Ok(newer) = receiver.try_recv() {
                        command = newer;
                    }

                    let result = command
                        .1
                        .save(&path)
                        .map(|()| Instant::now())
                        .map_err(|error| format!("Could not save settings: {error:#}"));
                    let _ = results.send(Saved {
                        revision: command.0,
                        result,
                    });
                    let _ = waker.request_redraw();
                }
            })?;

        Ok(Self {
            sender: Some(sender),
            results: updates,
            worker: Some(worker),
            revision: 0,
            confirm: false,
        })
    }

    pub fn save(&mut self, settings: Settings, confirm: bool) {
        self.revision += 1;
        self.confirm |= confirm;
        let _ = self
            .sender
            .as_ref()
            .unwrap()
            .send((self.revision, settings));
    }

    pub fn poll(&mut self) -> Option<Result<Option<Instant>, String>> {
        let saved = self
            .results
            .try_iter()
            .filter(|saved| saved.revision == self.revision)
            .last()?;
        let confirm = std::mem::take(&mut self.confirm);
        Some(saved.result.map(|time| confirm.then_some(time)))
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.sender.take();

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn confirmation_opacity(saved: Instant, enabled: bool, curve: Curve) -> f32 {
    let elapsed = saved.elapsed();

    if !enabled {
        return if elapsed < VISIBLE {
            1.0
        } else {
            0.0
        };
    }

    1.0 - curve.sample(elapsed.saturating_sub(VISIBLE - FADE).as_secs_f32() / FADE.as_secs_f32())
}

fn animations_default() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: DEFAULT_ID.to_owned(),
            font: None,
            rounded_corners: true,
            animations: true,
            layout_morphing: true,
            morph_duration_ms: crate::ui::animation::MORPH_DURATION.as_millis() as u32,
            morph_curve: Curve::Cubic,
            morph_strength: 50,
            animation_curve: Curve::default(),
            smooth_scrolling: false,
            zoom_step: 10,
            default_zoom: 110,
            message_time_24h: false,
            last_profile: None,
        }
    }
}

pub fn confirmation_deadline(saved: Instant, now: Instant, enabled: bool) -> Option<Instant> {
    let end = saved + VISIBLE;

    if now >= end {
        None
    } else if !enabled {
        Some(end)
    } else {
        Some((saved + VISIBLE - FADE).max(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morph_parameters_preserve_legacy_defaults_and_custom_values() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.morph_duration_ms, 100);
        assert_eq!(settings.morph_strength, 50);
        assert_eq!(settings.morph_curve, Curve::Cubic);
        for curve in Curve::ALL {
            settings.morph_duration_ms = 1000;
            settings.morph_strength = 35;
            settings.morph_curve = curve;
            let saved = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored.morph_duration_ms, 1000);
            assert_eq!(restored.morph_strength, 35);
            assert_eq!(restored.morph_curve, curve);
        }
    }

    #[test]
    fn layout_morphing_defaults_on_and_persists_independently() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.layout_morphing);
        settings.animations = true;
        settings.layout_morphing = false;
        let saved = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&saved).unwrap();
        assert!(restored.animations);
        assert!(!restored.layout_morphing);
    }
}
