//! the checkout rule for the agent: its configuration lives outside any checkout, so
//! the checkout a `diagnosticDirectory` must lie outside is found from the directory
//! itself, while `start`'s rule, the checkout above the configuration file, is
//! unchanged. `docs/operations/cli.md` describes both.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use marshal_cli::profile::Profile;
use marshal_cli::validate::{CheckoutRule, Failure, validate_with};
use support::TempDir;

const OUTSIDE: &str = "repositories[0].diagnosticDirectory: must be outside the checkout";

fn checkout() -> TempDir {
    let tree = TempDir::new();
    tree.write("checkout/.git/HEAD", b"ref: refs/heads/main\n");
    tree.write("checkout/cli/Cargo.toml", b"[package]\n");
    tree
}

fn errors(outcome: Result<marshal_cli::validate::Plan, Failure>) -> Vec<String> {
    match outcome {
        Err(Failure::Invalid(errors)) => errors,
        other => panic!("expected problems, got {other:?}"),
    }
}

#[cfg(unix)]
fn axmc(directory: &str) -> Vec<u8> {
    format!(
        r#"{{"budget":{{"cpus":8,"memoryGb":32,"diskGb":200}},"repositories":[
        {{"owner":"o","repository":"r","name":"r","os":"linux","slots":1,"tokenPath":"/t",
          "diskGb":40,"diagnostics":true,"diagnosticDirectory":"{directory}"}}]}}"#
    )
    .into_bytes()
}

#[cfg(unix)]
#[test]
fn a_sink_inside_a_checkout_is_refused_from_the_sinks_own_place() {
    let tree = checkout();
    let root = tree.path().join("checkout");
    for directory in [root.join("evidence"), root.join("a/b"), root.clone()] {
        let directory = directory.to_str().unwrap();
        let validation = validate_with(
            &axmc(directory),
            Profile::Axmc,
            CheckoutRule::AboveAndAtSink(None),
        );
        let found = errors(validation.outcome);
        assert!(found.iter().any(|e| e == OUTSIDE), "{directory}: {found:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_sink_outside_a_checkout_is_accepted_and_a_sibling_prefix_is_not_confused() {
    let tree = checkout();
    for directory in [
        tree.path().join("evidence"),
        tree.path().join("checkout-evidence"),
        tree.path().join("CHECKOUT/evidence"),
    ] {
        let directory = directory.to_str().unwrap();
        let validation = validate_with(
            &axmc(directory),
            Profile::Axmc,
            CheckoutRule::AboveAndAtSink(None),
        );
        assert!(validation.outcome.is_ok(), "{directory}: {validation:?}");
        assert!(!validation.checkout_rule_skipped, "the sink was examined");
    }
}

#[cfg(unix)]
#[test]
fn starts_rule_is_unchanged_and_still_skips_without_a_checkout_above_the_file() {
    use marshal_cli::validate::validate;
    let tree = checkout();
    let inside = tree.path().join("checkout/evidence");
    let validation = validate(&axmc(inside.to_str().unwrap()), Profile::Axmc, None);
    assert!(validation.outcome.is_ok());
    assert!(validation.checkout_rule_skipped);
}

#[cfg(unix)]
#[test]
fn the_checkout_above_the_configuration_is_still_applied_beside_the_sink_s() {
    let tree = checkout();
    let above = tree.path().join("checkout");
    let elsewhere = TempDir::new();
    // the sink is in no checkout of its own, but lies inside the one given
    let sink = above.join("a");
    let validation = validate_with(
        &axmc(sink.to_str().unwrap()),
        Profile::Axmc,
        CheckoutRule::AboveAndAtSink(Some(&above)),
    );
    assert!(errors(validation.outcome).iter().any(|e| e == OUTSIDE));
    let validation = validate_with(
        &axmc(elsewhere.path().join("e").to_str().unwrap()),
        Profile::Axmc,
        CheckoutRule::AboveAndAtSink(Some(&above)),
    );
    assert!(validation.outcome.is_ok());
}

#[cfg(windows)]
mod windows {
    use super::*;

    fn axpc(directory: &str) -> Vec<u8> {
        format!(
            r#"{{"imageName":"i","repositories":[{{"owner":"o","repository":"r","name":"r",
            "slots":1,"tokenPath":"C:\\t","volumes":[],"diagnostics":true,
            "diagnosticDirectory":"{}"}}]}}"#,
            directory.replace('\\', "\\\\")
        )
        .into_bytes()
    }

    #[test]
    fn axpc_refuses_a_sink_inside_a_checkout_whatever_its_case_and_accepts_one_outside() {
        let tree = checkout();
        let inside = tree.path().join("checkout").join("evidence");
        let text = inside.to_str().unwrap();
        for directory in [text.to_string(), text.to_uppercase(), text.to_lowercase()] {
            let validation = validate_with(
                &axpc(&directory),
                Profile::Axpc,
                CheckoutRule::AboveAndAtSink(None),
            );
            let found = errors(validation.outcome);
            assert!(found.iter().any(|e| e == OUTSIDE), "{directory}: {found:?}");
        }
        let outside = tree.path().join("evidence");
        let validation = validate_with(
            &axpc(outside.to_str().unwrap()),
            Profile::Axpc,
            CheckoutRule::AboveAndAtSink(None),
        );
        assert!(validation.outcome.is_ok(), "{validation:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_agent_names_a_sink_inside_a_checkout_as_a_configuration_problem() {
        use marshal_cli::agent::machine::Launch;
        use support::agent::Rig;

        let tree = checkout();
        let inside = tree.path().join("checkout").join("evidence");
        let config = String::from_utf8(axpc(inside.to_str().unwrap())).unwrap();
        let rig = Rig::new(&config);
        let rig = &rig;
        let (_, ()) = rig
            .drive(Launch::Running, |conn| async move {
                let snapshot = conn.until_state("stopped").await;
                assert_eq!(
                    snapshot.agent.reason.as_ref().unwrap().code,
                    "invalid_configuration"
                );
                assert!(
                    snapshot
                        .configuration
                        .problems
                        .iter()
                        .any(|problem| problem.ends_with(&OUTSIDE["repositories[0].".len()..])),
                    "{snapshot:#?}"
                );
                conn.ok("quit").await;
            })
            .await;
    }
}
