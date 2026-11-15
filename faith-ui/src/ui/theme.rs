use crate::graphics::Color;
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum ThemeName {
    #[default]
    #[value(alias = "blossom_light")]
    Light,
    #[value(alias = "gruvbox")]
    Dark,
}

impl ThemeName {
    pub const fn colors(self) -> Theme {
        match self {
            Self::Light => Theme::LIGHT,
            Self::Dark => Theme::DARK,
        }
    }
}

#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub radii: CornerRadii,
    pub background: Color,
    pub surface: Color,
    pub selected_icon: Color,
    pub separator: Color,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    pub input: Color,
    pub button: Color,
    pub button_text: Color,
    pub primary: Color,
    pub primary_text: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
}

impl Theme {
    pub const LIGHT: Self = Self {
        radii: CornerRadii::ROUNDED,
        background: hex(0xe6dfdc),
        surface: hex(0xdfd8d5),
        selected_icon: hex(0x5e524c),
        separator: hex(0xd3ccc9),
        text: hex(0x695d57),
        muted: hex(0x746862),
        border: hex(0xc0b9b6),
        input: hex(0xe6dfdc),
        button: hex(0xd9d2cf),
        button_text: hex(0x695d57),
        primary: hex(0x695d57),
        primary_text: hex(0xe6dfdc),
        success: hex(0x6c805c),
        warning: hex(0xb3816a),
        error: hex(0x976153),
    };

    pub const DARK: Self = Self {
        radii: CornerRadii::ROUNDED,
        background: hex(0x282828),
        surface: hex(0x232323),
        selected_icon: hex(0xbdae93),
        separator: hex(0x3c3836),
        text: hex(0xebdbb2),
        muted: hex(0xbdae93),
        border: hex(0x656565),
        input: hex(0x282828),
        button: hex(0x3c3836),
        button_text: hex(0xebdbb2),
        primary: hex(0x83a598),
        primary_text: hex(0x282828),
        success: hex(0xb8bb26),
        warning: hex(0xfabd2f),
        error: hex(0xfb4934),
    };
}

const fn hex(rgb: u32) -> Color {
    Color::rgba(
        ((rgb >> 16) & 255) as f32 / 255.0,
        ((rgb >> 8) & 255) as f32 / 255.0,
        (rgb & 255) as f32 / 255.0,
        1.0,
    )
}
