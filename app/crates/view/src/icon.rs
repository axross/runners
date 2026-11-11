use marshal_cli::agent::snapshot::Snapshot;

/// the five states the icon's shape tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    AllUp,
    SomeNotUp,
    Stopped,
    StoppedByError,
    NotInControl,
}

/// the stop reasons that mean something went wrong; any other reason, `by_you`
/// included, is a stop the user can read as intended.
const ERROR_REASONS: [&str; 6] = [
    "no_configuration",
    "invalid_configuration",
    "image_unusable",
    "no_pool_started",
    "restart_limit",
    "image_build_failed",
];

/// the icon's state for the latest snapshot, or for none: without one, Marshal is
/// not in control of the runners.
#[must_use]
pub fn icon_state(snapshot: Option<&Snapshot>) -> IconState {
    let Some(snapshot) = snapshot else {
        return IconState::NotInControl;
    };
    match snapshot.agent.state.as_str() {
        "running" if snapshot.runners_up >= snapshot.runners_wanted => IconState::AllUp,
        "running" | "resuming" => IconState::SomeNotUp,
        "stopped" if stopped_by_error(snapshot) => IconState::StoppedByError,
        _ => IconState::Stopped,
    }
}

fn stopped_by_error(snapshot: &Snapshot) -> bool {
    snapshot
        .agent
        .reason
        .as_ref()
        .is_some_and(|reason| ERROR_REASONS.contains(&reason.code.as_str()))
}

/// the icon's tooltip for the latest snapshot.
#[must_use]
pub fn tooltip(snapshot: Option<&Snapshot>) -> String {
    let Some(snapshot) = snapshot else {
        return "Marshal: not in control of the runners".to_string();
    };
    match snapshot.agent.state.as_str() {
        "building" => "Marshal: building the runner image".to_string(),
        "running" | "resuming" => format!(
            "Marshal: {} of {} runners up",
            snapshot.runners_up, snapshot.runners_wanted
        ),
        _ => "Marshal: runners stopped".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(state: &str, reason: Option<&str>, up: u32, wanted: u32) -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "agent": {
                "state": state,
                "reason": reason.map(|code| serde_json::json!({ "code": code })),
            },
            "runners_up": up,
            "runners_wanted": wanted,
        }))
        .expect("a snapshot")
    }

    #[test]
    fn no_snapshot_means_marshal_is_not_in_control() {
        assert_eq!(icon_state(None), IconState::NotInControl);
        assert_eq!(tooltip(None), "Marshal: not in control of the runners");
    }

    #[test]
    fn every_runner_up_is_all_up() {
        let all = snapshot("running", None, 4, 4);
        assert_eq!(icon_state(Some(&all)), IconState::AllUp);
        assert_eq!(tooltip(Some(&all)), "Marshal: 4 of 4 runners up");
    }

    #[test]
    fn a_runner_missing_is_some_not_up() {
        let some = snapshot("running", None, 3, 4);
        assert_eq!(icon_state(Some(&some)), IconState::SomeNotUp);
        assert_eq!(tooltip(Some(&some)), "Marshal: 3 of 4 runners up");
        let resuming = snapshot("resuming", None, 0, 4);
        assert_eq!(icon_state(Some(&resuming)), IconState::SomeNotUp);
    }

    #[test]
    fn a_stop_by_the_user_is_stopped() {
        let stopped = snapshot("stopped", Some("by_you"), 0, 4);
        assert_eq!(icon_state(Some(&stopped)), IconState::Stopped);
        assert_eq!(tooltip(Some(&stopped)), "Marshal: runners stopped");
    }

    #[test]
    fn every_failure_reason_is_stopped_by_an_error() {
        for code in ERROR_REASONS {
            let stopped = snapshot("stopped", Some(code), 0, 4);
            assert_eq!(
                icon_state(Some(&stopped)),
                IconState::StoppedByError,
                "{code}"
            );
        }
    }

    #[test]
    fn a_reason_this_reader_does_not_know_is_not_an_error() {
        let stopped = snapshot("stopped", Some("a_later_reason"), 0, 4);
        assert_eq!(icon_state(Some(&stopped)), IconState::Stopped);
    }

    #[test]
    fn a_build_shows_stopped_and_says_so() {
        let building = snapshot("building", None, 0, 4);
        assert_eq!(icon_state(Some(&building)), IconState::Stopped);
        assert_eq!(
            tooltip(Some(&building)),
            "Marshal: building the runner image"
        );
    }
}
