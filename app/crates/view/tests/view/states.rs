//! the window, tooltip, menu line and icon of each state in the boards' UI design,
//! drawn and not drawn.

use marshal_view::{
    Action, BannerKind, Body, Cause, Connection, IconState, Input, Look, Mark, Pending,
    PendingKind, Tone, build,
};

use crate::support::*;

const LOG: &str = r"%LOCALAPPDATA%\marshal\logs\";

#[test]
fn running_is_the_summary_the_pools_and_the_runners() {
    let snapshot = load("running");
    let view = view_of(&snapshot);
    assert_eq!(view.summary.heading, "3 of 6 runners up");
    assert_eq!(
        view.summary.line,
        "Running since today 03:15 · 3 pools, 1 skipped"
    );
    assert_eq!(view.tooltip, "Marshal: 3 of 6 runners up");
    assert_eq!(view.menu.summary, "3 of 6 runners up");
    assert_eq!(view.icon, IconState::SomeNotUp);
    let list = list(&view);
    assert!(list.banners.is_empty());
    assert!(!list.stale);

    let app = &list.pools[0];
    assert_eq!(
        (app.name.as_str(), app.repository.as_str()),
        ("app", "owner/app")
    );
    assert_eq!(app.line, "Running · 2 of 2 up");
    let first = &app.runners[0];
    assert_eq!(first.name, "Runner 1");
    assert_eq!(first.state, "Container running");
    assert_eq!(first.mark, Some(Mark::Check));
    assert_eq!(first.container.as_deref(), Some("app-1-20261104031522114"));
    assert_eq!(first.since.as_deref(), Some("since 03:15"));
    assert_eq!(first.label, "app, Runner 1, container running");

    let site = &list.pools[1];
    assert_eq!(site.line, "Running · 1 of 3 up");
    assert_eq!(site.runners[1].state, "Registering");
    assert_eq!(site.runners[1].mark, Some(Mark::Ring));
    assert_eq!(site.runners[1].since.as_deref(), Some("since 03:40"));
    assert_eq!(site.runners[2].state, "Backing off");
    assert_eq!(site.runners[2].mark, Some(Mark::History));
    assert_eq!(
        site.runners[2].detail.as_deref(),
        Some("container exited with code 1")
    );

    let docs = &list.pools[2];
    assert_eq!(docs.line, "Skipped · 0 of 1 up");
    assert_eq!(docs.mark, Some(Mark::Flag));
    assert_eq!(
        docs.note.as_deref(),
        Some("No token for owner/docs. Start All skipped this pool.")
    );
    assert!(docs.runners.is_empty());

    let footer = &view.window.footer;
    assert_eq!(
        footer.image_lines,
        ["Runner image: contract 3 · built 2 days ago · runner 2.338.2, released 9 days ago"]
    );
    assert_eq!(
        footer.warnings,
        ["Warning: runner 2.338.2 was released 9 days ago, and a newer runner may exist."]
    );
    assert_eq!(
        footer.configuration,
        r"Configuration: C:\Users\<user>\AppData\Local\marshal\config.toml"
    );
    assert_eq!(
        footer.note,
        "Closing this window keeps Marshal running in the notification area."
    );
}

#[test]
fn a_held_runner_says_held_with_its_reason_and_the_history_mark() {
    let view = view_of(&load("held"));
    let site = &list(&view).pools[1];
    let held = &site.runners[1];
    assert_eq!(held.state, "Held");
    assert_eq!(held.mark, Some(Mark::History));
    assert_eq!(held.detail.as_deref(), Some("Softnet is not set up"));
    assert_eq!(held.container, None);
    assert_eq!(held.since.as_deref(), Some("since 03:40"));
    assert_eq!(held.label, "site, Runner 2, held");
    assert_eq!(site.line, "Running · 1 of 3 up", "a held runner is not up");
    assert_eq!(view.summary.heading, "3 of 6 runners up");
    assert_eq!(
        site.runners[2].state, "Backing off",
        "a backing-off runner next to it is as before"
    );
}

#[test]
fn a_held_runner_is_known_so_it_is_not_shown_as_an_unknown_state() {
    let snapshot = patched("held", |value| {
        value["pools"][1]["runners"][1]["reason"] = serde_json::Value::Null;
    });
    let view = view_of(&snapshot);
    let held = &list(&view).pools[1].runners[1];
    assert_eq!(held.state, "Held", "the word, as the reason is missing");
    assert_eq!(held.detail, None);
    assert_ne!(held.mark, Some(Mark::Unknown));
}

#[test]
fn a_held_runner_after_contact_is_lost_keeps_its_words_and_loses_its_mark() {
    let view = view_in(
        Connection::LostContact {
            last_heard_ms: at(3, 50, 0),
            cause: Cause::Dropped,
        },
        Some(&load("held")),
    );
    let held = &list(&view).pools[1].runners[1];
    assert_eq!(held.state, "Held");
    assert_eq!(held.mark, None);
}

