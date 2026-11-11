//! what depends on the time passed in: the settled count, the summary announcement and
//! the glyph's colour, all as pure functions of the clock a test sets.

use marshal_view::{
    Announced, Announcer, Clock, Connection, FALL_DELAY_MS, GlyphColour, IconState, Input, Rgb,
    SUMMARY_INTERVAL_MS, Settled, TaskbarMode, Tracker, build, elapsed,
};

use crate::support::*;

const SECOND: u64 = 1_000;

/// the `all_up` snapshot with `up` of its 3 runners up.
fn with_up(up: u32) -> marshal_cli::agent::snapshot::Snapshot {
    patched("all_up", |value| value["runners_up"] = up.into())
}

fn step(
    tracker: &mut Tracker,
    snapshot: &marshal_cli::agent::snapshot::Snapshot,
    offset_s: u64,
) -> marshal_view::Update {
    let clock = Clock::new(at(3, 52, 0) + offset_s * SECOND, 0);
    tracker.update(Input::new(Connection::Connected, Some(snapshot), clock))
}

#[test]
fn a_fall_in_the_count_shows_after_it_has_lasted_30_s() {
    let mut tracker = Tracker::new();
    let all = with_up(3);
    let two = with_up(2);
    assert_eq!(
        step(&mut tracker, &all, 0).view.summary.heading,
        "All 3 runners up"
    );

    // the window's headline always follows the state at once; the icon, tooltip and menu
    // wait
    let after_10 = step(&mut tracker, &two, 10);
    assert_eq!(after_10.view.summary.heading, "2 of 3 runners up");
    assert_eq!(after_10.view.tooltip, "Marshal: all 3 runners up");
    assert_eq!(after_10.view.menu.summary, "All 3 runners up");
    assert_eq!(after_10.view.icon, IconState::AllUp);

    let before = step(&mut tracker, &two, 10 + 30 - 1);
    assert_eq!(before.view.tooltip, "Marshal: all 3 runners up");

    let settled = step(&mut tracker, &two, 10 + 30);
    assert_eq!(settled.view.tooltip, "Marshal: 2 of 3 runners up");
    assert_eq!(settled.view.menu.summary, "2 of 3 runners up");
    assert_eq!(settled.view.icon, IconState::SomeNotUp);
    assert_eq!(FALL_DELAY_MS, 30 * SECOND);
}

#[test]
fn a_rise_in_the_count_shows_at_once() {
    let mut tracker = Tracker::new();
    let two = with_up(2);
    assert_eq!(
        step(&mut tracker, &two, 0).view.tooltip,
        "Marshal: 2 of 3 runners up"
    );
    let all = with_up(3);
    let rise = step(&mut tracker, &all, 1);
    assert_eq!(rise.view.tooltip, "Marshal: all 3 runners up");
    assert_eq!(rise.view.icon, IconState::AllUp);
}

#[test]
fn a_runner_that_registers_again_between_jobs_never_flips_the_count() {
    let mut tracker = Tracker::new();
    let all = with_up(3);
    let two = with_up(2);
    step(&mut tracker, &all, 0);
    for (offset, snapshot) in [(5, &two), (12, &all), (20, &two), (28, &all), (90, &all)] {
        let update = step(&mut tracker, snapshot, offset);
        assert_eq!(
            update.view.tooltip, "Marshal: all 3 runners up",
            "at {offset} s"
        );
        assert!(update.announced.messages.is_empty(), "at {offset} s");
    }
}

#[test]
fn stopping_and_stopped_show_the_count_at_once() {
    let mut tracker = Tracker::new();
    step(&mut tracker, &with_up(3), 0);
    let stopping = step(&mut tracker, &load("stopping"), 2);
    assert_eq!(
        stopping.view.tooltip,
        "Marshal: stopping, 3 of 6 runners up"
    );
    let stopped = step(&mut tracker, &load("stopped_by_you"), 3);
    assert_eq!(stopped.view.tooltip, "Marshal: stopped by you");
    assert_eq!(stopped.view.summary.heading, "0 of 6 runners up");
}

#[test]
fn the_settled_count_is_a_function_of_the_time_passed_in() {
    let mut settled = Settled::new();
    assert_eq!(settled.observe(true, 4, 0), 4);
    assert_eq!(settled.observe(true, 3, 10 * SECOND), 4);
    assert_eq!(settled.observe(true, 3, 40 * SECOND), 3);
    assert_eq!(settled.observe(true, 5, 41 * SECOND), 5);
    assert_eq!(settled.observe(false, 0, 42 * SECOND), 0);
}

