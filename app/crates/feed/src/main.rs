//! `marshal-feed-check <latest.json> <version> <SHA256SUMS> <platform>=<file>...`: the
//! release run's check of the feed it wrote, before anything is published. It checks the
//! feed against the public key committed in the app's configuration, the same key every
//! installed app verifies with, and exits 1 listing every problem it found.

use std::path::Path;
use std::process::ExitCode;

use marshal_feed::{Released, check_feed, committed_public_key, recorded_sha256};

const USAGE: &str =
    "usage: marshal-feed-check <latest.json> <version> <SHA256SUMS> <platform>=<file>...";

fn main() -> ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(problems) => {
            for problem in problems {
                eprintln!("error: {problem}");
            }
            ExitCode::FAILURE
        }
    }
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("{path}: {error}"))
}

fn run(arguments: &[String]) -> Result<Vec<String>, Vec<String>> {
    let [feed, version, sums, files @ ..] = arguments else {
        return Err(vec![USAGE.to_string()]);
    };
    if files.is_empty() {
        return Err(vec![USAGE.to_string()]);
    }
    let feed = String::from_utf8(read(feed).map_err(|error| vec![error])?)
        .map_err(|_| vec![format!("{feed} is not text")])?;
    let sums_path = sums;
    let sums = String::from_utf8(read(sums).map_err(|error| vec![error])?)
        .map_err(|_| vec![format!("{sums_path} is not text")])?;
    let mut problems = Vec::new();
    let mut loaded = Vec::new();
    for file in files {
        let Some((platform, path)) = file.split_once('=') else {
            problems.push(format!("'{file}' is not <platform>=<file>"));
            continue;
        };
        let name = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .to_string();
        let Some(recorded) = recorded_sha256(&sums, &name) else {
            problems.push(format!("{sums_path} has no line for {name}"));
            continue;
        };
        match read(path) {
            Ok(bytes) => loaded.push((platform, name, bytes, recorded)),
            Err(error) => problems.push(error),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let public_key = committed_public_key().map_err(|refusal| vec![refusal.to_string()])?;
    let released: Vec<Released<'_>> = loaded
        .iter()
        .map(|(platform, name, bytes, recorded)| Released {
            platform,
            name,
            bytes,
            recorded_sha256: recorded,
        })
        .collect();
    check_feed(&feed, version, &released, &public_key)
        .map_err(|refusals| refusals.iter().map(ToString::to_string).collect::<Vec<_>>())?;
    Ok(released
        .iter()
        .map(|file| {
            format!(
                "{}: {} is signed for {version} with the committed key, at its release address",
                file.platform, file.name
            )
        })
        .collect())
}
