//! a token entered in the window reaches the agent in `token_set`, and the app keeps no
//! copy: the agent here is the library's own, on a scratch host with a token store in
//! memory.

use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use marshal_cli::agent::{AgentOptions, Launch};
use marshal_cli::token_store::{MemoryTokenStore, RepositoryKey, TokenStore};

use crate::support::{FAKE_DOCKER, NoLauncher, ONE_POOL, Scratch, run_session};

/// plainly not a credential.
const TOKEN: &str = "not-a-real-token-0123456789";

#[tokio::test(flavor = "current_thread")]
async fn a_token_entered_in_the_window_reaches_the_agents_store_and_nothing_else_holds_it() {
    let scratch = Scratch::new();
    scratch.write_config(ONE_POOL);
    let store = Arc::new(MemoryTokenStore::default());
    let agent = {
        let store = Arc::clone(&store);
        let root = scratch.path().to_path_buf();
        let engine = scratch.docker().into_os_string();
        std::thread::spawn(move || {
            let mut options = AgentOptions::new(Launch::Stopped);
            options.test_root = Some(&root);
            options.docker = Some(std::ffi::OsStr::new(FAKE_DOCKER));
            options.docker_environment = vec![(OsString::from("FAKE_DOCKER_DIR"), engine)];
            options.store = Some(store);
            marshal_cli::agent::run(options, &mut std::io::stderr())
        })
    };

    crate::support::wait_for_agent(&scratch).await;
    let running = run_session(scratch.roots(), Arc::new(NoLauncher));
    running
        .screens
        .until("the list of pools", Duration::from_secs(30), |shown| {
            shown.view.list().is_some()
        })
        .await;

    running
        .handle
        .set_token("o/r".to_string(), format!("  {TOKEN}\n"))
        .await
        .expect("the agent stored the token");

    let stored = store
        .get(&RepositoryKey::of("o", "r"))
        .unwrap()
        .expect("the token reached the store");
    assert_eq!(
        stored.expose(),
        TOKEN,
        "the token is trimmed, and nothing else"
    );

    // no view the window was given, and no line the app logged, holds the token
    let seen = serde_json::to_string(&running.screens.all()).unwrap();
    assert!(!seen.contains(TOKEN), "a view holds the token");
    assert!(
        !running.log.text().contains(TOKEN),
        "the log holds the token"
    );

    running.handle.quit();
    // the session ends first: joining the agent's thread blocks this runtime's only thread
    tokio::time::timeout(Duration::from_secs(60), running.ended)
        .await
        .expect("the app ended")
        .unwrap();
    let code = agent.join().unwrap();
    assert_eq!(code, 0);
}