#[test]
fn every_runner_up_says_all_and_shows_the_all_up_icon() {
    let snapshot = load("all_up");
    let view = view_of(&snapshot);
    assert_eq!(view.summary.heading, "All 3 runners up");
    assert_eq!(view.tooltip, "Marshal: all 3 runners up");
    assert_eq!(view.icon, IconState::AllUp);
}

#[test]
fn stopped_by_you_says_since_when_and_what_it_takes_to_start() {
    let snapshot = load("stopped_by_you");
    let view = view_of(&snapshot);
    assert_eq!(view.summary.heading, "0 of 6 runners up");
    assert_eq!(view.summary.line, "Stopped by you since today 03:40");
    assert_eq!(view.tooltip, "Marshal: stopped by you");
    assert_eq!(view.menu.summary, "Stopped by you");
    assert_eq!(view.icon, IconState::Stopped);
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::StoppedByYou);
    assert_eq!(banner.heading, "Runners stopped by you");
    assert_eq!(
        banner.lines,
        ["They stay stopped until you choose Start All."]
    );
    assert_eq!(banner.announcement, "Runners stopped by you");
    let list = list(&view);
    assert_eq!(list.pools[0].line, "Stopped · 0 of 2 up");
    assert!(list.pools[0].runners.is_empty());
    assert_eq!(list.pools[2].line, "Skipped · 0 of 1 up");
}

#[test]
fn no_configuration_names_the_file_and_lists_no_pools() {
    let snapshot = load("stopped_no_configuration");
    let view = view_of(&snapshot);
    assert_eq!(view.summary.heading, "No runners up");
    assert_eq!(view.menu.summary, "No runners up");
    assert_eq!(view.tooltip, "Marshal: stopped, there is no configuration");
    assert_eq!(view.icon, IconState::StoppedByError);
    let banner = first_banner(&view);
    assert_eq!(banner.heading, "Stopped: there is no configuration");
    assert_eq!(
        banner.lines,
        [
            r"C:\Users\<user>\AppData\Local\marshal\config.toml does not exist",
            "Create it as Marshal's operations document describes, then choose Start All."
        ]
    );
    assert_eq!(
        banner.announcement,
        r"Stopped: there is no configuration. C:\Users\<user>\AppData\Local\marshal\config.toml does not exist"
    );
    assert_eq!(banner.tone, Tone::Error);
    let list = list(&view);
    assert_eq!(
        list.pools_note.as_deref(),
        Some("No pools are listed while there is no configuration.")
    );
    assert_eq!(
        view.window.footer.image_lines,
        ["Runner image: not checked"]
    );
}

#[test]
fn a_missing_configuration_without_a_path_names_the_default_location() {
    let snapshot = patched("stopped_no_configuration", |value| {
        value["configuration"]["path"] = "".into();
    });
    let view = view_of(&snapshot);
    assert_eq!(
        first_banner(&view).lines[0],
        r"%LOCALAPPDATA%\marshal\config.toml does not exist"
    );
}

#[test]
fn an_invalid_configuration_lists_each_problem_on_its_own_line() {
    let snapshot = patched("stopped_invalid_configuration", |value| {
        value["configuration"]["problems"] = serde_json::json!([
            "repositories[1].slots: must be an integer from 1 to 16",
            "repositories[2].owner: must not be empty"
        ]);
    });
    let view = view_of(&snapshot);
    assert_eq!(
        view.tooltip,
        "Marshal: stopped, the configuration is not valid"
    );
    let banner = first_banner(&view);
    assert_eq!(banner.heading, "Stopped: the configuration is not valid");
    assert_eq!(
        banner.lines,
        [
            "repositories[1].slots: must be an integer from 1 to 16",
            "repositories[2].owner: must not be empty",
            r"Fix C:\Users\<user>\AppData\Local\marshal\config.toml, then choose Start All."
        ]
    );
    assert_eq!(
        list(&view).pools_note.as_deref(),
        Some("No pools are listed while the configuration cannot be read.")
    );
}

#[test]
fn an_invalid_configuration_with_no_problems_gives_the_agents_reason() {
    let snapshot = patched("stopped_invalid_configuration", |value| {
        value["configuration"]["problems"] = serde_json::json!([]);
        value["agent"]["reason"]["message"] = "the configuration names no image".into();
    });
    let view = view_of(&snapshot);
    assert_eq!(
        first_banner(&view).lines[0],
        "the configuration names no image"
    );
}