#[test]
fn an_action_changes_the_view_at_once_without_waiting_for_the_snapshot() {
    let running = load("running");
    let mut tracker = Tracker::new();
    let before = step(&mut tracker, &running, 0);
    assert_eq!(before.view.tooltip, "Marshal: 3 of 6 runners up");
    let pending = marshal_view::Pending {
        kind: marshal_view::PendingKind::StopAll,
        since_ms: at(3, 52, 1),
    };
    let clock = Clock::new(at(3, 52, 1), 0);
    let after = tracker
        .update(Input::new(Connection::Connected, Some(&running), clock).pending(Some(pending)));
    assert_eq!(after.view.tooltip, "Marshal: stopping, 3 of 6 runners up");
    assert_eq!(after.view.menu.summary, "Stopping, 3 of 6 runners up");
    assert!(
        after
            .announced
            .messages
            .iter()
            .any(|message| message.starts_with("Stopping all runners")),
        "{:?}",
        after.announced
    );
}

fn announce(
    announcer: &mut Announcer,
    snapshot: &marshal_cli::agent::snapshot::Snapshot,
    up: u32,
    seconds: u64,
) -> Announced {
    let clock = Clock::new(at(3, 52, 0) + seconds * SECOND, 0);
    let view = build(&Input::new(Connection::Connected, Some(snapshot), clock).settled(up));
    announcer.update(&view, clock.now_ms)
}

#[test]
fn the_first_view_is_read_with_the_page_and_not_announced() {
    let mut announcer = Announcer::new();
    let running = load("running");
    assert_eq!(
        announce(&mut announcer, &running, 3, 0),
        Announced::default()
    );
}

#[test]
fn the_summary_is_announced_at_most_once_per_5_s() {
    let snapshot = load("all_up");
    let mut announcer = Announcer::new();
    assert!(
        announce(&mut announcer, &snapshot, 3, 0)
            .messages
            .is_empty()
    );

    let first = announce(&mut announcer, &snapshot, 2, 1);
    assert_eq!(first.messages, ["2 of 3 runners up"]);
    assert_eq!(first.recheck_at_ms, None);

    let held = announce(&mut announcer, &snapshot, 1, 3);
    assert!(held.messages.is_empty(), "{held:?}");
    assert_eq!(held.recheck_at_ms, Some(at(3, 52, 1) + SUMMARY_INTERVAL_MS));

    let still_held = announce(&mut announcer, &snapshot, 1, 5);
    assert!(still_held.messages.is_empty());
    assert_eq!(
        still_held.recheck_at_ms,
        Some(at(3, 52, 1) + SUMMARY_INTERVAL_MS)
    );

    let due = announce(&mut announcer, &snapshot, 1, 6);
    assert_eq!(due.messages, ["1 of 3 runners up"]);
    assert_eq!(due.recheck_at_ms, None);

    let unchanged = announce(&mut announcer, &snapshot, 1, 60);
    assert_eq!(unchanged, Announced::default());
}

#[test]
fn a_held_summary_says_the_latest_count_not_the_one_it_was_held_at() {
    let snapshot = load("all_up");
    let mut announcer = Announcer::new();
    announce(&mut announcer, &snapshot, 3, 0);
    announce(&mut announcer, &snapshot, 2, 1);
    announce(&mut announcer, &snapshot, 1, 2);
    announce(&mut announcer, &snapshot, 0, 3);
    let due = announce(&mut announcer, &snapshot, 0, 7);
    assert_eq!(due.messages, ["0 of 3 runners up"]);
}

#[test]
fn a_summary_that_is_back_at_what_was_last_said_is_not_said_again() {
    let snapshot = load("all_up");
    let mut announcer = Announcer::new();
    announce(&mut announcer, &snapshot, 3, 0);
    announce(&mut announcer, &snapshot, 2, 1);
    announce(&mut announcer, &snapshot, 3, 2);
    let after = announce(&mut announcer, &snapshot, 2, 20);
    assert_eq!(after, Announced::default());
}

#[test]
fn a_banner_is_announced_when_it_appears_and_not_again_while_it_shows() {
    let mut announcer = Announcer::new();
    let running = load("running");
    announce(&mut announcer, &running, 3, 0);

    let stopped = load("stopped_by_you");
    let appears = announce(&mut announcer, &stopped, 0, 1);
    assert_eq!(
        appears.messages,
        ["Runners stopped by you"],
        "the banner says it, so the summary does not"
    );
    let shows = announce(&mut announcer, &stopped, 0, 30);
    assert!(shows.messages.is_empty());

    announce(&mut announcer, &running, 3, 40);
    let again = announce(&mut announcer, &stopped, 0, 50);
    assert_eq!(
        again.messages,
        ["Runners stopped by you"],
        "a banner that came back is news again"
    );
}

