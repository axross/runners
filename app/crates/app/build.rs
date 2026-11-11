use tauri_build::{AppManifest, Attributes};

/// the commands the web view may call; each gets a permission, so a window can call
/// it only when its capability grants it.
const COMMANDS: [&str; 2] = ["read_view", "hide_window"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_build::try_build(Attributes::new().app_manifest(AppManifest::new().commands(&COMMANDS)))?;
    Ok(())
}
