#[derive(Debug, Clone, Copy, serde::Serialize)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum SessionEvent {
    Log { level: LogLevel, message: String },
    Connected(String), // Remote Address
    Disconnected,
    Error(String),
}
