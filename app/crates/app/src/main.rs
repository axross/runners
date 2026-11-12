// a release build has no console, so the hidden modes and the app show none.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use marshal_modes::Mode;

fn main() -> ExitCode {
    match Mode::of(std::env::args_os()) {
        Mode::App => marshal_app::run(),
        hidden => marshal_modes::run(hidden),
    }
}
