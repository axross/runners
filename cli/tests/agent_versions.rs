//! the version rules: the agent speaks protocol version 2 only, and today's commands,
//! which speak version 1, change nothing of it and say so.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::process::Command;

use marshal_cli::agent::machine::Launch;
use support::agent::{ONE_POOL, Rig, TOKEN};

/// runs `marshal <args>` against the agent's channel, off the runtime the agent runs on.
async fn marshal(root: std::path::PathBuf, args: &[&str]) -> std::process::Output {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    tokio::task::spawn_blocking(move || {
        Command::new(support::BINARY)
            .args(&args)
            .env("MARSHAL_TEST_ROOT", &root)
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn the_version_1_commands_exit_1_against_the_agent_and_change_nothing() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let root = rig.root.path().to_path_buf();
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let before = conn.until_runner_runs().await;
            for command in [
                vec!["stop"],
                vec!["status"],
                vec!["restart"],
                vec!["diagnostics"],
            ] {
                let output = marshal(root.clone(), &command).await;
                assert_eq!(
                    output.status.code(),
                    Some(1),
                    "{command:?}: {}{}",
                    support::stdout(&output),
                    String::from_utf8_lossy(&output.stderr)
                );
                let text = String::from_utf8_lossy(&output.stderr).to_string();
                assert!(!text.is_empty(), "{command:?} said nothing");
                assert!(!text.contains(TOKEN));
            }
            let after = conn.status().await;
            assert_eq!(after.agent.state, "running");
            assert_eq!(after.runners_up, before.runners_up);
            assert_eq!(after.agent.since, before.agent.since);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}
