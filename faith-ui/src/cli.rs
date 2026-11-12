use clap::{Parser, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "faith")]
pub struct Cli {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 4433)]
    pub port: u16,

    #[arg(long)]
    pub server_name: Option<String>,

    #[arg(long)]
    pub ca_file: Option<std::path::PathBuf>,

    #[arg(long, default_value_os_t = default_profile())]
    pub profile: std::path::PathBuf,

    #[arg(long, value_enum, default_value_t = PlatformBackend::Auto)]
    pub platform: PlatformBackend,

    #[arg(long, value_enum, default_value_t = GraphicsBackendArg::Auto)]
    pub graphics: GraphicsBackendArg,

    #[arg(long, default_value_t = 1280)]
    pub width: u32,

    #[arg(long, default_value_t = 720)]
    pub height: u32,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PlatformBackend {
    Auto,
    Wayland,
    X11,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GraphicsBackendArg {
    Auto,
    Vulkan,

    #[value(name = "opengl")]
    OpenGl,
}

impl From<GraphicsBackendArg> for crate::graphics::GraphicsBackend {
    fn from(value: GraphicsBackendArg) -> Self {
        match value {
            GraphicsBackendArg::Auto => crate::graphics::GraphicsBackend::Auto,
            GraphicsBackendArg::Vulkan => crate::graphics::GraphicsBackend::Vulkan,
            GraphicsBackendArg::OpenGl => crate::graphics::GraphicsBackend::OpenGL,
        }
    }
}

fn default_profile() -> std::path::PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
                .join(".local/share")
        })
        .join("faith/default")
}
