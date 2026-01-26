fn main() {
  let mut attrs = tauri_build::Attributes::new();
  
  if cfg!(target_os = "windows") {
      // Add Windows Kit bin path to PATH for this process so tauri-build can find rc.exe
      if let Ok(path) = std::env::var("PATH") {
          let kit_path = r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64";
          let new_path = format!("{};{}", kit_path, path);
          std::env::set_var("PATH", new_path);
      }

      let manifest = include_str!("app.manifest");
      let win_attrs = tauri_build::WindowsAttributes::new()
          .app_manifest(manifest);
      attrs = attrs.windows_attributes(win_attrs);
  }

  tauri_build::try_build(attrs).expect("failed to run build script");
}
