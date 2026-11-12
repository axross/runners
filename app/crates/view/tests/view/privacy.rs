//! text from the configuration, Docker or the agent stays plain text, and no token is in
//! any view state, whatever the agent was sent. The token-shaped words are built here
//! from pieces, so no token-shaped literal is in any file.

use marshal_view::{
    Cause, Connection, Input, NotSaved, TokenResult, TokenUi, Tracker, plain, pools_started_by,
};
use serde_json::Value;

use crate::support::*;

const PREFIXES: [&str; 6] = ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"];

/// a word shaped like a token of GitHub's, with `prefix`: the prefix and 36 of one letter.
fn token_like(prefix: &str) -> String {
    format!("{prefix}{}", "Q".repeat(36))
}

fn has_token_shape(text: &str) -> bool {
    PREFIXES.iter().any(|prefix| {
        text.match_indices(prefix).any(|(index, _)| {
            let before = text.get(..index).and_then(|head| head.chars().next_back());
            let word = !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            let body = text.get(index + prefix.len()..).unwrap_or_default();
            let run = body
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .count();
            word && run >= 16
        })
    })
}

#[test]
fn markup_in_snapshot_text_comes_through_as_the_same_text() {
    let snapshot = load("markup");
    let view = view_of(&snapshot);
    let list = list(&view);
    let pool = &list.pools[0];
    assert_eq!(pool.name, "<b>app</b>");
    assert_eq!(
        pool.runners[0].detail.as_deref(),
        Some("<img src=x onerror=alert(1)> failed & more")
    );
    assert_eq!(pool.runners[0].label, "<b>app</b>, Runner 1, backing off");
    assert_eq!(
        view.window.footer.configuration,
        r"Configuration: C:\Users\<i>user</i>\config.json"
    );
    assert_eq!(list.tokens.rows[0].pools, "Used by pool <b>app</b>");
    assert!(
        view.window.footer.image_lines[0].contains("<b>1 day ago</b>"),
        "{:?}",
        view.window.footer.image_lines
    );
}

#[test]
fn markup_in_a_configuration_problem_and_a_reason_is_text_too() {
    let snapshot = load("markup_invalid_configuration");
    let view = view_of(&snapshot);
    let banner = first_banner(&view);
    assert_eq!(
        banner.lines[0],
        "repositories[0].name: <script>alert(1)</script>"
    );
    assert_eq!(
        banner.lines[1],
        r"Fix C:\Users\<i>user</i>\config.json, then choose Start All."
    );
}

#[test]
fn nothing_the_view_says_is_escaped_or_stripped() {
    // the page sets text, so an `&` is an `&` and a `<` is a `<`: escaping here would
    // show the user a literal `&amp;`
    let view = view_of(&load("markup"));
    let json = serde_json::to_string(&view).expect("view state is JSON");
    assert!(!json.contains("&amp;") && !json.contains("&lt;"));
    assert!(json.contains("failed & more"));
}

#[test]
fn a_token_shaped_word_in_a_snapshots_text_is_replaced() {
    for prefix in PREFIXES {
        let word = token_like(prefix);
        let snapshot = patched("markup", |value| {
            value["pools"][0]["runners"][0]["reason"] = format!("failed with {word} & more").into();
        });
        let view = view_of(&snapshot);
        let detail = list(&view).pools[0].runners[0]
            .detail
            .clone()
            .expect("a detail");
        assert_eq!(detail, "failed with <token> & more", "{prefix}");
    }
}

#[test]
fn plain_changes_nothing_but_a_token_shaped_word() {
    assert_eq!(
        plain("<b>x</b> & \u{201c}y\u{201d}"),
        "<b>x</b> & \u{201c}y\u{201d}"
    );
    assert_eq!(plain(&format!("a {} b", token_like("ghp_"))), "a <token> b");
    assert_eq!(
        plain("ghp_short"),
        "ghp_short",
        "a prefix alone is not a token"
    );
    assert_eq!(
        plain("xghp_QQQQQQQQQQQQQQQQQQQQ"),
        "xghp_QQQQQQQQQQQQQQQQQQQQ",
        "not at a word's start"
    );
    assert_eq!(
        plain(&format!("{0} {0}", token_like("github_pat_"))),
        "<token> <token>"
    );
}

/// the members whose values are codes the view matches, not text it shows.
const CODES: [&str; 5] = ["state", "code", "outcome", "export", "kind"];

/// the words the view matches in a text member: a pool skipped for want of a token, and
/// what the image check found.
const CODE_WORDS: [&str; 3] = ["no token", "missing", "not checked"];

/// every text of `value` followed by a token-shaped word, and GitHub's message inside a
/// runner's reason given one too; codes and the image lines' names are left as they are,
/// so each fixture keeps the state that shows its text.
fn tainted(value: &mut Value, word: &str) {
    match value {
        Value::String(text) if !CODE_WORDS.contains(&text.as_str()) => {
            *text = text.replace("\"message\":\"", &format!("\"message\":\"{word} "));
            text.push_str(&format!(" {word}"));
        }
        Value::Array(items) => items.iter_mut().for_each(|item| tainted(item, word)),
        Value::Object(map) => {
            let image_line = map.contains_key("value");
            for (key, item) in map.iter_mut() {
                if !CODES.contains(&key.as_str()) && !(image_line && key == "name") {
                    tainted(item, word);
                }
            }
        }
        _ => {}
    }
}

