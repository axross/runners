use std::path::Path;
use std::{env, fs};

use embed_resource::NONE;
use tauri_build::{AppManifest, Attributes};

/// the commands the web view may call; each gets a permission, so a window can call
/// it only when its capability grants it.
const COMMANDS: [&str; 13] = [
    "read_view",
    "start_all",
    "stop_all",
    "build",
    "stop_build",
    "try_again",
    "set_token",
    "remove_token",
    "set_setting",
    "hide_window",
    "read_confirmation",
    "answer_confirmation",
    "open_job",
];

/// the dependency on the version 6 common controls that `common-controls-v6` needs and
/// that tauri-build's own manifest, linked into `marshal-app.exe` alone, already states.
const COMMON_CONTROLS_MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_build::try_build(Attributes::new().app_manifest(AppManifest::new().commands(&COMMANDS)))?;
    if targets_windows_msvc() {
        link_common_controls_into_tests()?;
    }
    Ok(())
}

/// the target, not the host, decides: a cross build for Windows needs it and a Windows host
/// building for another target does not.
fn targets_windows_msvc() -> bool {
    env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows")
        && env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|target_env| target_env == "msvc")
}

/// tauri-build links its manifest into the binaries only, so an integration test that links
/// the Tauri runtime imports `TaskDialogIndirect` from a comctl32 it cannot resolve and
/// fails to load. Cargo gives a `-tests` argument to integration tests and none to the
/// library's own unit tests.
fn link_common_controls_into_tests() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = env::var("OUT_DIR")?;
    let manifest = Path::new(&out_dir).join("test-manifest.xml");
    fs::write(&manifest, COMMON_CONTROLS_MANIFEST)?;
    // the resource script's path is a string literal, in which a backslash escapes.
    let manifest_path = manifest.display().to_string().replace('\\', "\\\\");
    let script = Path::new(&out_dir).join("test-manifest.rc");
    fs::write(&script, format!("1 24 \"{manifest_path}\"\n"))?;
    embed_resource::compile_for_tests(&script, NONE).manifest_required()?;
    Ok(())
}
