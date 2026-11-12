//! GitHub status in the view, from the recorded `running` snapshot and a recorded
//! `github` value: each runner's GitHub state in words, the checking-before-not-listed
//! rule, the job and its Open run, the pool notices, the budget, the pause, the summary's
//! running jobs, and the announcements, queued and said once each.

use marshal_cli::github_status::GithubStatus;
use marshal_view::{BannerKind, Connection, GithubFeed, HIDDEN_NAMES, Input, Tracker, View, build};
use serde_json::Value;

use crate::support::{at, clock_at, list, load, load_json, now};

/// when the window opened in these tests: before the recorded check at 03:51:40.
const OPENED: (u64, u64, u64) = (3, 50, 0);

fn github_of(change: impl FnOnce(&mut Value)) -> GithubStatus {
    let mut value = load_json("github/running");
    change(&mut value);
    serde_json::from_value(value).expect("a github value")
}

fn live(status: &GithubStatus) -> GithubFeed<'_> {
    GithubFeed::Live {
        opened_ms: at(OPENED.0, OPENED.1, OPENED.2),
        status: Some(status),
    }
}

fn view_with(status: &GithubStatus) -> View {
    let snapshot = load("running");
    build(&Input::new(Connection::Connected, Some(&snapshot), now()).github(live(status)))
}

/// each runner row's container and GitHub words, in list order.
fn states(view: &View) -> Vec<(Option<String>, Option<String>)> {
    list(view)
        .pools
        .iter()
        .flat_map(|card| &card.runners)
        .map(|row| (row.container.clone(), row.github.clone()))
        .collect()
}

fn pair(container: Option<&str>, github: Option<&str>) -> (Option<String>, Option<String>) {
    (container.map(str::to_string), github.map(str::to_string))
}

fn notes(view: &View, pool: usize) -> Vec<String> {
    list(view).pools[pool]
        .github_notes
        .iter()
        .map(|note| match &note.line {
            Some(line) => format!("{} | {line}", note.text),
            None => note.text.clone(),
        })
        .collect()
}

#[test]
fn each_registered_runner_shows_githubs_state_in_words_beside_its_local_state() {
    let status = github_of(|_| {});
    let view = view_with(&status);
    assert_eq!(
        states(&view),
        [
            pair(
                Some("app-1-20261104031522114"),
                Some("GitHub: running a job")
            ),
            pair(Some("app-2-20261104031530871"), Some("GitHub: idle")),
            pair(
                Some("site-1-20261104033907412"),
                Some("GitHub: running a job")
            ),
            // registering and backing off: no registration, so no GitHub state
            pair(None, None),
            pair(None, None),
        ]
    );
    let row = &list(&view).pools[0].runners[0];
    assert_eq!(
        row.state, "Container running",
        "the local state stays first"
    );
    assert_eq!(
        row.label,
        "app, Runner 1, container running, GitHub: running a job"
    );
    assert_eq!(row.job.as_deref(), Some("CI · test (ubuntu-latest)"));
    let open = row.open_run.as_ref().unwrap();
    assert_eq!(open.label, "Open run");
    assert_eq!(
        open.name,
        "Open run of CI · test (ubuntu-latest), app, Runner 1, on GitHub"
    );
    assert_eq!(open.runner, "app-1-20261104031522114");
    // a busy runner whose job is not known yet shows no name and no link
    let site = &list(&view).pools[1].runners[0];
    assert_eq!(site.job, None);
    assert_eq!(site.open_run, None);
}

#[test]
fn offline_not_listed_and_a_word_this_reader_does_not_know_which_reads_as_checking() {
    let status = github_of(|value| {
        let runners = &mut value["repositories"][0]["runners"];
        runners[0]["github"] = "offline".into();
        runners[0]["job"] = Value::Null;
        runners[1]["github"] = "draining".into();
        value["repositories"][1]["runners"] = Value::Array(Vec::new());
    });
    let view = view_with(&status);
    assert_eq!(
        states(&view)[..3],
        [
            pair(Some("app-1-20261104031522114"), Some("GitHub: offline")),
            pair(Some("app-2-20261104031530871"), Some("GitHub: checking…")),
            pair(Some("site-1-20261104033907412"), Some("GitHub: not listed")),
        ]
    );
}

#[test]
fn a_runner_that_entered_its_state_after_the_last_check_reads_as_checking_not_as_not_listed() {
    let status = github_of(|value| {
        value["repositories"][1]["runners"] = Value::Array(Vec::new());
    });
    // the site runner's state began at 03:52:00, after the check at 03:51:40
    let snapshot = {
        let mut value = load_json("running");
        value["pools"][1]["runners"][0]["since"] = at(3, 52, 0).into();
        serde_json::from_value(value).unwrap()
    };
    let view = build(
        &Input::new(Connection::Connected, Some(&snapshot), clock_at(3, 52, 5))
            .github(live(&status)),
    );
    assert_eq!(
        states(&view)[2],
        pair(Some("site-1-20261104033907412"), Some("GitHub: checking…"))
    );
}