const FIXTURES: [&str; 39] = [
    "all_up",
    "build_cancelled",
    "build_failed",
    "build_ready",
    "building",
    "diagnostics",
    "diagnostics_other_cases",
    "every_runner_backing_off",
    "every_runner_backing_off_image",
    "first_launch",
    "held",
    "image_out_of_date",
    "image_out_of_date_stopped",
    "leftover",
    "leftover_running",
    "markup",
    "markup_invalid_configuration",
    "newer_agent",
    "newer_agent_stopped",
    "packages_old",
    "quitting",
    "restarting",
    "resuming",
    "running",
    "shared_repository",
    "stopped_by_you",
    "stopped_image_mismatch",
    "stopped_image_missing",
    "stopped_invalid_configuration",
    "stopped_no_configuration",
    "stopped_no_pool_other_reason",
    "stopped_no_pool_started",
    "stopped_restart_limit",
    "stopping",
    "token_rejected",
    "token_removed_runners_up",
    "truncated",
    "unknown_agent_state",
    "waiting_for_docker",
];

fn connections() -> [Connection; 8] {
    let lost = |cause| Connection::LostContact {
        last_heard_ms: at(3, 50, 0),
        cause,
    };
    [
        Connection::Connected,
        Connection::StartingMarshal,
        Connection::NoAgent,
        Connection::NotAnswering,
        Connection::LockHeld,
        Connection::Quitting,
        lost(Cause::Dropped),
        lost(Cause::Busy),
    ]
}

#[test]
fn the_fixture_list_is_every_fixture() {
    let mut on_disk: Vec<String> =
        std::fs::read_dir(format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR")))
            .expect("the fixtures")
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".json").map(str::to_string)
            })
            .collect();
    on_disk.sort();
    assert_eq!(on_disk, FIXTURES);
}

#[test]
fn no_token_is_in_any_view_state_whatever_text_the_agent_sends() {
    let token = token_like("ghp_");
    // the rows are keyed by the repository as the view shows it, tainted and plain, so
    // the form, the removal and the results below reach their rows
    let key = |repository: &str| plain(&format!("{repository} {token}"));
    let ui = TokenUi {
        editing: Some(key("owner/app")),
        removing: Some(key("owner/site")),
        results: [
            (
                key("owner/app"),
                TokenResult::NotSaved(NotSaved::Store {
                    detail: Some(format!("denied {token}")),
                }),
            ),
            (
                key("owner/site"),
                TokenResult::NotSaved(NotSaved::Refused(format!("refused {token}"))),
            ),
            (
                key("owner/docs"),
                // the pool names as `pools_started_by` gives them
                TokenResult::Saved {
                    started: vec![plain(&format!("docs {token}"))],
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    let announcements: Vec<String> = ui
        .results
        .iter()
        .map(|(repository, result)| result.announcement(repository))
        .collect();
    let (mut form_error, mut refused_status, mut confirming) = (false, false, false);
    let (mut rejected, mut started) = (false, Vec::new());
    for name in FIXTURES {
        let mut value = load_json(name);
        tainted(&mut value, &token);
        let snapshot = snapshot_of(value);
        started.extend(pools_started_by(&snapshot, &key("owner/docs")));
        for connection in connections() {
            let mut tracker = Tracker::new();
            let input = Input::new(connection, Some(&snapshot), now()).tokens(&ui);
            let baseline = tracker.update(input);
            let update = tracker
                .update(Input::new(connection, Some(&snapshot), clock_at(3, 59, 0)).tokens(&ui));
            for row in update.view.list().iter().flat_map(|list| &list.tokens.rows) {
                let error = row.form.as_ref().and_then(|form| form.error.as_deref());
                form_error |= error.is_some_and(|error| error.contains("(denied <token>)"));
                refused_status |= row
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("refused <token>"));
                confirming |= row.confirming_removal;
                rejected |= row.state_text.contains("rejected the token")
                    && row.state_text.contains("<token> Bad credentials");
            }
            for text in [
                serde_json::to_string(&baseline.view).expect("JSON"),
                serde_json::to_string(&update.view).expect("JSON"),
                update.announced.messages.join("\n"),
                format!("{:?}", update.view),
                announcements.join("\n"),
            ] {
                assert!(!has_token_shape(&text), "{name} {connection:?}: {text}");
                assert!(!text.contains(&token), "{name} {connection:?}");
            }
        }
    }
    assert!(form_error, "no row showed the store's refusal in its form");
    assert!(
        refused_status,
        "no row showed the agent's refusal as its status"
    );
    assert!(confirming, "no row asked its removal question");
    assert!(rejected, "no row showed GitHub's rejection message");
    assert!(
        !started.is_empty(),
        "no save would start a pool skipped for want of a token"
    );
    for name in &started {
        assert!(!has_token_shape(name), "{name}");
    }
}

#[test]
fn no_recorded_state_shows_anything_token_shaped() {
    for name in FIXTURES {
        let view = view_of(&load(name));
        let json = serde_json::to_string(&view).expect("JSON");
        assert!(!has_token_shape(&json), "{name}");
    }
}