#[test]
fn a_missing_image_offers_build_image_as_the_one_solid_action() {
    let snapshot = load("stopped_image_missing");
    let view = view_of(&snapshot);
    assert_eq!(
        view.tooltip,
        "Marshal: stopped, the runner image cannot run jobs"
    );
    assert_eq!(view.icon, IconState::StoppedByError);
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::ImageCannotRunJobs);
    assert_eq!(banner.heading, "Stopped: the runner image cannot run jobs");
    assert_eq!(
        banner.lines,
        [
            "the image actions-runner:local is missing; Marshal needs one with image contract 3",
            "Build the runner image, then choose Start All."
        ]
    );
    let build = banner.action.as_ref().expect("Build Image");
    assert_eq!(
        (build.action.clone(), build.label.as_str()),
        (Action::BuildImage, "Build Image")
    );
    assert!(build.available);
    assert_eq!(build.look, Look::Solid);

    let start = header(&view, &Action::StartAll);
    assert!(start.available);
    assert_eq!(start.look, Look::Neutral);
    let header_build = header(&view, &Action::BuildImage);
    assert!(header_build.available);
    assert_eq!(header_build.look, Look::Tonal);
    assert_eq!(list(&view).header.controls.len(), 3);
    assert_eq!(view.window.footer.image_lines, ["Runner image: missing"]);
}

#[test]
fn an_image_of_another_contract_gives_the_agents_reason() {
    let snapshot = load("stopped_image_mismatch");
    let view = view_of(&snapshot);
    assert_eq!(
        first_banner(&view).lines[0],
        "the image actions-runner:local has image contract 2, but Marshal needs image contract 3"
    );
    assert_eq!(
        view.window.footer.image_lines,
        ["Runner image: mismatch (image 2, CLI 3) · built unknown · runner unknown"]
    );
}

#[test]
fn a_build_in_progress_shows_its_elapsed_time_and_stop_the_build() {
    let snapshot = load("building");
    let view = build(&Input::new(
        Connection::Connected,
        Some(&snapshot),
        clock_at(3, 44, 42),
    ));
    assert_eq!(
        view.summary.line,
        "Building the runner image since today 03:43"
    );
    assert_eq!(view.tooltip, "Marshal: building the runner image");
    assert_eq!(view.menu.summary, "Building the runner image");
    assert_eq!(view.icon, IconState::Stopped);
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::Building);
    assert_eq!(banner.heading, "Building the runner image");
    assert_eq!(
        banner.lines,
        [format!(
            "actions-runner:local · 1 min 42 s so far. Docker's output goes to the log in {LOG}."
        )]
    );
    assert_eq!(banner.announcement, "Building the runner image");
    let stop = banner.action.as_ref().expect("Stop the Build");
    assert_eq!(
        (stop.action.clone(), stop.label.as_str()),
        (Action::StopBuild, "Stop the Build")
    );
    assert!(stop.available);
    let question = stop.confirmation.as_ref().expect("a question");
    assert_eq!(question.title, "Stop the build?");
    assert_eq!(
        question.body,
        "A build in progress is discarded. If it has already finished, the new image stays."
    );
    assert_eq!(
        (question.confirm.as_str(), question.cancel.as_str()),
        ("Stop", "Cancel")
    );
}

#[test]
fn a_failed_build_gives_its_time_its_reason_and_build_again() {
    let snapshot = load("build_failed");
    let view = view_of(&snapshot);
    assert_eq!(view.tooltip, "Marshal: stopped, the image build failed");
    assert_eq!(view.icon, IconState::StoppedByError);
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::BuildFailed);
    assert_eq!(banner.heading, "The image build failed");
    assert_eq!(
        banner.lines,
        [
            "The build failed after 2 min 41 s.".to_string(),
            "docker build ended with exit code 1".to_string(),
            format!("Docker's output is in the log in {LOG}."),
        ]
    );
    let again = banner.action.as_ref().expect("Build Again");
    assert_eq!(again.label, "Build Again");
    assert_eq!(again.look, Look::Solid);
    assert_eq!(header(&view, &Action::StartAll).look, Look::Neutral);
    assert_eq!(header(&view, &Action::BuildImage).look, Look::Tonal);
}

#[test]
fn a_finished_build_says_the_image_is_ready_and_a_stopped_one_names_the_image() {
    let ready = view_of(&load("build_ready"));
    assert_eq!(
        first_banner(&ready).lines,
        [
            "The runner image is ready.",
            "They stay stopped until you choose Start All."
        ]
    );
    let cancelled = view_of(&load("build_cancelled"));
    assert_eq!(first_banner(&cancelled).heading, "Runners stopped by you");
    assert_eq!(
        cancelled.window.footer.warnings,
        [
            "The build was stopped. Check the image actions-runner:local: a stopped build may already have replaced it."
        ]
    );
    let unnamed = view_of(&patched("build_cancelled", |value| {
        value["images"][0]["name"] = "".into();
    }));
    assert_eq!(
        unnamed.window.footer.warnings,
        [
            "The build was stopped. Check the runner image: a stopped build may already have replaced it."
        ]
    );
}

