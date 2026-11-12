//! the agent's other debug-only overrides, the limits, the file it watches, the token
//! store, the log size and the `docker` client's environment, move only in a debug
//! build; a release build ignores each request, so a release agent cannot be pointed
//! at a store, a limit or a program someone else prepared. The GitHub address, the
//! `docker` client, the definition and the roots have their own tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use marshal_cli::agent::machine::Launch;
use marshal_cli::agent::run::applied_overrides;
use marshal_cli::agent::server::Limits;
use marshal_cli::agent::tokens::{NO_TOKEN_STORE, StoreTokens};
use marshal_cli::agent::{AgentOptions, log::AgentLog};
use marshal_cli::token_store::{MemoryTokenStore, TokenKey};

#[test]
fn the_profile_is_the_one_ci_asked_for() {
    let debug_profile = cfg!(debug_assertions);
    match std::env::var("MARSHAL_EXPECT_RELEASE").as_deref() {
        Ok("1") => assert!(!debug_profile, "this test was built in the debug profile"),
        Ok("0") => assert!(debug_profile, "this test was built in the release profile"),
        _ => {}
    }
}

fn asked_for_everything(watched: &Path) -> AgentOptions<'_> {
    let mut options = AgentOptions::new(Launch::Stopped);
    options.docker_environment = vec![(OsString::from("FAKE"), OsString::from("1"))];
    options.executable = Some(watched);
    options.store = Some(Arc::new(MemoryTokenStore::default()));
    options.limits = Some(Limits {
        connections: 1,
        idle: Duration::from_millis(1),
        ..Limits::default()
    });
    options.max_log_bytes = Some(1);
    options
}

#[test]
fn every_override_moves_a_setting_in_a_debug_build_only() {
    let watched = Path::new("/tmp/not-the-agent");
    let applied = applied_overrides(&asked_for_everything(watched));
    if cfg!(debug_assertions) {
        assert_eq!(applied.max_log_bytes, 1);
        assert_eq!(applied.environment.len(), 1);
        assert!(applied.store.is_some());
        assert_eq!(applied.limits.connections, 1);
        assert_eq!(applied.executable.as_deref(), Some(watched));
    } else {
        assert_eq!(applied.max_log_bytes, AgentLog::default_max_bytes());
        assert!(applied.environment.is_empty());
        assert!(applied.store.is_none());
        assert_eq!(applied.limits, Limits::default());
        assert_eq!(applied.executable, std::env::current_exe().ok());
    }
}

#[test]
fn a_store_override_leaves_a_release_agent_with_no_store_of_the_test() {
    let watched = Path::new("/tmp/not-the-agent");
    let applied = applied_overrides(&asked_for_everything(watched));
    let tokens = StoreTokens::new(applied.store);
    let key = TokenKey::of("o", "r");
    let error = tokens.read(&key).unwrap_err();
    if cfg!(debug_assertions) {
        assert_ne!(error, NO_TOKEN_STORE, "the override store was not used");
    } else {
        assert_eq!(error, NO_TOKEN_STORE);
    }
}
