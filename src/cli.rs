use clap::{Parser, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "faith")]
pub struct Cli {
    #[arg(long, value_enum, default_value_t = Backend::Auto)]
    pub backend: Backend,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Backend {
    Auto,
    Wayland,
    X11,
}
