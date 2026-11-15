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

use crate::{platform::window::PlatformWaker, ui::theme::ThemeName};

const VISIBLE: Duration = Duration::from_secs(2);
const FADE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeName,
    pub rounded_corners: bool,
}

impl Settings {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("Could not read settings from {}", path.display())),
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
}

impl Store {
    pub fn start(path: PathBuf, waker: PlatformWaker) -> anyhow::Result<Self> {
        let (sender, receiver) = mpsc::channel::<(u64, Settings)>();
        let (results, updates) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("settings".into())
            .spawn(move || {
                let mut animation: Option<Instant> = None;

                loop {
                    let command = match animation {
                        Some(start) => {
                            let now = Instant::now();
                            let fade_start = start + VISIBLE - FADE;
                            let end = start + VISIBLE;
                            let next = if now < fade_start {
                                fade_start
                            } else {
                                (now + Duration::from_millis(33)).min(end)
                            };

                            match receiver.recv_timeout(next.saturating_duration_since(now)) {
                                Ok(command) => Some(command),
                                Err(mpsc::RecvTimeoutError::Timeout) => {
                                    let _ = waker.request_redraw();

                                    if Instant::now() >= end {
                                        animation = None;
                                    }

                                    continue;
                                }

                                Err(mpsc::RecvTimeoutError::Disconnected) => None,
                            }
                        }

                        None => receiver.recv().ok(),
                    };

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
                    animation = result.as_ref().ok().copied();
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
        })
    }

    pub fn save(&mut self, settings: Settings) {
        self.revision += 1;
        let _ = self
            .sender
            .as_ref()
            .unwrap()
            .send((self.revision, settings));
    }

    pub fn poll(&self) -> Option<Result<Instant, String>> {
        self.results
            .try_iter()
            .filter(|saved| saved.revision == self.revision)
            .last()
            .map(|saved| saved.result)
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

pub fn confirmation_opacity(saved: Instant) -> f32 {
    ((VISIBLE.as_secs_f32() - saved.elapsed().as_secs_f32()) / FADE.as_secs_f32()).clamp(0.0, 1.0)
}