#[test]
fn an_image_that_is_out_of_date_while_running_says_to_stop_first() {
    let snapshot = load("image_out_of_date");
    let view = view_of(&snapshot);
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::ImageOutOfDate);
    assert_eq!(banner.heading, "The runner image is out of date");
    assert_eq!(
        banner.lines,
        [
            "the image is 12 days old and should be rebuilt",
            "It still runs jobs. To build it again, stop the runners, then choose Build Image."
        ]
    );
    let build = header(&view, &Action::BuildImage);
    assert_eq!(reason(build), "Unavailable: stop the runners first.");
    assert_eq!(build.look, Look::Neutral);
    assert_eq!(
        view.window.footer.warnings,
        [
            "Warning: the image is 12 days old and should be rebuilt.",
            "Warning: runner 2.338.2 was released 15 days ago, and a newer runner may exist."
        ]
    );
}

#[test]
fn once_stopped_the_out_of_date_image_offers_a_tonal_build_image() {
    let snapshot = load("image_out_of_date_stopped");
    let view = view_of(&snapshot);
    assert_eq!(first_banner(&view).kind, BannerKind::StoppedByYou);
    let build = header(&view, &Action::BuildImage);
    assert!(build.available);
    assert_eq!(build.look, Look::Tonal);
    assert_eq!(header(&view, &Action::StartAll).look, Look::Solid);
}

#[test]
fn old_packages_and_an_image_not_built_by_marshal_are_worded_without_a_command() {
    let snapshot = load("packages_old");
    let view = view_of(&snapshot);
    assert_eq!(
        first_banner(&view).lines[0],
        "the image's operating-system packages are 30 days old and it should be rebuilt"
    );
    assert_eq!(
        view.window.footer.warnings,
        [
            "Warning: the image's operating-system packages are 30 days old; build the image again.",
            "Warning: runner 2.330.0 was released 40 days ago, so each job's runner likely updates itself at start; update Marshal to get a newer runner, then build the image again.",
            "Warning: the image was not built by Marshal, so its build age is unknown."
        ]
    );
    for warning in &view.window.footer.warnings {
        assert!(
            !warning.contains("marshal build") && !warning.contains('`'),
            "{warning}"
        );
    }
}

#[test]
fn waiting_for_docker_says_to_start_docker_desktop() {
    let snapshot = load("waiting_for_docker");
    let view = view_of(&snapshot);
    assert_eq!(view.summary.heading, "0 of 6 runners up");
    assert_eq!(view.summary.line, "Running since today 03:15 · 3 pools");
    assert_eq!(view.tooltip, "Marshal: waiting for Docker");
    assert_eq!(view.icon, IconState::SomeNotUp);
    let banner = first_banner(&view);
    assert_eq!(banner.heading, "Waiting for Docker");
    assert_eq!(
        banner.lines,
        ["Start Docker Desktop. Marshal checks every 5 s."]
    );
    assert_eq!(list(&view).pools[0].line, "Waiting for Docker · 0 of 2 up");
}

#[test]
fn starting_all_runners_is_the_resuming_state() {
    let snapshot = load("resuming");
    let view = view_of(&snapshot);
    assert_eq!(view.summary.line, "Starting since today 03:52 · 3 pools");
    assert_eq!(view.tooltip, "Marshal: starting, 0 of 6 runners up");
    assert_eq!(view.menu.summary, "Starting, 0 of 6 runners up");
    assert_eq!(view.icon, IconState::SomeNotUp);
    let banner = first_banner(&view);
    assert_eq!(banner.heading, "Starting all runners");
    assert_eq!(
        banner.lines,
        ["Reading the configuration and checking the image."]
    );
}

#[test]
fn stopping_says_how_long_and_that_running_jobs_fail() {
    let snapshot = load("stopping");
    let view = view_of(&snapshot);
    assert_eq!(view.summary.line, "Stopping since today 03:52");
    assert_eq!(view.tooltip, "Marshal: stopping, 3 of 6 runners up");
    assert_eq!(view.menu.summary, "Stopping, 3 of 6 runners up");
    assert_eq!(view.icon, IconState::SomeNotUp);
    let banner = first_banner(&view);
    assert_eq!(banner.heading, "Stopping all runners");
    assert_eq!(banner.lines, ["Up to 30 s. Any job running on them fails."]);

    let idle = patched("stopping", |value| {
        for pool in value["pools"].as_array_mut().expect("pools") {
            for runner in pool["runners"].as_array_mut().expect("runners") {
                runner["state"] = "stopped".into();
            }
            pool["runners_up"] = 0.into();
        }
        value["runners_up"] = 0.into();
    });
    assert_eq!(first_banner(&view_of(&idle)).lines, ["Up to 30 s."]);
}

