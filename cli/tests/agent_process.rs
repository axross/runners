//! the agent as a process: its entry wires the real container backend, GitHub, the
//! token store, the lock, the channel and the log, here on the fake `docker` and a
//! loopback GitHub stub.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::time::Duration;

use support::agent::{AgentProcess, Conn};
use support::host::{Host, JIT, TOKEN, registered};
use support::stub::Stub;

fn stub() -> Stub {
    Stub::start((0..8).map(|_| registered()).collect())
}

fn put_config(host: &Host) {
    std::fs::copy(&host.config, host.state().join("config.json")).unwrap();
}

fn log_text(host: &Host) -> String {
    host.log_files()
        .iter()
        .map(|file| std::fs::read_to_string(file).unwrap_or_default())
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn the_agent_runs_a_pool_on_the_real_backend_and_quits_cleanly() {
    let host = Host::new();
    put_config(&host);
    host.script("run_mode", "hold");
    host.script("run_prints_jit", "1");
    let stub = stub();
    let token = format!("o/r={TOKEN}");
    let agent = AgentProcess::spawn(&host, &stub.address, &["--token", &token]);

    let conn = Conn::open(&agent.roots).await;
    let snapshot = conn.until_runner_runs().await;
    assert_eq!(snapshot.pools[0].state, "running");
    let request = &stub.requests()[0];
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );

    // a second agent is refused, and writes no log
    let logs = host.log_files();
    let second = AgentProcess::spawn(&host, &stub.address, &[]);
    let (code, stderr) = second.finish(Duration::from_secs(20)).await;
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("instance lock"), "{stderr}");
    assert_eq!(host.log_files(), logs);

    conn.ok("quit").await;
    let (code, stderr) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(host.containers().is_empty(), "{:?}", host.containers());

    let log = log_text(&host);
    assert!(log.contains("The agent is running"), "{log}");
    assert!(log.contains("The agent has ended"), "{log}");
    for secret in [TOKEN, JIT] {
        assert!(!log.contains(secret), "{secret} is in the log");
        assert!(!stderr.contains(secret));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_log_continues_in_a_new_file_and_ten_are_kept() {
    let host = Host::new();
    let stub = stub();
    // no configuration: every start is logged and refused, which makes lines cheaply
    let agent = AgentProcess::spawn(
        &host,
        &stub.address,
        &["--stopped", "--max-log-bytes", "400"],
    );
    let conn = Conn::open(&agent.roots).await;
    for _ in 0..60 {
        conn.ok("start_all").await;
    }
    let files = host.log_files();
    assert_eq!(files.len(), 10, "{files:?}");
    let names: Vec<_> = files
        .iter()
        .map(|f| f.file_name().unwrap().to_owned())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(names, sorted, "the names rise with time");
    let newest = std::fs::read_to_string(files.last().unwrap()).unwrap();
    assert!(newest.contains("Resuming"), "{newest}");
    for file in &files {
        let size = std::fs::metadata(file).unwrap().len();
        assert!(size < 400 + 400, "{file:?} is {size} bytes");
    }
    conn.ok("quit").await;
    let (code, _) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(code, Some(0));
}

#[tokio::test(flavor = "current_thread")]
async fn a_build_logs_its_output_masked_and_a_failure_carries_the_last_lines() {
    let host = Host::new();
    put_config(&host);
    host.script("build_fails", "");
    host.script(
        "build_stderr",
        &format!("step 1\nstep 2\nauthentication used {TOKEN}\nstep 4 failed\n"),
    );
    let stub = stub();
    let agent = AgentProcess::spawn(&host, &stub.address, &["--stopped"]);
    let conn = Conn::open(&agent.roots).await;
    conn.set_token("o/r", TOKEN).await.unwrap();
    conn.ok("build").await;
    let snapshot = conn.until_state("stopped").await;
    let reason = snapshot.agent.reason.unwrap();
    assert_eq!(reason.code, "image_build_failed");
    assert!(
        reason.message.contains("step 4 failed"),
        "{}",
        reason.message
    );
    assert!(reason.message.contains("<masked>"), "{}", reason.message);
    assert!(!reason.message.contains(TOKEN));
    assert!(!reason.message.contains("docker "), "{}", reason.message);
    conn.ok("quit").await;
    let (code, _) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(code, Some(0));
    let log = log_text(&host);
    assert!(log.contains("docker build: fake build output"), "{log}");
    assert!(log.contains("docker build: step 2"), "{log}");
    assert!(!log.contains(TOKEN), "the token reached the log");
}
