//! a stand-in for a second `marshal` process, so the instance-layer tests and CI can
//! run the lock and the channel in real processes, and as another user; it takes its
//! roots from `--root`, which only a debug build allows, so it can never touch a real
//! lock or channel; a release build of it does nothing.
//!
//! ```text
//! instance-helper hold --root DIR [--child]   take the lock, print `locked`, wait
//! instance-helper try --root DIR              try the lock and exit with the outcome
//! instance-helper serve --root DIR            take the lock, open the channel, print
//!                                             its address, answer `ok` to every
//!                                             request until end of input
//! instance-helper mute --root DIR             as serve, but answer nothing
//! instance-helper serve-agent --root DIR      as serve, but open the agent's channel
//!                                             of protocol version 2
//! instance-helper agent --root DIR [--stopped] [--github-api URL] [--docker PROGRAM]
//!     [--docker-dir DIR] [--max-log-bytes N] [--token OWNER/NAME=TOKEN]...
//!                                             run the resident agent as a process, on a
//!                                             token store in memory that holds the
//!                                             tokens given, until it is told to quit
//! instance-helper shim ...                    the other commands of `marshal` run as
//!                                             it does, because the agent starts its
//!                                             job containers and exports through the
//!                                             program it runs as
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
use marshal_cli::instance::channel::ChannelServer;
#[cfg(debug_assertions)]
use marshal_cli::instance::lock::{InstanceLock, LockError};
#[cfg(debug_assertions)]
use marshal_cli::instance::protocol::Response;
#[cfg(debug_assertions)]
use marshal_cli::instance::roots::Roots;

/// the exit codes, which the tests and CI read.
#[cfg(debug_assertions)]
mod exit {
    pub const ACQUIRED: u8 = 0;
    pub const USAGE: u8 = 2;
    pub const HELD: u8 = 3;
    pub const CANNOT_OPEN: u8 = 4;
    pub const LOST: u8 = 5;
    pub const NO_CHANNEL: u8 = 6;
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

/// signals, from a thread of its own, when standard input ends.
#[cfg(debug_assertions)]
fn input_closed() -> tokio::sync::oneshot::Receiver<()> {
    let (closed, receiver) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        let _ = closed.send(());
    });
    receiver
}

/// serves the channel until end of input; a `mute` server keeps each request
/// unanswered, as an instance that has stopped answering does.
#[cfg(debug_assertions)]
fn serve(lock: &InstanceLock, roots: &Roots, mute: bool) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("the runtime cannot start: {error}");
            return ExitCode::from(exit::NO_CHANNEL);
        }
    };
    runtime.block_on(async {
        let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let server = match ChannelServer::open(lock, roots, sender) {
            Ok(server) => server,
            Err(error) => {
                println!("refused: {error}");
                return ExitCode::from(exit::NO_CHANNEL);
            }
        };
        println!("{}", server.address());
        let mut closed = input_closed();
        let mut unanswered = Vec::new();
        loop {
            tokio::select! {
                _ = &mut closed => break,
                incoming = requests.recv() => match incoming {
                    Some(incoming) if mute => unanswered.push(incoming),
                    Some(incoming) => {
                        let _ = incoming.reply.send(Response::ok("helper"));
                    }
                    None => break,
                },
            }
        }
        drop(unanswered);
        drop(server);
        ExitCode::SUCCESS
    })
}

/// opens the agent's channel, which serves protocol version 2 and answers every
/// request that reaches the agent with a refusal, until end of input.
#[cfg(debug_assertions)]
fn serve_agent(lock: &InstanceLock, roots: &Roots) -> ExitCode {
    use marshal_cli::agent::AgentChannel;
    use marshal_cli::agent::server::Limits;
    use marshal_cli::agent::snapshot::Snapshot;
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("the runtime cannot start: {error}");
            return ExitCode::from(exit::NO_CHANNEL);
        }
    };
    runtime.block_on(async {
        let (calls, requests) = tokio::sync::mpsc::unbounded_channel();
        let (_snapshots, receiver) =
            tokio::sync::watch::channel(std::sync::Arc::new(Snapshot::default()));
        let channel = match AgentChannel::open(lock, roots, calls, receiver, Limits::default()) {
            Ok(channel) => channel,
            Err(error) => {
                println!("refused: {error}");
                return ExitCode::from(exit::NO_CHANNEL);
            }
        };
        println!("{}", channel.address());
        let _ = input_closed().await;
        drop(requests);
        drop(channel);
        ExitCode::SUCCESS
    })
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
        "try" | "hold" | "serve" | "mute" | "serve-agent" => {
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
            if mode == "serve" || mode == "mute" {
                return serve(&lock, &roots, mode == "mute");
            }
            if mode == "serve-agent" {
                return serve_agent(&lock, &roots);
            }
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
        "agent" => run_agent(&args),
        _ => marshal_cli::command::run(
            std::env::args_os(),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        ),
    }
}

/// the value after `flag`, when it is given.
#[cfg(debug_assertions)]
fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let position = args.iter().position(|arg| arg == flag)?;
    args.get(position + 1).map(String::as_str)
}

/// runs the resident agent as this process, with the debug-build overrides the tests
/// need, and exits with the code the agent ends with.
#[cfg(debug_assertions)]
fn run_agent(args: &[String]) -> ExitCode {
    use std::ffi::OsString;
    use std::sync::Arc;

    use marshal_cli::agent::machine::Launch;
    use marshal_cli::agent::{AgentOptions, run};
    use marshal_cli::runtime::secret::Secret;
    use marshal_cli::token_store::{MemoryTokenStore, TokenKey, TokenStore};

    let Some(root) = value_of(args, "--root") else {
        eprintln!("--root DIR is required");
        return ExitCode::from(exit::USAGE);
    };
    let store = Arc::new(MemoryTokenStore::default());
    for (position, _) in args.iter().enumerate().filter(|(_, arg)| *arg == "--token") {
        let Some((repository, token)) =
            args.get(position + 1).and_then(|pair| pair.split_once('='))
        else {
            eprintln!("--token wants OWNER/NAME=TOKEN");
            return ExitCode::from(exit::USAGE);
        };
        let Ok(key) = TokenKey::parse(repository) else {
            eprintln!("--token wants OWNER/NAME=TOKEN");
            return ExitCode::from(exit::USAGE);
        };
        let _ = store.set(&key, &Secret::new(token.to_string()));
    }
    let root = PathBuf::from(root);
    let docker = value_of(args, "--docker").map(OsString::from);
    let mut options = AgentOptions::new(if args.iter().any(|arg| arg == "--stopped") {
        Launch::Stopped
    } else {
        Launch::Running
    });
    options.test_root = Some(&root);
    options.github_api = value_of(args, "--github-api");
    options.docker = docker.as_deref();
    if let Some(directory) = value_of(args, "--docker-dir") {
        options
            .docker_environment
            .push((OsString::from("FAKE_DOCKER_DIR"), OsString::from(directory)));
    }
    options.store = Some(store);
    // the tests' configurations are axpc's on every host
    options.profile = Some(marshal_cli::profile::Profile::Axpc);
    options.max_log_bytes = value_of(args, "--max-log-bytes").and_then(|bytes| bytes.parse().ok());
    ExitCode::from(run(options, &mut std::io::stderr()))
}