#[test]
fn a_check_from_before_the_window_opened_reads_as_checking() {
    let status = github_of(|_| {});
    let snapshot = load("running");
    let view = build(
        &Input::new(Connection::Connected, Some(&snapshot), now()).github(GithubFeed::Live {
            opened_ms: at(3, 51, 50),
            status: Some(&status),
        }),
    );
    assert!(
        states(&view)
            .iter()
            .filter_map(|(_, github)| github.as_deref())
            .all(|github| github == "GitHub: checking…")
    );
    assert_eq!(
        view.window.footer.github[0],
        "GitHub requests: 4,210 of 5,000 left this hour · resets 04:05 · checked every 15 s · as of 03:51"
    );
}

#[test]
fn a_window_opened_during_a_pause_or_while_github_cannot_answer_keeps_the_last_values() {
    let paused = github_of(|value| {
        value["budget"]["level"] = "paused".into();
    });
    let unreachable = github_of(|value| {
        value["repositories"][0]["status"] = "unreachable".into();
    });
    let snapshot = load("running");
    let reopened = |status| {
        build(
            &Input::new(Connection::Connected, Some(&snapshot), now()).github(GithubFeed::Live {
                opened_ms: at(3, 51, 50),
                status: Some(status),
            }),
        )
    };
    for status in [&paused, &unreachable] {
        let view = reopened(status);
        let row = &list(&view).pools[0].runners[0];
        assert_eq!(row.github.as_deref(), Some("GitHub: running a job"));
        assert_eq!(row.github_checked.as_deref(), Some("checked 03:51"));
        assert_eq!(row.job.as_deref(), Some("CI · test (ubuntu-latest)"));
    }
    assert!(reopened(&paused).summary.spoken.ends_with("2 running jobs"));
}

#[test]
fn runners_an_oversize_value_left_out_are_marked_not_shown_and_counted() {
    let status = github_of(|value| {
        value["repositories"][1]["runners"] = Value::Array(Vec::new());
        value["omitted_runners"] = 1.into();
    });
    let view = view_with(&status);
    assert_eq!(
        states(&view)[2],
        pair(Some("site-1-20261104033907412"), Some("GitHub: not shown"))
    );
    assert_eq!(
        view.window.footer.github[1],
        "GitHub's state cannot be shown for 1 runner: the agent's report is too large."
    );
    let two = github_of(|value| value["omitted_runners"] = 2.into());
    assert_eq!(
        view_with(&two).window.footer.github[1],
        "GitHub's state cannot be shown for 2 runners: the agent's report is too large."
    );
}

#[test]
fn a_long_or_hostile_job_name_is_plain_text_cut_to_120_code_points() {
    let long = "x".repeat(130);
    let status = github_of(|value| {
        let job = &mut value["repositories"][0]["runners"][0]["job"];
        job["workflow"] = "C\u{202E}I\u{0007}".into();
        job["name"] = long.clone().into();
    });
    let view = view_with(&status);
    let job = list(&view).pools[0].runners[0].job.clone().unwrap();
    let (workflow, name) = job.split_once(" · ").unwrap();
    assert_eq!(workflow, "CI");
    assert_eq!(name.chars().count(), 120);
    assert!(name.ends_with('…'));
}

#[test]
fn hidden_and_unreadable_job_names_are_said_on_the_pools_card() {
    let status = github_of(|value| {
        value["repositories"][0]["job_names"] = "hidden".into();
        value["repositories"][1]["job_names"] = "unreadable".into();
        value["repositories"][1]["job_names_cause"] = "not_found".into();
        value["repositories"][1]["job_names_retry_at"] = at(3, 56, 40).into();
    });
    let view = view_with(&status);
    assert_eq!(notes(&view, 0), [HIDDEN_NAMES]);
    assert_eq!(
        HIDDEN_NAMES,
        "Job names hidden: this repository's token has no Actions: read. Windows Runner Host says how to add it."
    );
    assert_eq!(
        notes(&view, 1),
        [
            "Job names could not be read: GitHub cannot find this repository with its token. | Last checked 03:51 · trying again at 03:56."
        ]
    );
    // the busy runners still read as running a job
    assert_eq!(states(&view)[0].1.as_deref(), Some("GitHub: running a job"));
}

