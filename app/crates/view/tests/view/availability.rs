//! every row of the plan's availability table, for the header buttons, the banner's
//! action and the menu.

use marshal_view::{
    Action, Cause, Connection, Control, Input, Look, Pending, PendingKind, View, build,
};

use crate::support::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Can {
    Yes,
    No,
}
use Can::{No, Yes};

/// what the table says of the three agent actions in a state.
struct Row {
    state: &'static str,
    view: View,
    start: Can,
    stop: Can,
    build: Can,
}

fn can(control: &Control) -> Can {
    if control.available { Yes } else { No }
}

fn rows() -> Vec<Row> {
    let heard = at(3, 50, 0);
    let lost = |cause| Connection::LostContact {
        last_heard_ms: heard,
        cause,
    };
    let running = load("running");
    let mut rows = vec![
        Row {
            state: "running",
            view: view_of(&running),
            start: No,
            stop: Yes,
            build: No,
        },
        Row {
            state: "resuming",
            view: view_of(&load("resuming")),
            start: No,
            stop: Yes,
            build: No,
        },
        Row {
            state: "stopped by you",
            view: view_of(&load("stopped_by_you")),
            start: Yes,
            stop: No,
            build: Yes,
        },
        Row {
            state: "stopped, the image is missing",
            view: view_of(&load("stopped_image_missing")),
            start: Yes,
            stop: No,
            build: Yes,
        },
        Row {
            state: "stopped, the build failed",
            view: view_of(&load("build_failed")),
            start: Yes,
            stop: No,
            build: Yes,
        },
        Row {
            state: "stopped, the image is out of date",
            view: view_of(&load("image_out_of_date_stopped")),
            start: Yes,
            stop: No,
            build: Yes,
        },
        Row {
            state: "stopped, no pool could start",
            view: view_of(&load("stopped_no_pool_started")),
            start: Yes,
            stop: No,
            build: Yes,
        },
        Row {
            state: "stopped, the restart limit",
            view: view_of(&load("stopped_restart_limit")),
            start: Yes,
            stop: No,
            build: Yes,
        },
        Row {
            state: "stopped, no configuration",
            view: view_of(&load("stopped_no_configuration")),
            start: Yes,
            stop: No,
            build: No,
        },
        Row {
            state: "stopped, invalid configuration",
            view: view_of(&load("stopped_invalid_configuration")),
            start: Yes,
            stop: No,
            build: No,
        },
        Row {
            state: "building",
            view: view_of(&load("building")),
            start: No,
            stop: No,
            build: No,
        },
        Row {
            state: "stopping",
            view: view_of(&load("stopping")),
            start: No,
            stop: No,
            build: No,
        },
        Row {
            state: "quitting",
            view: view_of(&load("quitting")),
            start: No,
            stop: No,
            build: No,
        },
        Row {
            state: "starting Marshal",
            view: view_in(Connection::StartingMarshal, None),
            start: No,
            stop: No,
            build: No,
        },
        Row {
            state: "lost contact",
            view: view_in(lost(Cause::Dropped), Some(&running)),
            start: No,
            stop: No,
            build: No,
        },
        Row {
            state: "not answering",
            view: view_in(Connection::NotAnswering, Some(&running)),
            start: No,
            stop: No,
            build: No,
        },
        Row {
            state: "busy",
            view: view_in(lost(Cause::Busy), Some(&running)),
            start: No,
            stop: No,
            build: No,
        },
        Row {
            state: "the lock is held",
            view: view_in(Connection::LockHeld, None),
            start: No,
            stop: No,
            build: No,
        },
        Row {
            state: "no agent",
            view: view_in(Connection::NoAgent, Some(&running)),
            start: No,
            stop: No,
            build: No,
        },
        Row {
            state: "an agent state not known",
            view: view_of(&load("unknown_agent_state")),
            start: No,
            stop: No,
            build: No,
        },
    ];
    // the app's own quit, with the agent still up behind it
    rows.push(Row {
        state: "the app quitting over a running agent",
        view: view_in(Connection::Quitting, Some(&running)),
        start: No,
        stop: No,
        build: No,
    });
    rows
}

#[test]
fn the_menu_follows_the_availability_table_in_every_state() {
    for row in rows() {
        let menu = &row.view.menu;
        assert_eq!(
            menu_item(&row.view, &Action::StartAll).available,
            row.start == Yes,
            "start in {}",
            row.state
        );
        assert_eq!(
            menu_item(&row.view, &Action::StopAll).available,
            row.stop == Yes,
            "stop in {}",
            row.state
        );
        assert_eq!(
            menu_item(&row.view, &Action::BuildImage).available,
            row.build == Yes,
            "build in {}",
            row.state
        );
        let open_and_quit = !row.state.contains("quitting");
        assert_eq!(
            menu_item(&row.view, &Action::OpenMarshal).available,
            open_and_quit,
            "open in {}",
            row.state
        );
        assert_eq!(
            menu_item(&row.view, &Action::Quit).available,
            open_and_quit,
            "quit in {}",
            row.state
        );
        for item in &menu.items {
            assert_eq!(
                item.reason.is_some(),
                !item.available,
                "{} in {}",
                item.label,
                row.state
            );
            if let Some(why) = &item.reason {
                assert!(
                    why.starts_with("Unavailable: ") && why.ends_with('.'),
                    "{why}"
                );
            }
        }
    }
}

