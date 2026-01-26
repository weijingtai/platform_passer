use anyhow::Result;

pub trait ClipboardProvider {
    fn get_text(&self) -> Result<String>;
    fn set_text(&self, text: String) -> Result<()>;
    fn get_image(&self) -> Result<Option<Vec<u8>>>; // Returns PNG bytes
    fn set_image(&self, png_data: Vec<u8>) -> Result<()>;
    fn get_files(&self) -> Result<Option<Vec<String>>>; // Returns list of file paths
    fn set_files(&self, files: Vec<String>) -> Result<()>;
    
    // Callback is invoked when local clipboard changes
    fn start_listener(&self, callback: Box<dyn Fn() + Send + Sync>) -> Result<()>;
}
