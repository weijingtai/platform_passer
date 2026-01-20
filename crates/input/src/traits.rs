use anyhow::Result;
use platform_passer_core::InputEvent;

pub trait InputSource {
    /// Start capturing input events. The callback is invoked for each event.
    fn start_capture(&self, callback: Box<dyn Fn(InputEvent) + Send + Sync>) -> Result<()>;
    fn stop_capture(&self) -> Result<()>;
}

pub trait InputSink {
    /// Inject a remote input event into the local system.
    fn inject_event(&self, event: InputEvent) -> Result<()>;
}
