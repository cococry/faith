use crate::graphics::Color;
use anyhow::Context;
use serde::{Deserialize, Deserializer};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CornerRadii {
    pub control: f32,
    pub row: f32,
    pub icon: f32,
    pub badge: f32,
    pub checkbox: f32,
    pub popup: f32,
}

impl CornerRadii {
    pub const SQUARE: Self = Self {
        control: 0.0,
        row: 0.0,
        icon: 0.0,
        badge: 0.0,
        checkbox: 0.0,
        popup: 0.0,
    };

    pub const ROUNDED: Self = Self {
        control: 10.0,
        row: 12.0,
        icon: 10.0,
        badge: 10.0,
        checkbox: 6.0,
        popup: 12.0,
    };
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    #[serde(default)]
    pub radii: CornerRadii,
    #[serde(deserialize_with = "deserialize_color")]
    pub background: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub surface: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub selected_icon: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub separator: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub text: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub muted: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub border: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub input: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub button: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub button_text: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub primary: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub primary_text: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub success: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub warning: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub error: Color,
}

impl Theme {
    pub fn interpolate(self, target: Self, t: f32) -> Self {
        Self {
            radii: target.radii,
            background: super::animation::color(self.background, target.background, t),
            surface: super::animation::color(self.surface, target.surface, t),
            selected_icon: super::animation::color(self.selected_icon, target.selected_icon, t),
            separator: super::animation::color(self.separator, target.separator, t),
            text: super::animation::color(self.text, target.text, t),
            muted: super::animation::color(self.muted, target.muted, t),
            border: super::animation::color(self.border, target.border, t),
            input: super::animation::color(self.input, target.input, t),
            button: super::animation::color(self.button, target.button, t),
            button_text: super::animation::color(self.button_text, target.button_text, t),
            primary: super::animation::color(self.primary, target.primary, t),
            primary_text: super::animation::color(self.primary_text, target.primary_text, t),
            success: super::animation::color(self.success, target.success, t),
            warning: super::animation::color(self.warning, target.warning, t),
            error: super::animation::color(self.error, target.error, t),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    #[serde(default, rename = "name")]
    _name: Option<String>,
    #[serde(flatten)]
    colors: Theme,
}

#[derive(Default, Deserialize)]
struct Metadata {
    name: Option<String>,
}

const BLOODVOID: &str = include_str!("../../assets/themes/bloodvoid.json");
const BLOSSOM: &str = include_str!("../../assets/themes/blossom-light.json");
const BUNDLED: &[(&str, &str)] = &[
    ("bloodvoid.json", BLOODVOID),
    ("blossom-light.json", BLOSSOM),
    (
        "catppuccin.json",
        include_str!("../../assets/themes/catppuccin.json"),
    ),
    (
        "catppuccin-latte.json",
        include_str!("../../assets/themes/catppuccin-latte.json"),
    ),
    (
        "tokyonight.json",
        include_str!("../../assets/themes/tokyonight.json"),
    ),
    (
        "kanagawa.json",
        include_str!("../../assets/themes/kanagawa.json"),
    ),
    (
        "kanagawa-dragon.json",
        include_str!("../../assets/themes/kanagawa-dragon.json"),
    ),
    (
        "gruvbox.json",
        include_str!("../../assets/themes/gruvbox.json"),
    ),
    (
        "gruvbox_light.json",
        include_str!("../../assets/themes/gruvbox_light.json"),
    ),
    (
        "onedark.json",
        include_str!("../../assets/themes/onedark.json"),
    ),
    (
        "one_light.json",
        include_str!("../../assets/themes/one_light.json"),
    ),
    ("nord.json", include_str!("../../assets/themes/nord.json")),
    (
        "rosepine.json",
        include_str!("../../assets/themes/rosepine.json"),
    ),
    (
        "rosepine-dawn.json",
        include_str!("../../assets/themes/rosepine-dawn.json"),
    ),
    (
        "ayu_dark.json",
        include_str!("../../assets/themes/ayu_dark.json"),
    ),
    (
        "ayu_light.json",
        include_str!("../../assets/themes/ayu_light.json"),
    ),
    (
        "everforest.json",
        include_str!("../../assets/themes/everforest.json"),
    ),
    (
        "everforest_light.json",
        include_str!("../../assets/themes/everforest_light.json"),
    ),
    (
        "nightfox.json",
        include_str!("../../assets/themes/nightfox.json"),
    ),
    (
        "carbonfox.json",
        include_str!("../../assets/themes/carbonfox.json"),
    ),
    (
        "github_dark.json",
        include_str!("../../assets/themes/github_dark.json"),
    ),
    (
        "github_light.json",
        include_str!("../../assets/themes/github_light.json"),
    ),
    (
        "solarized_dark.json",
        include_str!("../../assets/themes/solarized_dark.json"),
    ),
    (
        "solarized_light.json",
        include_str!("../../assets/themes/solarized_light.json"),
    ),
    (
        "chadracula.json",
        include_str!("../../assets/themes/chadracula.json"),
    ),
    (
        "nightowl.json",
        include_str!("../../assets/themes/nightowl.json"),
    ),
    (
        "monekai.json",
        include_str!("../../assets/themes/monekai.json"),
    ),
];
const NOTICES: &str = include_str!("../../assets/licenses/themes/NOTICE.txt");

pub const DEFAULT_ID: &str = "bloodvoid";

impl Default for CornerRadii {
    fn default() -> Self {
        Self::ROUNDED
    }
}

impl Default for Theme {
    fn default() -> Self {
        static DEFAULT: OnceLock<Theme> = OnceLock::new();
        *DEFAULT.get_or_init(|| {
            serde_json::from_str::<ThemeFile>(BLOODVOID)
                .expect("invalid bundled theme")
                .colors
        })
    }
}

fn deserialize_color<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Color, D::Error> {
    let value = String::deserialize(deserializer)?;
    let hex = value.strip_prefix('#').unwrap_or("");

    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(serde::de::Error::custom("expected #RRGGBB or #RRGGBBAA"));
    }

    let rgb = u32::from_str_radix(hex, 16).map_err(serde::de::Error::custom)?;
    let (rgb, alpha) = if hex.len() == 8 {
        (rgb >> 8, (rgb & 255) as f32 / 255.0)
    } else {
        (rgb, 1.0)
    };
    Ok(Color::rgba(
        ((rgb >> 16) & 255) as f32 / 255.0,
        ((rgb >> 8) & 255) as f32 / 255.0,
        (rgb & 255) as f32 / 255.0,
        alpha,
    ))
}

pub fn canonical_id(id: &str) -> &str {
    match id {
        "light" | "blossom_light" => "blossom-light",
        "dark" => "gruvbox",
        _ => id,
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

pub struct Catalog {
    directory: PathBuf,
    pub ids: Vec<String>,
    pub names: Arc<[String]>,
}

impl Catalog {
    pub fn open(directory: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(&directory).context("Could not create themes directory")?;

        /* we only install missing files, so edits to the bundled themes stay yours */
        for (filename, source) in BUNDLED
            .iter()
            .copied()
            .chain(std::iter::once(("LICENSES.txt", NOTICES)))
        {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(filename))
            {
                Ok(mut file) => file.write_all(source.as_bytes())?,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error).context("Could not install bundled theme"),
            }
        }

        let mut catalog = Self {
            directory,
            ids: Vec::new(),
            names: Arc::from([]),
        };
        catalog.refresh()?;
        Ok(catalog)
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let mut ids = Vec::new();

        /* we read names for the menu, but leave color loading until selection */
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let path = entry.path();

            if !entry.file_type()?.is_file()
                || path.extension().and_then(|s| s.to_str()) != Some("json")
            {
                continue;
            }

            if let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|id| valid_id(id))
            {
                ids.push(id.to_owned());
            }
        }

        ids.sort_unstable();
        let names: Arc<[String]> = ids
            .iter()
            .map(|id| {
                let metadata = fs::read(self.directory.join(format!("{id}.json")))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Metadata>(&bytes).ok())
                    .unwrap_or_default();
                metadata
                    .name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| id.clone())
            })
            .collect();
        self.ids = ids;
        self.names = names;
        Ok(())
    }

    pub fn load(&self, id: &str) -> anyhow::Result<Theme> {
        let id = canonical_id(id);
        anyhow::ensure!(valid_id(id), "Invalid theme filename: {id}");
        let path = self.directory.join(format!("{id}.json"));
        load(&path).with_context(|| format!("Could not load theme {}", path.display()))
    }
}

fn load(path: &Path) -> anyhow::Result<Theme> {
    let document: ThemeFile = serde_json::from_slice(&fs::read(path)?)?;
    let theme = document.colors;
    let r = theme.radii;
    anyhow::ensure!(
        [r.control, r.row, r.icon, r.badge, r.checkbox, r.popup]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0 && *value <= 256.0),
        "Corner radii must be between 0 and 256"
    );
    Ok(theme)
}
