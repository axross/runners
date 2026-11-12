//! prints the view of a recorded snapshot as JSON, so the web view's pages can be
//! rendered and checked outside the app.
//!
//! `cargo run -p marshal-view --example dump_view -- <fixture.json> [options]`
//!
//! - `--connection <connected|starting|dropped|restart-failed|busy|no-agent|not-answering|lock-held|quitting>`
//!   (default `connected`); the ones that hold no list print without the snapshot unless
//!   `--keep-snapshot` is given.
//! - `--pending <stop-all|start-all|build|stop-build>`
//! - `--editing <owner/name>` and `--removing <owner/name>`: the token forms that are open.
//! - `--result <owner/name>=<saved|removed|not-removed:<why>|too-long|store|store:<detail>|refused:<words>|lost>`
//! - `--settled <count>`: the settled count of runners up.

use std::collections::BTreeMap;
use std::error::Error;

use marshal_cli::agent::snapshot::Snapshot;
use marshal_view::{
    Cause, Clock, Connection, Input, NotSaved, Pending, PendingKind, TokenResult, TokenUi, build,
};

/// 2026-11-04 03:52 UTC, the moment the fixtures' times are written against.
const NOW_MS: u64 = 1_793_750_400_000 + (3 * 60 + 52) * 60_000;

type Failure = Box<dyn Error>;

fn connection(name: &str) -> Result<Connection, Failure> {
    let lost = |cause| Connection::LostContact {
        last_heard_ms: NOW_MS,
        cause,
    };
    Ok(match name {
        "connected" => Connection::Connected,
        "starting" => Connection::StartingMarshal,
        "dropped" => lost(Cause::Dropped),
        "restart-failed" => lost(Cause::RestartFailed),
        "busy" => lost(Cause::Busy),
        "no-agent" => Connection::NoAgent,
        "not-answering" => Connection::NotAnswering,
        "lock-held" => Connection::LockHeld,
        "quitting" => Connection::Quitting,
        other => return Err(format!("unknown connection {other}").into()),
    })
}

fn pending(name: &str) -> Result<Pending, Failure> {
    let kind = match name {
        "stop-all" => PendingKind::StopAll,
        "start-all" => PendingKind::StartAll,
        "build" => PendingKind::Build,
        "stop-build" => PendingKind::StopBuild,
        other => return Err(format!("unknown pending action {other}").into()),
    };
    Ok(Pending {
        kind,
        since_ms: NOW_MS,
    })
}

fn result(spec: &str) -> Result<(String, TokenResult), Failure> {
    let (repository, kind) = spec
        .split_once('=')
        .ok_or("a result is written <owner/name>=<kind>")?;
    let (word, detail) = match kind.split_once(':') {
        Some((word, detail)) => (word, Some(detail.to_string())),
        None => (kind, None),
    };
    let result = match word {
        "saved" => TokenResult::Saved {
            started: detail.into_iter().collect(),
        },
        "removed" => TokenResult::Removed,
        "not-removed" => TokenResult::NotRemoved(detail.unwrap_or_default()),
        "too-long" => TokenResult::NotSaved(NotSaved::TooLong),
        "store" => TokenResult::NotSaved(NotSaved::Store { detail }),
        "refused" => TokenResult::NotSaved(NotSaved::Refused(detail.unwrap_or_default())),
        "lost" => TokenResult::NotSaved(NotSaved::LostContact),
        other => return Err(format!("unknown result {other}").into()),
    };
    Ok((repository.to_string(), result))
}

fn main() -> Result<(), Failure> {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().ok_or("name a fixture file")?;
    let snapshot: Snapshot = serde_json::from_str(&std::fs::read_to_string(path)?)?;

    let mut connected = Connection::Connected;
    let mut pending_action = None;
    let mut tokens = TokenUi::default();
    let mut settled = None;
    let mut keep_snapshot = false;
    let mut results = BTreeMap::new();
    while let Some(flag) = arguments.next() {
        if flag == "--keep-snapshot" {
            keep_snapshot = true;
            continue;
        }
        let value = arguments.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--connection" => connected = connection(&value)?,
            "--pending" => pending_action = Some(pending(&value)?),
            "--editing" => tokens.editing = Some(value),
            "--removing" => tokens.removing = Some(value),
            "--result" => {
                let (repository, outcome) = result(&value)?;
                results.insert(repository, outcome);
            }
            "--settled" => settled = Some(value.parse::<u32>()?),
            other => return Err(format!("unknown option {other}").into()),
        }
    }
    tokens.results = results;

    let holds_list = matches!(
        connected,
        Connection::Connected | Connection::LostContact { .. }
    );
    let shown = (holds_list || keep_snapshot).then_some(&snapshot);
    let mut input = Input::new(connected, shown, Clock::new(NOW_MS, 0))
        .pending(pending_action)
        .tokens(&tokens);
    if let Some(count) = settled {
        input = input.settled(count);
    }
    println!("{}", serde_json::to_string_pretty(&build(&input))?);
    Ok(())
}