#[test]
fn each_repository_cause_is_said_with_when_marshal_tries_again_and_the_states_keep_their_time() {
    for (word, text) in [
        (
            "token_rejected",
            "GitHub rejected this repository's token. Replace it on the Tokens tab.",
        ),
        (
            "not_found",
            "GitHub cannot find this repository with its token.",
        ),
        ("moved", "This repository has moved on GitHub."),
        ("unreachable", "GitHub could not be reached."),
        ("error", "GitHub answered with an error."),
        (
            "token_unreadable",
            "This repository's token cannot be read.",
        ),
    ] {
        let status = github_of(|value| {
            value["repositories"][0]["status"] = word.into();
            value["repositories"][0]["retry_at"] = at(3, 56, 0).into();
        });
        let view = view_with(&status);
        assert_eq!(
            notes(&view, 0),
            [format!(
                "{text} | Last checked 03:51 · trying again at 03:56."
            )],
            "{word}"
        );
        let row = &list(&view).pools[0].runners[0];
        assert_eq!(
            row.github.as_deref(),
            Some("GitHub: running a job"),
            "{word}"
        );
        assert_eq!(
            row.github_checked.as_deref(),
            Some("checked 03:51"),
            "{word}"
        );
    }
}

#[test]
fn a_token_cause_w1s_card_already_gives_is_not_said_twice() {
    // GitHub rejected the app pool's token at registration: W1's card says so, with
    // Replace Token
    let snapshot = load("token_rejected");
    let status = github_of(|value| {
        value["repositories"][0]["status"] = "token_rejected".into();
        value["repositories"][1]["status"] = "token_rejected".into();
    });
    let view =
        build(&Input::new(Connection::Connected, Some(&snapshot), now()).github(live(&status)));
    let app = &list(&view).pools[0];
    assert!(app.pill.is_some(), "W1's card gives the reason");
    assert!(app.github_notes.is_empty(), "{:?}", app.github_notes);
    // a pool whose card says nothing of its token gets W2's words
    assert_eq!(
        notes(&view, 1),
        [
            "GitHub rejected this repository's token. Replace it on the Tokens tab. | Last checked 03:51."
        ]
    );
}

#[test]
fn a_pool_without_a_stored_token_gets_no_github_state_or_notice() {
    let status = github_of(|value| {
        value["repositories"][0]["repository"] = "owner/docs".into();
        value["repositories"][0]["status"] = "error".into();
    });
    let view = view_with(&status);
    let docs = &list(&view).pools[2];
    assert!(docs.github_notes.is_empty());
    assert!(docs.runners.iter().all(|row| row.github.is_none()));
}

#[test]
fn the_budget_is_the_footers_first_line_and_its_words_follow_the_level() {
    let snapshot = load("running");
    let unknown = build(
        &Input::new(Connection::Connected, Some(&snapshot), now()).github(GithubFeed::Live {
            opened_ms: at(3, 50, 0),
            status: None,
        }),
    );
    assert_eq!(
        unknown.window.footer.github,
        ["GitHub requests: not known yet"]
    );

    let normal = view_with(&github_of(|_| {}));
    assert_eq!(
        normal.window.footer.github,
        ["GitHub requests: 4,210 of 5,000 left this hour · resets 04:05 · checked every 15 s"]
    );

    let slowed = view_with(&github_of(|value| {
        value["budget"]["remaining"] = 1_200.into();
        value["budget"]["level"] = "slowed".into();
    }));
    assert_eq!(
        slowed.window.footer.github,
        [
            "GitHub requests: 1,200 of 5,000 left this hour · resets 04:05 · checks slowed to once a minute to save the budget"
        ]
    );
    assert!(
        slowed
            .banners()
            .iter()
            .all(|banner| banner.kind != BannerKind::GithubPaused)
    );
}

#[test]
fn a_pause_is_a_banner_and_the_states_keep_the_last_checks_time() {
    let view = view_with(&github_of(|value| {
        value["budget"]["remaining"] = 410.into();
        value["budget"]["level"] = "paused".into();
    }));
    let banner = view
        .banners()
        .iter()
        .find(|banner| banner.kind == BannerKind::GithubPaused)
        .unwrap();
    assert_eq!(banner.heading, "GitHub checks paused until 04:05");
    assert_eq!(
        banner.lines,
        ["410 of 5,000 requests left this hour. Runner states on this computer still update."]
    );
    assert_eq!(
        banner.announcement,
        "GitHub checks paused until 04:05: 410 of 5,000 requests left this hour. Runner states on this computer still update."
    );
    let row = &list(&view).pools[0].runners[0];
    assert_eq!(row.github_checked.as_deref(), Some("checked 03:51"));
}

#[test]
fn the_summary_line_counts_running_jobs_and_the_menu_and_tooltip_do_not() {
    let snapshot = load("running");
    let without = build(&Input::new(Connection::Connected, Some(&snapshot), now()));
    let status = github_of(|_| {});
    let with = view_with(&status);
    assert_eq!(
        list(&with).header.line,
        format!("{} · 2 running jobs", list(&without).header.line)
    );
    assert_eq!(
        with.summary.spoken,
        format!("{} · 2 running jobs", with.summary.menu_line)
    );
    assert_eq!(with.menu, without.menu);
    assert_eq!(with.tooltip, without.tooltip);
    assert_eq!(with.icon, without.icon);
    let one = view_with(&github_of(|value| {
        value["repositories"][1]["runners"][0]["github"] = "idle".into();
    }));
    assert!(list(&one).header.line.ends_with(" · 1 running job"));
}

