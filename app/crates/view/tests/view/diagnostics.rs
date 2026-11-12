//! a pool's Diagnostics section: what the snapshot carries of its containers, bundles
//! and findings, in the words of the boards without what the snapshot does not hold (the
//! directory, the bundle names and why a removal failed).

use crate::support::*;

const RETAINED: &str =
    "It stays retained, and is retried at the next Stop All and when the agent next starts.";

#[test]
fn a_pool_with_diagnostics_off_has_no_section() {
    let view = view_of(&load("diagnostics"));
    let pools = &list(&view).pools;
    assert!(pools[1].diagnostics.is_none());
    assert!(pools[2].diagnostics.is_none());
    assert!(pools[1].tags.is_empty());
}

#[test]
fn the_section_says_the_count_the_limit_and_what_is_being_saved() {
    let view = view_of(&load("diagnostics"));
    let app = &list(&view).pools[0];
    assert_eq!(app.tags, ["diagnostics on"]);
    assert_eq!(app.line, "Running · 2 of 2 up · diagnostics on");
    let section = app.diagnostics.as_ref().expect("a section");
    assert_eq!(section.heading, "Diagnostics");
    assert!(section.on);
    assert_eq!(section.summary, "9 saved of 10; 1 being saved");
    assert_eq!(
        section.notes,
        [
            "When it holds 10, later containers are removed without a bundle.",
            "Removed containers stay listed until the next start."
        ]
    );
}

#[test]
fn each_container_says_which_runner_it_came_from_and_how_it_ended() {
    let view = view_of(&load("diagnostics"));
    let section = list(&view).pools[0]
        .diagnostics
        .as_ref()
        .expect("a section");
    let items: Vec<_> = section
        .items
        .iter()
        .map(|item| {
            (
                item.container.as_str(),
                item.from.as_deref(),
                item.text.as_str(),
                item.problem,
            )
        })
        .collect();
    assert_eq!(
        items,
        [
            (
                "app-2-20261104031530871",
                Some("Runner 2"),
                "Saving a bundle, then removing the container.",
                false
            ),
            (
                "app-1-20261104032944518",
                Some("Runner 1"),
                "Saved. Its removal failed. It stays retained, and is retried at the next Stop All and when the agent next starts.",
                true
            ),
            (
                "app-1-20261104031522114",
                Some("Runner 1"),
                "Saved with a gap, because the final observation is missing; removed.",
                true
            ),
            (
                "app-1-20261104030000000",
                Some("Runner 1"),
                "Saved; removed.",
                false
            ),
        ]
    );
    assert!(
        section
            .items
            .iter()
            .all(|item| item.container != "app-1-20261104034410233"),
        "a running container is a runner, not a diagnostic item"
    );
}

#[test]
fn a_problem_is_announced_once_with_its_pool_and_container() {
    let view = view_of(&load("diagnostics"));
    let texts: Vec<_> = view
        .attention
        .iter()
        .map(|item| item.text.as_str())
        .filter(|text| text.starts_with("Pool app:"))
        .collect();
    assert_eq!(
        texts,
        [
            format!("Pool app: app-1-20261104032944518: Saved. Its removal failed. {RETAINED}"),
            "Pool app: app-1-20261104031522114: Saved with a gap, because the final observation is missing; removed.".to_string(),
        ]
    );
}

#[test]
fn an_unknown_count_shows_a_question_mark_and_says_what_it_means() {
    let view = view_of(&load("diagnostics_other_cases"));
    let section = list(&view).pools[0]
        .diagnostics
        .as_ref()
        .expect("a section");
    assert_eq!(section.summary, "? saved of 10");
    assert_eq!(
        section.notes,
        [
            "When it holds 10, later containers are removed without a bundle.",
            "? means the count is not known yet, or it failed.",
            "Removed containers stay listed until the next start."
        ]
    );
}

#[test]
fn the_other_ways_an_export_ends_are_each_worded() {
    let view = view_of(&load("diagnostics_other_cases"));
    let section = list(&view).pools[0]
        .diagnostics
        .as_ref()
        .expect("a section");
    let texts: Vec<_> = section
        .items
        .iter()
        .map(|item| item.text.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            format!("Not saved, because the export failed. Its removal failed. {RETAINED}"),
            "Not saved, because the pool's directory already holds 10 bundles; removed.".to_string(),
            "Saved. A forced stop ended it, so the container is kept; the next start with this pool's diagnostics on exports and removes it."
                .to_string(),
        ]
    );
    assert!(section.items.iter().all(|item| item.problem));
}

#[test]
fn a_kept_container_with_diagnostics_off_is_left_for_the_manual_recovery() {
    let view = view_of(&load("diagnostics_other_cases"));
    let site = list(&view).pools[1]
        .diagnostics
        .as_ref()
        .expect("a section");
    assert!(!site.on);
    assert_eq!(site.summary, "Off for this pool");
    assert!(site.notes.is_empty());
    assert_eq!(site.items.len(), 1);
    assert_eq!(site.items[0].container, "site-1-20261103221408763");
    assert_eq!(site.items[0].from.as_deref(), Some("Runner 1"));
    assert_eq!(
        site.items[0].text,
        "Left for the manual recovery in Windows Runner Host: Marshal has no diagnostics settings to save it with."
    );
    assert!(site.items[0].problem);
}

#[test]
fn a_finding_the_snapshot_no_longer_lists_a_container_for_is_still_shown() {
    let view = view_of(&load("diagnostics"));
    let section = list(&view).pools[0]
        .diagnostics
        .as_ref()
        .expect("a section");
    assert!(
        section
            .items
            .iter()
            .any(|item| item.container == "app-1-20261104030000000")
    );
}

#[test]
fn an_export_result_a_newer_agent_adds_is_shown_by_its_word() {
    let snapshot = patched("diagnostics", |value| {
        value["pools"][0]["diagnostics"]["findings"][0]["export"] = "queued".into();
    });
    let view = view_of(&snapshot);
    let section = list(&view).pools[0]
        .diagnostics
        .as_ref()
        .expect("a section");
    assert!(
        section
            .items
            .iter()
            .any(|item| item.text.starts_with("Export result: queued")),
        "{section:?}"
    );
}

#[test]
fn diagnostics_are_not_shown_in_a_state_without_pools() {
    let view = view_of(&load("restarting"));
    assert!(list(&view).pools.is_empty());
}
