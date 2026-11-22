use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, mpsc},
};

use fontdb::{Database, Family as FamilyName, Query, Source, Style, Weight};
use serde::{Deserialize, Serialize};

use crate::platform::window::PlatformWaker;

pub const BUNDLED: &str = "Noto Sans (bundled)";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Face {
    pub path: PathBuf,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Family {
    pub name: String,
    pub regular: Face,
    pub bold: Face,
    pub italic: Face,
}

fn discover() -> Vec<Family> {
    let mut database = Database::new();
    database.load_system_fonts();
    let names: BTreeSet<_> = database
        .faces()
        .filter_map(|face| face.families.first().map(|(name, _)| name.as_str()))
        .filter(|name| !name.is_empty())
        .collect();

    let mut families: Vec<_> = names
        .into_iter()
        .filter_map(|name| {
            let query = |weight, style| {
                let id = database.query(&Query {
                    families: &[FamilyName::Name(name)],
                    weight,
                    style,
                    ..Query::default()
                })?;
                let (source, index) = database.face_source(id)?;
                let path = match source {
                    Source::File(path) | Source::SharedFile(path, _) => path,
                    Source::Binary(_) => return None,
                };
                Some(Face { path, index })
            };

            let regular = query(Weight::NORMAL, Style::Normal)?;
            Some(Family {
                name: name.to_owned(),
                bold: query(Weight::BOLD, Style::Normal).unwrap_or_else(|| regular.clone()),
                italic: query(Weight::NORMAL, Style::Italic).unwrap_or_else(|| regular.clone()),
                regular,
            })
        })
        .collect();
    families.sort_by_cached_key(|family| family.name.to_lowercase());
    families
}

pub struct Catalog {
    pub families: Vec<Family>,
    pub labels: Arc<[String]>,
    pub selected: usize,
    worker: Option<mpsc::Receiver<Vec<Family>>>,
    loaded: bool,
}

impl Catalog {
    pub fn new(selected: Option<&Family>) -> Self {
        Self {
            families: selected.cloned().into_iter().collect(),
            labels: std::iter::once(BUNDLED.to_owned())
                .chain(selected.map(|font| font.name.clone()))
                .collect(),
            selected: usize::from(selected.is_some()),
            worker: None,
            loaded: false,
        }
    }

    pub fn start(&mut self, waker: PlatformWaker) -> anyhow::Result<()> {
        if self.loaded || self.worker.is_some() {
            return Ok(());
        }

        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("system-fonts".into())
            .spawn(move || {
                let _ = sender.send(discover());
                let _ = waker.request_redraw();
            })?;
        self.worker = Some(receiver);
        Ok(())
    }

    pub fn select(&mut self, selected: Option<&Family>) {
        self.selected = selected
            .and_then(|font| {
                self.families
                    .iter()
                    .position(|family| family.name == font.name)
            })
            .map_or(0, |index| index + 1);
    }

    pub fn needs_discovery(&self) -> bool {
        !self.loaded && !self.loading()
    }

    pub fn loading(&self) -> bool {
        self.worker.is_some()
    }

    pub fn poll(&mut self, selected: Option<&Family>) -> bool {
        let Some(worker) = &self.worker else {
            return false;
        };

        match worker.try_recv() {
            Ok(mut families) => {
                if let Some(font) = selected
                    && !families.iter().any(|family| family.name == font.name)
                {
                    families.insert(0, font.clone());
                }

                self.labels = std::iter::once(BUNDLED.to_owned())
                    .chain(families.iter().map(|family| family.name.clone()))
                    .collect();
                self.families = families;
                self.select(selected);
            }
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {}
        }

        self.worker = None;
        self.loaded = true;
        true
    }
}