#[test]
fn github_status_unavailable_is_said_in_the_footer_and_no_state_shows() {
    let snapshot = load("running");
    for (feed, text) in [
        (
            GithubFeed::Unavailable,
            "GitHub status unavailable: the running agent does not report it.",
        ),
        (
            GithubFeed::UnavailableForNow,
            "GitHub status unavailable for now: the agent has no room for another subscription. Asking again every 60 s.",
        ),
    ] {
        let view = build(&Input::new(Connection::Connected, Some(&snapshot), now()).github(feed));
        assert_eq!(view.window.footer.github, [text]);
        assert!(states(&view).iter().all(|(_, github)| github.is_none()));
    }
    let off = build(&Input::new(Connection::Connected, Some(&snapshot), now()));
    assert!(off.window.footer.github.is_empty());
}

#[test]
fn after_contact_is_lost_the_states_stay_and_open_run_is_not_offered() {
    let snapshot = load("running");
    let status = github_of(|_| {});
    let view = build(
        &Input::new(
            Connection::LostContact {
                last_heard_ms: at(3, 51, 50),
                cause: marshal_view::Cause::Dropped,
            },
            Some(&snapshot),
            now(),
        )
        .github(live(&status)),
    );
    assert!(list(&view).stale);
    let row = &list(&view).pools[0].runners[0];
    assert_eq!(row.github.as_deref(), Some("GitHub: running a job"));
    assert!(row.open_run.is_none());
}

#[test]
fn the_one_time_announcements_are_said_once_each_in_order_at_least_5_s_apart() {
    let snapshot = load("running");
    let mut tracker = Tracker::new();
    let normal = github_of(|_| {});
    let mut say = |status: &GithubStatus, clock| {
        let feed = GithubFeed::Live {
            opened_ms: at(3, 50, 0),
            status: Some(status),
        };
        tracker
            .update(Input::new(Connection::Connected, Some(&snapshot), clock).github(feed))
            .announced
    };
    // the first view is read with the page
    assert!(say(&normal, clock_at(3, 52, 0)).messages.is_empty());

    let worse = github_of(|value| {
        value["budget"]["level"] = "slowed".into();
        value["repositories"][0]["job_names"] = "hidden".into();
        value["repositories"][1]["status"] = "unreachable".into();
    });
    let first = say(&worse, clock_at(3, 52, 1));
    assert_eq!(
        first.messages,
        ["GitHub requests: checks slowed to once a minute to save the budget"]
    );
    assert_eq!(first.recheck_at_ms, Some(at(3, 52, 6)));
    assert!(
        say(&worse, clock_at(3, 52, 4)).messages.is_empty(),
        "not before 5 s"
    );
    assert_eq!(
        say(&worse, clock_at(3, 52, 6)).messages,
        [format!("Pool app: {HIDDEN_NAMES}")]
    );
    assert_eq!(
        say(&worse, clock_at(3, 52, 11)).messages,
        ["owner/site: GitHub could not be reached."]
    );
    assert!(
        say(&worse, clock_at(3, 53, 0)).messages.is_empty(),
        "each is said once"
    );

    let paused = github_of(|value| {
        value["budget"]["level"] = "paused".into();
        value["repositories"][0]["job_names"] = "hidden".into();
        value["repositories"][1]["status"] = "unreachable".into();
    });
    assert_eq!(
        say(&paused, clock_at(3, 53, 1)).messages,
        [
            "GitHub checks paused until 04:05: 4,210 of 5,000 requests left this hour. Runner states on this computer still update."
        ],
        "a pause is said by its banner, at once"
    );
    assert_eq!(
        say(&normal, clock_at(4, 5, 30)).messages,
        ["GitHub requests: checked every 15 s"]
    );
}

#[test]
fn a_cause_this_reader_does_not_know_gives_no_notice() {
    let status = github_of(|value| {
        value["repositories"][0]["status"] = "a_cause_added_later".into();
        value["repositories"][1]["job_names"] = "unreadable".into();
        value["repositories"][1]["job_names_cause"] = "another_new_cause".into();
    });
    let view = view_with(&status);
    assert!(notes(&view, 0).is_empty(), "{:?}", notes(&view, 0));
    assert_eq!(list(&view).pools[0].runners[0].github_checked, None);
    assert_eq!(
        notes(&view, 1),
        ["Job names could not be read: GitHub answered with an error. | Last checked 03:51."]
    );
}