#[test]
fn a_banner_whose_words_change_with_time_is_still_one_banner() {
    let mut announcer = Announcer::new();
    let stopped = load("stopped_by_you");
    announce(&mut announcer, &stopped, 0, 0);
    let building = load("building");
    let first = announce(&mut announcer, &building, 0, 1);
    assert_eq!(first.messages, ["Building the runner image"]);
    let later = announce(&mut announcer, &building, 0, 11);
    assert!(later.messages.is_empty(), "{later:?}");
}

#[test]
fn a_full_window_state_is_announced_with_its_heading_and_text() {
    let mut announcer = Announcer::new();
    let view = |connection| build(&Input::new(connection, None, now()));
    announcer.update(&view(Connection::StartingMarshal), at(3, 52, 0));
    let spoken = announcer.update(&view(Connection::NotAnswering), at(3, 52, 5));
    assert_eq!(
        spoken.messages,
        [
            "The agent is not answering. Marshal's agent did not answer within 5 s, and its runners may still be running."
        ]
    );
}

#[test]
fn something_that_needs_the_eye_is_announced_when_it_starts() {
    let mut announcer = Announcer::new();
    let running = load("running");
    announce(&mut announcer, &running, 3, 0);
    let rejected = load("token_rejected");
    let spoken = announce(&mut announcer, &rejected, 3, 1);
    assert!(
        spoken.messages.contains(
            &"Pool app: GitHub rejected the token for owner/app: Bad credentials.".to_string()
        ),
        "{spoken:?}"
    );
    let repeated = announce(&mut announcer, &rejected, 3, 2);
    assert!(repeated.messages.is_empty());
}

#[test]
fn a_time_is_shown_in_the_users_own_hour_and_day() {
    let clock = Clock::new(at(3, 52, 0), 9 * 60);
    assert_eq!(clock.when(at(3, 15, 22)), "12:15");
    assert_eq!(clock.when_with_day(at(3, 15, 22)), "today 12:15");
    assert_eq!(
        clock.when_with_day(at(3, 15, 22) - 86_400_000),
        "yesterday 12:15"
    );
    let west = Clock::new(at(3, 52, 0), -5 * 60);
    assert_eq!(west.when_with_day(at(3, 15, 22)), "today 22:15");
    assert_eq!(
        west.when_with_day(at(3, 15, 22) - 24 * 3_600_000),
        "yesterday 22:15"
    );
}

#[test]
fn an_elapsed_time_is_in_whole_seconds_then_minutes() {
    assert_eq!(elapsed(7 * SECOND), "7 s");
    assert_eq!(elapsed(102 * SECOND), "1 min 42 s");
    assert_eq!(elapsed(161 * SECOND), "2 min 41 s");
}

#[test]
fn the_glyph_is_white_on_a_dark_taskbar_and_near_black_on_a_light_one() {
    assert_eq!(
        GlyphColour::for_mode(Some(TaskbarMode::Dark)),
        GlyphColour::White
    );
    assert_eq!(
        GlyphColour::for_mode(Some(TaskbarMode::Light)),
        GlyphColour::NearBlack
    );
    assert_eq!(GlyphColour::White.rgb(), Rgb(0xff, 0xff, 0xff));
    assert_eq!(GlyphColour::NearBlack.rgb(), Rgb(0x1f, 0x1f, 0x1f));
}

#[test]
fn the_glyph_is_white_when_the_mode_is_absent_or_cannot_be_read() {
    assert_eq!(GlyphColour::for_mode(None), GlyphColour::White);
    for value in [None, Some(2), Some(7), Some(u32::MAX)] {
        assert_eq!(TaskbarMode::from_registry(value), None, "{value:?}");
        assert_eq!(
            GlyphColour::for_mode(TaskbarMode::from_registry(value)),
            GlyphColour::White
        );
    }
}

#[test]
fn the_taskbar_mode_comes_from_the_registrys_system_uses_light_theme() {
    assert_eq!(TaskbarMode::from_registry(Some(0)), Some(TaskbarMode::Dark));
    assert_eq!(
        TaskbarMode::from_registry(Some(1)),
        Some(TaskbarMode::Light)
    );
}

#[test]
fn a_near_black_glyph_is_legible_on_a_light_taskbar() {
    // the contrast of near-black on the lightest taskbar and of white on the darkest, as
    // WCAG defines it, is far above the 3:1 an icon needs
    fn luminance(Rgb(r, g, b): Rgb) -> f64 {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }
    let contrast = |a: Rgb, b: Rgb| {
        let (hi, lo) = (
            luminance(a).max(luminance(b)),
            luminance(a).min(luminance(b)),
        );
        (hi + 0.05) / (lo + 0.05)
    };
    assert!(contrast(GlyphColour::NEAR_BLACK, Rgb(0xf3, 0xf3, 0xf3)) > 3.0);
    assert!(contrast(GlyphColour::WHITE, Rgb(0x20, 0x20, 0x20)) > 3.0);
}
