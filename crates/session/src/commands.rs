use platform_passer_core::config::AppConfig;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum SessionCommand {
    SendFile(PathBuf),
    UpdateConfig(AppConfig),
    Disconnect,
}