#[test]
fn quitting_comes_from_the_agent_or_the_app() {
    let snapshot = load("quitting");
    let from_agent = view_of(&snapshot);
    assert_eq!(from_agent.tooltip, "Marshal: quitting, 3 of 6 runners up");
    assert_eq!(from_agent.menu.summary, "Quitting, 3 of 6 runners up");
    let banner = first_banner(&from_agent);
    assert_eq!(banner.heading, "Quitting Marshal");
    assert_eq!(
        banner.lines,
        [
            "Stopping all runners first: up to 30 s. Any job running on them fails.",
            "The window closes with the app."
        ]
    );
    let running = load("running");
    let from_app = view_in(Connection::Quitting, Some(&running));
    assert_eq!(from_app.tooltip, "Marshal: quitting, 3 of 6 runners up");
    assert_eq!(first_banner(&from_app).kind, BannerKind::Quitting);
    let without_a_list = view_in(Connection::Quitting, None);
    assert_eq!(full_heading(&without_a_list), "Quitting Marshal");
}

#[test]
fn no_pool_could_start_for_want_of_tokens_says_so() {
    let snapshot = load("stopped_no_pool_started");
    let view = view_of(&snapshot);
    assert_eq!(view.tooltip, "Marshal: stopped, no pool could start");
    assert_eq!(view.icon, IconState::StoppedByError);
    let banner = first_banner(&view);
    assert_eq!(banner.heading, "Stopped: no pool could start");
    assert_eq!(
        banner.lines,
        [
            "Every pool was skipped because its repository has no token. Set a token for each repository, then choose Start All."
        ]
    );
    assert_eq!(
        banner.announcement,
        "Stopped: no pool could start. Every pool was skipped because its repository has no token."
    );
    assert_eq!(
        view.banners().len(),
        1,
        "the token banner would say it twice"
    );
    let list = list(&view);
    assert_eq!(
        list.pools[0].note.as_deref(),
        Some("No token for owner/app. Start All skipped this pool.")
    );
    assert_eq!(
        list.header.tokens_chip.as_deref(),
        Some("3 repositories need attention")
    );
}

#[test]
fn no_pool_could_start_for_another_reason_points_to_the_pools() {
    let snapshot = load("stopped_no_pool_other_reason");
    let view = view_of(&snapshot);
    assert_eq!(
        first_banner(&view).lines,
        ["Each pool below says why. Fix them, then choose Start All."]
    );
    let list = list(&view);
    assert_eq!(
        list.pools[0].note.as_deref(),
        Some(
            "repositories[0]: the token store failed: access was refused (kind access_refused, code 5)."
        ),
        "the card keeps the agent's reason, not \"No token\""
    );
    assert_eq!(list.pools[0].pill, None);
    assert_eq!(
        list.tokens.rows[0].state_text,
        "The token store failed: access was refused (kind access_refused, code 5)."
    );
    assert!(
        view.banners()
            .iter()
            .all(|banner| banner.heading != "No repository has a token yet"),
        "a store that failed does not show that no token is stored"
    );
}

#[test]
fn the_restart_limit_stop_names_the_log_and_start_all() {
    let snapshot = load("stopped_restart_limit");
    let view = view_of(&snapshot);
    assert_eq!(
        view.tooltip,
        "Marshal: stopped, the runtime core failed 4 times"
    );
    assert_eq!(view.icon, IconState::StoppedByError);
    let banner = first_banner(&view);
    assert_eq!(
        banner.heading,
        "Stopped: the runtime core failed 4 times within 10 minutes"
    );
    assert_eq!(
        banner.lines,
        [format!("The log in {LOG} says why. Start All tries again.")]
    );
}

#[test]
fn restarting_after_a_failure_says_when_and_that_no_pools_are_listed() {
    let snapshot = load("restarting");
    let view = view_of(&snapshot);
    assert_eq!(view.summary.heading, "No runners up");
    assert_eq!(view.tooltip, "Marshal: restarting after a failure");
    assert_eq!(view.icon, IconState::SomeNotUp);
    let banner = first_banner(&view);
    assert_eq!(banner.heading, "Restarting at 03:52");
    assert_eq!(
        banner.lines,
        [
            format!(
                "The runtime core failed; it restarts after 10, 30 or 60 s. The log in {LOG} says why."
            ),
            "No pools are listed until it restarts.".to_string()
        ]
    );
    assert!(list(&view).pools.is_empty());
}

