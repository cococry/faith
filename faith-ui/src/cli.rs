use clap::{Parser, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "faith")]
pub struct Cli {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 4434)]
    pub port: u16,

    #[arg(long)]
    pub server_name: Option<String>,

    #[arg(long)]
    pub ca_file: Option<std::path::PathBuf>,

    #[arg(long)]
    pub profile: Option<std::path::PathBuf>,

    #[arg(long, value_enum, default_value_t = PlatformBackend::Auto)]
    pub platform: PlatformBackend,

    #[arg(long, value_enum, default_value_t = GraphicsBackendArg::Auto)]
    pub graphics: GraphicsBackendArg,

    #[arg(long, default_value_t = 1280)]
    pub width: u32,

    #[arg(long, default_value_t = 720)]
    pub height: u32,
}

impl Cli {
    pub fn resolve_local_tls(&mut self) {
        if !matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "::1") {
            return;
        }

        if self.ca_file.is_none()
            && self
                .server_name
                .as_deref()
                .is_none_or(|name| name == "localhost")
        {
            let certificate =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/system/cert.pem");
            if certificate.is_file() {
                self.ca_file = Some(certificate);
            }
        }

        if self.ca_file.is_some() && self.server_name.is_none() {
            self.server_name = Some("localhost".into());
        }
    }
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
