//! protocol version 2 on the wire: the handshake, the methods, the events and the
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! version rules, spoken by hand and with the library's client.
mod support;

use std::time::Duration;

use marshal_cli::agent::Push;
use marshal_cli::agent::machine::Launch;
use marshal_cli::agent::server::Limits;
use serde_json::{Value, json};
use support::agent::{ONE_POOL, Raw, Rig, TOKEN};

#[tokio::test(flavor = "current_thread")]
async fn a_first_line_that_is_not_hello_is_refused_with_the_versions_served_and_closed() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            for first in [
                r#"{"type":"request","id":1,"method":"status"}"#,
                r#"stop"#,
                r#"{"type":"hello","versions":[1]}"#,
                r#"{"type":"hello","versions":[]}"#,
                r#"[]"#,
            ] {
                let mut raw = Raw::open(&conn.roots).await;
                raw.send_text(first).await;
                let error = raw.next().await.unwrap();
                assert_eq!(error["type"], "error", "{first}: {error}");
                assert_eq!(error["error"]["code"], "unsupported_protocol");
                assert_eq!(error["error"]["versions"], json!([2]));
                assert!(raw.closed_within(Duration::from_secs(5)).await, "{first}");
            }
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn welcome_lists_the_version_the_methods_and_the_topics() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut raw = Raw::open(&conn.roots).await;
            raw.send(&json!({"type":"hello","versions":[7,2,1],"client":{"name":"t"},"later":1}))
                .await;
            let welcome = raw.next().await.unwrap();
            assert_eq!(welcome["type"], "welcome");
            assert_eq!(welcome["version"], 2);
            assert_eq!(welcome["agent"]["version"], env!("CARGO_PKG_VERSION"));
            let methods: Vec<&str> = welcome["methods"]
                .as_array()
                .unwrap()
                .iter()
                .map(|method| method.as_str().unwrap())
                .collect();
            for method in [
                "status",
                "subscribe",
                "start_all",
                "stop_all",
                "build",
                "quit",
                "token_set",
                "token_remove",
            ] {
                assert!(methods.contains(&method), "{method} is not served");
            }
            assert_eq!(welcome["topics"], json!(["state"]));
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_client_meets_unknown_methods_topics_and_fields_without_harm() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut raw = Raw::hello(&conn.roots).await;
            raw.request(1, "dance", json!({})).await;
            let answer = raw.answer().await;
            assert_eq!(answer["id"], 1);
            assert_eq!(answer["error"]["code"], "unknown_method");

            raw.request(2, "subscribe", json!({"topic": "weather"}))
                .await;
            assert_eq!(raw.answer().await["error"]["code"], "invalid_request");

            // fields the agent does not know are ignored, in the message and the params
            raw.send(&json!({"type":"request","id":3,"method":"status","params":{"x":1},"y":2}))
                .await;
            let answer = raw.answer().await;
            assert_eq!(answer["id"], 3);
            assert_eq!(answer["result"]["agent"]["state"], "stopped");

            // a message that is not a request is answered with an error, and the
            // connection stays
            raw.send_text(r#"{"type":"nonsense"}"#).await;
            assert_eq!(raw.answer().await["error"]["code"], "invalid_request");
            raw.request(4, "status", Value::Null).await;
            assert_eq!(raw.answer().await["id"], 4);

            // a request with bad params is invalid, not unknown
            raw.request(5, "token_set", json!({"repository": 5})).await;
            assert_eq!(raw.answer().await["error"]["code"], "invalid_request");
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_subscription_gets_the_snapshot_at_once_and_an_event_for_each_change() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut watcher = conn.another().await;
            watcher.client.subscribe().await.unwrap();
            let first = watcher.push().await;
            let Push::Event { seq, snapshot } = first else {
                panic!("not an event: {first:?}");
            };
            assert_eq!(seq, 1);
            assert_eq!(snapshot.agent.state, "stopped");

            let started = std::time::Instant::now();
            conn.ok("start_all").await;
            let mut last = seq;
            loop {
                let Push::Event { seq, snapshot } = watcher.push().await else {
                    continue;
                };
                assert_eq!(seq, last + 1, "seq rises by one");
                last = seq;
                if snapshot.agent.state != "stopped" {
                    break;
                }
            }
            assert!(started.elapsed() < Duration::from_secs(1));
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn events_are_coalesced_to_one_in_a_window() {
    let rig = Rig::new(ONE_POOL);
    rig.limits.set(Limits {
        coalesce: Duration::from_millis(250),
        ..Limits::default()
    });
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut watcher = conn.another().await;
            watcher.client.subscribe().await.unwrap();
            watcher.push().await;
            let started = std::time::Instant::now();
            // the first change is pushed after the window that follows the previous push
            conn.set_token("o/r", TOKEN).await.unwrap();
            conn.client.token_remove("o/r").await.unwrap();
            conn.set_token("o/r", TOKEN).await.unwrap();
            let mut events = 0;
            while started.elapsed() < Duration::from_millis(700) {
                if let Ok(Some(Push::Event { .. })) =
                    tokio::time::timeout(Duration::from_millis(100), watcher.pushes.recv()).await
                {
                    events += 1;
                }
            }
            assert!(events >= 1, "the change was never pushed");
            assert!(
                events <= 2,
                "{events} events for three changes in one window"
            );
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_quiet_subscription_gets_a_heartbeat() {
    let rig = Rig::new(ONE_POOL);
    rig.limits.set(Limits {
        heartbeat: Duration::from_millis(300),
        ..Limits::default()
    });
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut watcher = conn.another().await;
            watcher.client.subscribe().await.unwrap();
            let Push::Event { seq, .. } = watcher.push().await else {
                panic!("the snapshot comes first");
            };
            let Push::Heartbeat { seq: beat } = watcher.push().await else {
                panic!("a heartbeat follows a quiet while");
            };
            assert_eq!(beat, seq, "a heartbeat carries the last seq");
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn ending_is_pushed_to_every_connection_before_the_agent_ends() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut watcher = conn.another().await;
            watcher.client.subscribe().await.unwrap();
            let mut quiet = Raw::hello(&conn.roots).await;
            conn.ok("quit").await;
            loop {
                match watcher.push().await {
                    Push::Ending { reason } => {
                        assert_eq!(reason, "quit");
                        break;
                    }
                    Push::Closed => panic!("closed without an ending"),
                    _ => {}
                }
            }
            let ending = quiet.next().await.unwrap();
            assert_eq!(ending["type"], "ending");
            assert_eq!(ending["reason"], "quit");
            assert!(quiet.closed_within(Duration::from_secs(5)).await);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_fifth_subscription_is_refused_and_a_closed_one_frees_its_place() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut held = Vec::new();
            for _ in 0..4 {
                let watcher = conn.another().await;
                watcher.client.subscribe().await.unwrap();
                held.push(watcher);
            }
            let fifth = conn.another().await;
            let error = fifth.client.subscribe().await.unwrap_err();
            assert!(error.to_string().contains("subscription_limit"), "{error}");
            // the refused connection is still usable
            fifth.status().await;
            drop(held.pop());
            let mut again = 0;
            loop {
                if fifth.client.subscribe().await.is_ok() {
                    break;
                }
                again += 1;
                assert!(again < 200, "the place was never freed");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_seventeenth_connection_is_told_it_is_busy() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut held = vec![];
            for _ in 0..15 {
                held.push(conn.another().await);
            }
            // the first is `conn`, so sixteen are open
            let mut seventeenth = Raw::open(&conn.roots).await;
            seventeenth
                .send(&json!({"type":"hello","versions":[2]}))
                .await;
            let error = seventeenth.next().await.unwrap();
            assert_eq!(error["error"]["code"], "busy", "{error}");
            assert!(seventeenth.closed_within(Duration::from_secs(5)).await);
            // the sixteen are unharmed
            held[0].status().await;
            drop(held);
            conn.ok("quit").await;
        })
        .await;
}

// the limits as timers ---------------------------------------------------------------

fn quick() -> Limits {
    Limits {
        handshake: Duration::from_millis(300),
        idle: Duration::from_millis(400),
        write: Duration::from_millis(300),
        ..Limits::default()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_connection_that_never_says_hello_is_closed() {
    let rig = Rig::new(ONE_POOL);
    rig.limits.set(quick());
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut silent = Raw::open(&conn.roots).await;
            assert!(silent.closed_within(Duration::from_secs(5)).await);
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_idle_connection_is_closed_and_a_subscribed_one_is_not() {
    let rig = Rig::new(ONE_POOL);
    rig.limits.set(quick());
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut idle = Raw::hello(&conn.roots).await;
            let watcher = conn.another().await;
            watcher.client.subscribe().await.unwrap();
            assert!(idle.closed_within(Duration::from_secs(5)).await);
            tokio::time::sleep(Duration::from_millis(600)).await;
            watcher.status().await;
            // the first connection was idle too
            assert!(conn.client.call("status", Value::Null).await.is_err());
            conn.another().await.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_connection_that_stops_reading_is_closed_and_costs_no_thread() {
    let rig = Rig::new(ONE_POOL);
    rig.limits.set(Limits {
        write: Duration::from_millis(300),
        coalesce: Duration::from_millis(10),
        ..Limits::default()
    });
    let rig = &rig;
    let (_, (before, during)) = rig
        .drive(Launch::Stopped, |conn| async move {
            let before = support::agent::threads();
            let mut stalled = Vec::new();
            for _ in 0..8 {
                let mut raw = Raw::hello(&conn.roots).await;
                raw.request(1, "subscribe", json!({"topic": "state"})).await;
                stalled.push(raw);
            }
            // every change is an event the stalled connections never read
            for round in 0..200 {
                if round % 2 == 0 {
                    conn.set_token("o/r", TOKEN).await.unwrap();
                } else {
                    conn.client.token_remove("o/r").await.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            let during = support::agent::threads();
            // the agent still answers every other connection
            conn.status().await;
            conn.ok("quit").await;
            drop(stalled);
            (before, during)
        })
        .await;
    if let (Some(before), Some(during)) = (before, during) {
        assert!(
            during <= before,
            "{} threads are held for stalled readers",
            during - before
        );
    }
}