#[test]
fn a_container_a_stop_could_not_remove_is_named_in_the_stopped_banner() {
    let snapshot = load("leftover");
    let view = view_of(&snapshot);
    assert_eq!(view.banners().len(), 1);
    let banner = first_banner(&view);
    assert_eq!(
        banner.heading,
        "Runners stopped by you; a container could not be removed"
    );
    assert_eq!(
        banner.lines,
        [
            format!(
                "site-1-20261104033907412 remains; the next start removes it. The log in {LOG} says why."
            ),
            "They stay stopped until you choose Start All.".to_string()
        ]
    );
}

#[test]
fn leftover_containers_while_running_have_a_banner_of_their_own() {
    let snapshot = load("leftover_running");
    let view = view_of(&snapshot);
    assert_eq!(view.banners().len(), 1);
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::ContainerLeft);
    assert_eq!(banner.heading, "Some containers could not be removed");
    assert_eq!(
        banner.lines,
        [format!(
            "site-1-20261104033907412 and site-2-20261104033911000 remain; the next start removes them. The log in {LOG} says why."
        )]
    );
}

#[test]
fn every_runner_backing_off_with_the_image_replaced_says_to_build_it_again() {
    let snapshot = load("every_runner_backing_off_image");
    let view = view_of(&snapshot);
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::EveryRunnerBacksOff);
    assert_eq!(banner.heading, "Every runner is backing off");
    assert_eq!(
        banner.lines,
        [
            "Runner image: mismatch (image 2, CLI 3).",
            "The image was replaced outside Marshal. Stop the runners, then choose Build Image."
        ]
    );
    assert_eq!(list(&view).pools[0].state, "Backing off");
    assert_eq!(list(&view).pools[0].mark, Some(Mark::History));
}

#[test]
fn every_runner_backing_off_otherwise_points_to_the_rows() {
    let snapshot = load("every_runner_backing_off");
    let view = view_of(&snapshot);
    assert_eq!(
        first_banner(&view).lines,
        ["Each runner's reason is on its row below."]
    );
    assert_eq!(
        list(&view).pools[0].runners[0].detail.as_deref(),
        Some("container could not run (docker exited)")
    );
}

#[test]
fn a_snapshot_too_large_to_send_whole_says_how_much_it_leaves_out() {
    let snapshot = load("truncated");
    let view = view_of(&snapshot);
    let banner = view
        .banners()
        .iter()
        .find(|banner| banner.kind == BannerKind::Truncated)
        .expect("a notice");
    assert_eq!(banner.heading, "Part of the list is left out");
    assert_eq!(
        banner.lines,
        [
            "The agent's list was too large to send whole. Not shown: 12 runners, 1 pool and 3 configuration problems."
        ]
    );
}

#[test]
fn lost_contact_keeps_the_last_list_marked_with_its_time() {
    let snapshot = load("running");
    let heard = at(3, 52, 0);
    let view = view_in(
        Connection::LostContact {
            last_heard_ms: heard,
            cause: Cause::RestartFailed,
        },
        Some(&snapshot),
    );
    assert_eq!(view.summary.heading, "3 of 6 runners up");
    assert_eq!(view.summary.line, "Last heard from the agent at 03:52");
    assert_eq!(view.tooltip, "Marshal: lost contact with the agent");
    assert_eq!(view.icon, IconState::NotInControl);
    let list = list(&view);
    assert!(list.stale);
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::LostContact);
    assert_eq!(banner.heading, "Lost contact with the agent");
    assert_eq!(
        banner.lines,
        [format!(
            "The list below is from 03:52 and may be out of date. Marshal started the agent again and it did not answer; the log in {LOG} says why."
        )]
    );
    let again = banner.action.as_ref().expect("Try Again");
    assert_eq!(again.label, "Try Again");
    assert_eq!(again.look, Look::Solid);
    assert!(again.available);
    for pool in &list.pools {
        assert_eq!(pool.from.as_deref(), Some("from 03:52"));
        assert!(pool.mark.is_none());
        assert!(pool.runners.iter().all(|runner| runner.mark.is_none()));
    }
    assert_eq!(list.pools[0].line, "Running · 2 of 2 up · from 03:52");
    assert_eq!(list.pools[0].runners[0].state, "Container running");
}

#[test]
fn lost_contact_offers_try_again_only_after_the_one_restart() {
    let snapshot = load("running");
    let dropped = view_in(
        Connection::LostContact {
            last_heard_ms: at(3, 51, 0),
            cause: Cause::Dropped,
        },
        Some(&snapshot),
    );
    let banner = first_banner(&dropped);
    assert!(banner.action.is_none());
    assert_eq!(
        banner.lines,
        [
            "The list below is from 03:51 and may be out of date. Reconnecting; Marshal starts the agent once."
        ]
    );
    let busy = view_in(
        Connection::LostContact {
            last_heard_ms: at(3, 51, 0),
            cause: Cause::Busy,
        },
        Some(&snapshot),
    );
    let banner = first_banner(&busy);
    assert!(banner.action.is_some());
    assert_eq!(
        banner.lines,
        [
            "The list below is from 03:51 and may be out of date. The agent is busy; Marshal tries again every 5 s."
        ]
    );
}

