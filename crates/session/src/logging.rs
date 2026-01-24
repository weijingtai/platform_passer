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

// -- Improved Tracing Integration --

use std::sync::{Arc, Mutex};
use tracing_subscriber::Layer;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;

pub struct GuiLogLayer {
    // We use a shared sender that can be updated when a session starts.
    // This is a bit of a hack to bridge the static tracing registry with our dynamic session.
    pub tx: Arc<Mutex<Option<Sender<SessionEvent>>>>,
}

impl<S> Layer<S> for GuiLogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut message = String::new();
        let mut visitor = MessageVisitor(&mut message);
        event.record(&mut visitor);

        let level = match *event.metadata().level() {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::TRACE => LogLevel::Trace,
        };

        // We can't await here (sync context), so we use blocking_send if possible or spawn.
        // Since we are in a library, spawning is tricky. But `mpsc::Sender` has `try_send` or `blocking_send`.
        // However, `tokio::sync::mpsc::Sender` blocking_send requires a runtime if not available? No, it blocks thread.
        // Tracing often happens in async context, blocking is bad.
        // Best effort: try_send.
        
        if let Ok(guard) = self.tx.lock() {
            if let Some(tx) = &*guard {
                let _ = tx.try_send(SessionEvent::Log { level, message });
            }
        }
    }
}

struct MessageVisitor<'a>(&'a mut String);

impl<'a> tracing::field::Visit for MessageVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            let _ = write!(self.0, "{:?}", value);
        }
    }
    
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
         if field.name() == "message" {
             self.0.push_str(value);
         }
    }
}
