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
    Waiting(String), // Bind Address
    Connecting(String), // Target Address
    Reconnecting(String), // Target Address
    Connected(String), // Remote Address
    Disconnected,
    Error(String),
}