#[test]
fn a_list_from_an_earlier_day_names_the_day() {
    let snapshot = load("running");
    let view = view_in(
        Connection::LostContact {
            last_heard_ms: at(3, 52, 0) - 86_400_000,
            cause: Cause::Dropped,
        },
        Some(&snapshot),
    );
    assert_eq!(
        view.summary.line,
        "Last heard from the agent at yesterday 03:52"
    );
}

#[test]
fn without_a_list_the_whole_window_says_why_marshal_is_not_in_control() {
    let starting = view_in(Connection::StartingMarshal, None);
    let full = starting.full().expect("a full window");
    assert_eq!(full.heading, "Starting Marshal");
    assert_eq!(full.lines, ["Connecting to the agent."]);
    assert!(full.action.is_none());
    assert_eq!(starting.tooltip, "Marshal: starting");
    assert_eq!(starting.icon, IconState::NotInControl);
    assert_eq!(
        starting.window.footer.image_lines,
        ["Runner image: not checked"]
    );

    let none = view_in(Connection::NoAgent, None);
    let full = none.full().expect("a full window");
    assert_eq!(full.heading, "Marshal cannot reach its agent");
    assert_eq!(
        full.lines,
        [format!(
            "The agent did not start, or stopped answering and did not come back after one restart. The log in {LOG} says why."
        )]
    );
    assert_eq!(full.action.as_ref().expect("Try Again").label, "Try Again");
    assert_eq!(none.tooltip, "Marshal: agent not reachable");

    let silent = view_in(Connection::NotAnswering, None);
    let full = silent.full().expect("a full window");
    assert_eq!(full.heading, "The agent is not answering");
    assert!(full.lines[1].starts_with("End marshal-app.exe in Task Manager"));
    assert_eq!(silent.tooltip, "Marshal: the agent is not answering");

    let locked = view_in(Connection::LockHeld, None);
    let full = locked.full().expect("a full window");
    assert_eq!(full.heading, "Another Marshal is running");
    assert!(full.lines[0].contains("Marshal checks again every 5 s"));
    assert!(full.lines[0].contains("marshal-app.exe"));
    assert!(full.action.is_none());
    assert_eq!(locked.tooltip, "Marshal: another Marshal is running");
    assert_eq!(locked.icon, IconState::NotInControl);
}

#[test]
fn busy_with_no_list_to_keep_is_a_full_window_that_retries() {
    let busy = view_in(
        Connection::LostContact {
            last_heard_ms: 0,
            cause: Cause::Busy,
        },
        None,
    );
    let full = busy.full().expect("a full window");
    assert_eq!(
        full.lines,
        ["The agent is busy. Marshal tries again every 5 s."]
    );
    assert!(full.action.is_some());
    let restart_failed = view_in(
        Connection::LostContact {
            last_heard_ms: 0,
            cause: Cause::RestartFailed,
        },
        None,
    );
    assert_eq!(
        full_heading(&restart_failed),
        "Marshal cannot reach its agent"
    );
    let dropped = view_in(
        Connection::LostContact {
            last_heard_ms: 0,
            cause: Cause::Dropped,
        },
        None,
    );
    assert_eq!(full_heading(&dropped), "Starting Marshal");
}

#[test]
fn a_connected_app_with_no_snapshot_yet_is_starting() {
    assert_eq!(
        full_heading(&view_in(Connection::Connected, None)),
        "Starting Marshal"
    );
}

#[test]
fn full_window_states_announce_their_heading_and_text() {
    let view = view_in(Connection::NotAnswering, None);
    let full = view.full().expect("a full window");
    assert_eq!(
        full.announcement,
        "The agent is not answering. Marshal's agent did not answer within 5 s, and its runners may still be running."
    );
    assert!(matches!(view.window.body, Body::Full(_)));
}