#[test]
fn the_menu_keeps_its_items_in_place_and_in_order() {
    for row in rows() {
        let labels: Vec<_> = row
            .view
            .menu
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "Open Marshal",
                "Start all runners",
                "Stop all runners",
                "Build image",
                "Quit Marshal"
            ],
            "{}",
            row.state
        );
    }
}

#[test]
fn the_header_buttons_follow_the_availability_table_in_every_state() {
    for row in rows() {
        let Some(list) = row.view.list() else {
            continue;
        };
        for (action, expected) in [
            (Action::StartAll, row.start),
            (Action::StopAll, row.stop),
            (Action::BuildImage, row.build),
        ] {
            let Some(control) = list
                .header
                .controls
                .iter()
                .find(|control| control.action == action)
            else {
                continue;
            };
            assert_eq!(can(control), expected, "{action:?} in {}", row.state);
            if expected == No {
                assert_eq!(control.look, Look::Neutral, "{action:?} in {}", row.state);
                assert!(control.reason.is_some(), "{action:?} in {}", row.state);
            }
        }
        for action in [Action::StartAll, Action::StopAll] {
            assert!(
                list.header
                    .controls
                    .iter()
                    .any(|control| control.action == action),
                "the header keeps {action:?} in place in {}",
                row.state
            );
        }
    }
}

#[test]
fn the_solid_action_is_the_one_the_state_calls_for() {
    let running = view_of(&load("running"));
    assert_eq!(header(&running, &Action::StopAll).look, Look::Destructive);
    assert_eq!(header(&running, &Action::StartAll).look, Look::Neutral);
    assert!(
        running
            .list()
            .expect("a list")
            .header
            .controls
            .iter()
            .all(|control| control.action != Action::BuildImage),
        "Build Image is in the header only when the image calls for it"
    );
    let stopped = view_of(&load("stopped_by_you"));
    assert_eq!(header(&stopped, &Action::StartAll).look, Look::Solid);
    assert_eq!(header(&stopped, &Action::StopAll).look, Look::Neutral);
}

#[test]
fn each_unavailable_action_says_why_in_the_words_of_its_state() {
    let running = view_of(&load("running"));
    assert_eq!(
        reason(header(&running, &Action::StartAll)),
        "Unavailable: the runners are already running."
    );
    assert_eq!(
        menu_item(&running, &Action::BuildImage).reason.as_deref(),
        Some("Unavailable: stop the runners first.")
    );

    let resuming = view_of(&load("resuming"));
    assert_eq!(
        reason(header(&resuming, &Action::StartAll)),
        "Unavailable: the runners are starting."
    );
    assert_eq!(
        menu_item(&resuming, &Action::BuildImage).reason.as_deref(),
        Some("Unavailable: stop the runners first.")
    );

    let stopped = view_of(&load("stopped_by_you"));
    assert_eq!(
        reason(header(&stopped, &Action::StopAll)),
        "Unavailable: the runners are already stopped."
    );

    for name in ["stopped_no_configuration", "stopped_invalid_configuration"] {
        let view = view_of(&load(name));
        assert_eq!(
            menu_item(&view, &Action::BuildImage).reason.as_deref(),
            Some(
                "Unavailable: Build needs a valid configuration, because the image name comes from it."
            ),
            "{name}"
        );
    }

    let building = view_of(&load("building"));
    for action in [Action::StartAll, Action::StopAll, Action::BuildImage] {
        assert_eq!(
            reason(header(&building, &action)),
            "Unavailable: the runner image is being built."
        );
    }
    let stopping = view_of(&load("stopping"));
    for action in [Action::StartAll, Action::StopAll] {
        assert_eq!(
            reason(header(&stopping, &action)),
            "Unavailable: the runners are stopping."
        );
    }
    assert_eq!(
        menu_item(&stopping, &Action::BuildImage).reason.as_deref(),
        Some("Unavailable: the runners are stopping.")
    );
}

#[test]
fn nothing_but_try_again_and_the_two_always_there_is_available_without_control() {
    let running = load("running");
    let cases = [
        (
            Connection::StartingMarshal,
            None,
            "Unavailable: Marshal is starting.",
        ),
        (
            Connection::NotAnswering,
            Some(&running),
            "Unavailable: the agent is not answering.",
        ),
        (
            Connection::LockHeld,
            None,
            "Unavailable: another Marshal is running.",
        ),
        (
            Connection::NoAgent,
            Some(&running),
            "Unavailable: Marshal cannot reach its agent.",
        ),
        (
            Connection::LostContact {
                last_heard_ms: at(3, 50, 0),
                cause: Cause::Dropped,
            },
            Some(&running),
            "Unavailable: Marshal has lost contact with the agent.",
        ),
        (
            Connection::Quitting,
            Some(&running),
            "Unavailable: Marshal is quitting.",
        ),
    ];
    for (connection, snapshot, expected) in cases {
        let view = view_in(connection, snapshot);
        for action in [Action::StartAll, Action::StopAll, Action::BuildImage] {
            assert_eq!(
                menu_item(&view, &action).reason.as_deref(),
                Some(expected),
                "{action:?} in {connection:?}"
            );
        }
    }
}

