// Define the events that the session emits to the UI/CLI
#[derive(Debug, Clone)]
pub enum SessionEvent {
    Log(String),
    Connected(String), // Remote Address
    Disconnected,
    Error(String),
}