#[test]
fn an_action_not_yet_in_the_snapshot_shows_its_transition_at_once() {
    let running = load("running");
    let pending = Pending {
        kind: PendingKind::StopAll,
        since_ms: at(3, 52, 1),
    };
    let view =
        build(&Input::new(Connection::Connected, Some(&running), now()).pending(Some(pending)));
    assert_eq!(view.summary.line, "Stopping since today 03:52");
    assert_eq!(view.tooltip, "Marshal: stopping, 3 of 6 runners up");
    assert_eq!(view.menu.summary, "Stopping, 3 of 6 runners up");
    assert_eq!(first_banner(&view).heading, "Stopping all runners");
    assert_eq!(
        reason(header(&view, &Action::StopAll)),
        "Unavailable: the runners are stopping."
    );
    assert!(!menu_item(&view, &Action::StopAll).available);

    let stopped = load("stopped_by_you");
    let pending = Pending {
        kind: PendingKind::StartAll,
        since_ms: at(3, 52, 1),
    };
    let view =
        build(&Input::new(Connection::Connected, Some(&stopped), now()).pending(Some(pending)));
    assert_eq!(first_banner(&view).heading, "Starting all runners");
    assert_eq!(view.tooltip, "Marshal: starting, 0 of 6 runners up");

    let pending = Pending {
        kind: PendingKind::Build,
        since_ms: at(3, 52, 0),
    };
    let view = build(
        &Input::new(Connection::Connected, Some(&stopped), clock_at(3, 52, 7))
            .pending(Some(pending)),
    );
    assert_eq!(view.tooltip, "Marshal: building the runner image");
    assert!(first_banner(&view).lines[0].contains("7 s so far"));

    let building = load("building");
    let pending = Pending {
        kind: PendingKind::StopBuild,
        since_ms: at(3, 52, 1),
    };
    let view =
        build(&Input::new(Connection::Connected, Some(&building), now()).pending(Some(pending)));
    assert_eq!(view.tooltip, "Marshal: stopping the build");
    let banner = first_banner(&view);
    assert_eq!(banner.heading, "Stopping the build");
    assert_eq!(banner.lines, ["Up to 30 s."]);
    let stop = banner.action.as_ref().expect("Stop the Build");
    assert_eq!(reason(stop), "Unavailable: the build is stopping.");
}

#[test]
fn an_action_the_snapshot_already_shows_is_not_applied_twice() {
    let stopped = load("stopped_by_you");
    let pending = Pending {
        kind: PendingKind::StopAll,
        since_ms: at(3, 52, 1),
    };
    let view =
        build(&Input::new(Connection::Connected, Some(&stopped), now()).pending(Some(pending)));
    assert_eq!(first_banner(&view).kind, BannerKind::StoppedByYou);
}

#[test]
fn the_window_opens_on_tokens_while_no_repository_has_one() {
    let first = view_of(&load("first_launch"));
    assert_eq!(list(&first).header.opens_on, marshal_view::Tab::Tokens);
    let running = view_of(&load("running"));
    assert_eq!(list(&running).header.opens_on, marshal_view::Tab::Pools);
}

#[test]
fn the_view_serialises_for_the_page() {
    let view = view_of(&load("running"));
    let json = serde_json::to_value(&view).expect("view state is JSON");
    assert_eq!(json["window"]["body"]["kind"], "list");
    assert_eq!(json["icon"], "some_not_up");
    assert_eq!(
        json["window"]["body"]["header"]["heading"],
        "3 of 6 runners up"
    );
}

#[test]
fn a_fresh_install_has_the_image_banner_first_and_the_token_banner_after_it() {
    let view = view_of(&load("first_launch"));
    let kinds: Vec<_> = view.banners().iter().map(|banner| banner.kind).collect();
    assert_eq!(
        kinds,
        [BannerKind::ImageCannotRunJobs, BannerKind::NoTokens]
    );
    let token = &view.banners()[1];
    assert_eq!(token.heading, "No repository has a token yet");
    assert_eq!(
        token.lines,
        ["Set a token for each repository, then choose Start All."]
    );
    assert!(token.action.is_none(), "the token banner has no button");
    assert_eq!(token.tone, Tone::Warning);
    let image = &view.banners()[0];
    assert_eq!(
        image.action.as_ref().map(|action| action.look),
        Some(Look::Solid)
    );
    let list = list(&view);
    assert_eq!(
        list.header.controls.len(),
        3,
        "the first-launch window has three header buttons"
    );
    assert_eq!(header(&view, &Action::StartAll).look, Look::Neutral);
    assert!(header(&view, &Action::StartAll).available);
    assert_eq!(
        reason(header(&view, &Action::StopAll)),
        "Unavailable: the runners are already stopped."
    );
    assert_eq!(view.window.footer.image_lines, ["Runner image: missing"]);
}

#[test]
fn a_stopped_pool_lists_no_runner_rows_and_a_stopped_runner_has_no_since() {
    let snapshot = patched("running", |value| {
        value["pools"][0]["runners"][1]["state"] = "stopped".into();
    });
    let view = view_of(&snapshot);
    let stopped = &list(&view).pools[0].runners[1];
    assert_eq!(stopped.state, "Stopped");
    assert_eq!(stopped.mark, Some(Mark::Stop));
    assert!(stopped.since.is_none());
}
