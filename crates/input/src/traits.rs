use anyhow::Result;
use platform_passer_core::InputEvent;

use platform_passer_core::config::AppConfig;

pub trait InputSource: Send + Sync {
    /// Start capturing input events. The callback is invoked for each event.
    fn start_capture(&self, callback: Box<dyn Fn(InputEvent) + Send + Sync>) -> Result<()>;
    fn stop_capture(&self) -> Result<()>;
    fn set_remote(&self, remote: bool) -> Result<()>;
    fn update_config(&self, _config: AppConfig) -> Result<()> { Ok(()) }
}

pub trait InputSink {
    /// Inject a remote input event into the local system.
    fn inject_event(&self, event: InputEvent) -> Result<()>;
    fn update_config(&self, _config: AppConfig) -> Result<()> { Ok(()) }
    fn reset_input(&self) -> Result<()> { Ok(()) }
}
