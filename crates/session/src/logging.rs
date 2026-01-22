use crate::events::{SessionEvent, LogLevel};
use tokio::sync::mpsc::Sender;
use tracing;

pub async fn emit_log(tx: &Sender<SessionEvent>, level: LogLevel, message: String) {
    // Print to tracing (stdout/file)
    match level {
        LogLevel::Trace => tracing::trace!("{}", message),
        LogLevel::Debug => tracing::debug!("{}", message),
        LogLevel::Info => tracing::info!("{}", message),
        LogLevel::Warn => tracing::warn!("{}", message),
        LogLevel::Error => tracing::error!("{}", message),
    }

    // Send to UI
    let _ = tx.send(SessionEvent::Log { level, message }).await;
}

#[macro_export]
macro_rules! log_info {
    ($tx:expr, $($arg:tt)*) => {
        $crate::logging::emit_log($tx, $crate::events::LogLevel::Info, format!($($arg)*)).await
    };
}

#[macro_export]
macro_rules! log_error {
    ($tx:expr, $($arg:tt)*) => {
        $crate::logging::emit_log($tx, $crate::events::LogLevel::Error, format!($($arg)*)).await
    };
}

#[macro_export]
macro_rules! log_debug {
    ($tx:expr, $($arg:tt)*) => {
        $crate::logging::emit_log($tx, $crate::events::LogLevel::Debug, format!($($arg)*)).await
    };
}

#[macro_export]
macro_rules! log_warn {
    ($tx:expr, $($arg:tt)*) => {
        $crate::logging::emit_log($tx, $crate::events::LogLevel::Warn, format!($($arg)*)).await
    };
}
