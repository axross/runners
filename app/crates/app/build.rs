use tauri_build::{AppManifest, Attributes};

/// the commands the web view may call; each gets a permission, so a window can call
/// it only when its capability grants it.
const COMMANDS: [&str; 11] = [
    "read_view",
    "start_all",
    "stop_all",
    "build",
    "stop_build",
    "try_again",
    "set_token",
    "remove_token",
    "hide_window",
    "read_confirmation",
    "answer_confirmation",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_build::try_build(Attributes::new().app_manifest(AppManifest::new().commands(&COMMANDS)))?;
    Ok(())
}
