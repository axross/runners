//! A stand-in for a second `runners` process, built so the instance-layer tests and
//! CI can run the lock and the channel in real processes, and as another user.
//!
//! It takes its roots from `--root`, which only a debug build allows, so it can never
//! touch a real lock or channel. A release build of it does nothing.
//!
//! ```text
//! instance-helper hold --root DIR [--child]   take the lock, print `locked`, wait
//! instance-helper try --root DIR              try the lock and exit with the outcome
//! instance-helper idle                        wait for end of input
//! ```
//!
//! `hold --child` also starts an `idle` child, which inherits the helper's standard
//! streams and no lock, so a test can kill the helper and see the child outlive it.

#[cfg(not(debug_assertions))]
fn main() -> std::process::ExitCode {
    eprintln!("instance-helper is for debug builds only");
    std::process::ExitCode::from(2)
}

#[cfg(debug_assertions)]
use std::path::PathBuf;
#[cfg(debug_assertions)]
use std::process::ExitCode;

#[cfg(debug_assertions)]
use runners_cli::instance::lock::{InstanceLock, LockError};
#[cfg(debug_assertions)]
use runners_cli::instance::roots::Roots;

/// The exit codes, which the tests and CI read.
#[cfg(debug_assertions)]
mod exit {
    pub const ACQUIRED: u8 = 0;
    pub const USAGE: u8 = 2;
    pub const HELD: u8 = 3;
    pub const CANNOT_OPEN: u8 = 4;
    pub const LOST: u8 = 5;
}

#[cfg(debug_assertions)]
fn root_from(args: &[String]) -> Option<Roots> {
    let position = args.iter().position(|arg| arg == "--root")?;
    let root = PathBuf::from(args.get(position + 1)?);
    Some(Roots::for_tests(&root))
}

#[cfg(debug_assertions)]
fn lock_exit(error: &LockError) -> u8 {
    match error {
        LockError::Held => exit::HELD,
        LockError::Lost => exit::LOST,
        LockError::CannotOpen(_) => exit::CANNOT_OPEN,
    }
}

#[cfg(debug_assertions)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(mode) = args.first().map(String::as_str) else {
        return ExitCode::from(exit::USAGE);
    };
    match mode {
        "idle" => {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            ExitCode::SUCCESS
        }
        "try" | "hold" => {
            let Some(roots) = root_from(&args) else {
                eprintln!("--root DIR is required");
                return ExitCode::from(exit::USAGE);
            };
            let lock = match InstanceLock::acquire(&roots) {
                Ok(lock) => lock,
                Err(error) => {
                    println!("refused: {error}");
                    return ExitCode::from(lock_exit(&error));
                }
            };
            if mode == "try" {
                println!("acquired");
                drop(lock);
                return ExitCode::from(exit::ACQUIRED);
            }
            let _child = if args.iter().any(|arg| arg == "--child") {
                std::env::current_exe().ok().and_then(|program| {
                    std::process::Command::new(program).arg("idle").spawn().ok()
                })
            } else {
                None
            };
            println!("locked");
            loop {
                std::thread::park();
            }
        }
        _ => ExitCode::from(exit::USAGE),
    }
}