#[test]
fn try_again_is_offered_only_where_the_table_gives_it() {
    let running = load("running");
    for (connection, offered) in [
        (Connection::NoAgent, true),
        (Connection::NotAnswering, true),
        (
            Connection::LostContact {
                last_heard_ms: at(3, 50, 0),
                cause: Cause::RestartFailed,
            },
            true,
        ),
        (
            Connection::LostContact {
                last_heard_ms: at(3, 50, 0),
                cause: Cause::Busy,
            },
            true,
        ),
        (
            Connection::LostContact {
                last_heard_ms: at(3, 50, 0),
                cause: Cause::Dropped,
            },
            false,
        ),
        (Connection::LockHeld, false),
        (Connection::StartingMarshal, false),
        (Connection::Quitting, false),
    ] {
        let view = view_in(connection, Some(&running));
        let action = match view.window.body {
            marshal_view::Body::Full(ref full) => full.action.clone(),
            marshal_view::Body::List(ref list) => list
                .banners
                .first()
                .and_then(|banner| banner.action.clone()),
        };
        assert_eq!(
            action
                .as_ref()
                .is_some_and(|control| control.action == Action::TryAgain && control.available),
            offered,
            "{connection:?}"
        );
    }
}

#[test]
fn an_action_already_asked_for_is_unavailable_at_once() {
    let running = load("running");
    let stopped = load("stopped_by_you");
    let ask = |snapshot, kind| {
        build(
            &Input::new(Connection::Connected, Some(snapshot), now()).pending(Some(Pending {
                kind,
                since_ms: at(3, 52, 0),
            })),
        )
    };
    let stopping = ask(&running, PendingKind::StopAll);
    assert!(!menu_item(&stopping, &Action::StopAll).available);
    assert!(!menu_item(&stopping, &Action::StartAll).available);
    assert!(!menu_item(&stopping, &Action::BuildImage).available);

    let starting = ask(&stopped, PendingKind::StartAll);
    assert!(!menu_item(&starting, &Action::StartAll).available);
    assert!(menu_item(&starting, &Action::StopAll).available);
    assert!(!menu_item(&starting, &Action::BuildImage).available);

    let building = ask(&stopped, PendingKind::Build);
    for action in [Action::StartAll, Action::StopAll, Action::BuildImage] {
        assert!(!menu_item(&building, &action).available, "{action:?}");
    }
}

#[test]
fn a_build_in_progress_offers_only_stop_the_build() {
    let building = view_of(&load("building"));
    let stop = first_banner(&building).action.as_ref().expect("an action");
    assert_eq!(stop.action, Action::StopBuild);
    assert!(stop.available);
    let stopped = view_of(&load("stopped_by_you"));
    assert!(first_banner(&stopped).action.is_none());
}

#[test]
fn the_questions_asked_before_the_actions_that_cost_something() {
    let running = view_of(&load("running"));
    let question = menu_item(&running, &Action::StopAll)
        .confirmation
        .as_ref()
        .expect("a question");
    assert_eq!(question.title, "Stop all runners?");
    assert_eq!(question.confirm, "Stop");
    assert_eq!(question.cancel, "Cancel");
    assert!(question.body.contains("Any job running on them fails"));
    let header_question = header(&running, &Action::StopAll)
        .confirmation
        .as_ref()
        .expect("a question");
    assert_eq!(
        header_question, question,
        "the window and the menu ask the same"
    );

    let stopped = view_of(&load("stopped_by_you"));
    assert!(
        menu_item(&stopped, &Action::StartAll)
            .confirmation
            .is_none()
    );
    assert!(
        menu_item(&stopped, &Action::StopAll).confirmation.is_none(),
        "an unavailable action asks nothing"
    );
}

#[test]
fn the_quit_question_while_marshal_reaches_its_agent_says_it_waits_and_quits_it() {
    let running = load("running");
    let reaching = [
        view_in(Connection::StartingMarshal, None),
        view_in(
            Connection::LostContact {
                last_heard_ms: at(3, 50, 0),
                cause: Cause::Dropped,
            },
            Some(&running),
        ),
    ];
    for view in &reaching {
        let question = menu_item(view, &Action::Quit)
            .confirmation
            .as_ref()
            .expect("a question");
        assert!(
            question.body.contains("Quit waits for it, stops it"),
            "{}",
            question.body
        );
        assert!(
            !question.body.contains("keep running"),
            "an agent this app is starting is not left running: {}",
            question.body
        );
    }
    let not_answering = view_in(Connection::NotAnswering, Some(&running));
    let question = menu_item(&not_answering, &Action::Quit)
        .confirmation
        .as_ref()
        .expect("a question");
    assert!(question.body.contains("any runners keep running"));
}
