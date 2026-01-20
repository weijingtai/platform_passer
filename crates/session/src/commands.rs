use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum SessionCommand {
    SendFile(PathBuf),
    Disconnect,
}
