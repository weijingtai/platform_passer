fn main() {
    // Add Windows Kit bin path to PATH for this process so winres can find rc.exe
    if let Ok(path) = std::env::var("PATH") {
        let kit_path = r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64";
        let new_path = format!("{};{}", kit_path, path);
        std::env::set_var("PATH", new_path);
    }

    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_manifest_file("app.manifest");
        res.compile().unwrap();
    }
}
