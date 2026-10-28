//! `build` as a real process, on a test root, with the fake `docker` as its client.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::path::PathBuf;
use std::process::Output;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use support::host::{Host, Started, config_for, registered, texts};
use support::stub::Stub;
use support::{TempDir, repository_root};

const PREVIOUS: &str = "actions-runner:local\tsha256:previous\tcom.github.axross.runners.image=actions-runner\tcom.github.axross.runners.built=1";

const BUILT_WITHIN: Duration = Duration::from_secs(20);

fn host() -> Host {
    let host = Host::new();
    host.script("images", PREVIOUS);
    host
}

fn host_for(image: &str) -> Host {
    let host = host();
    let config = config_for(&host.json_token()).replace("actions-runner:local", image);
    fs::write(&host.config, config).unwrap();
    host
}

fn build(host: &Host) -> Output {
    build_from(host, host.root.path())
}

fn build_from(host: &Host, directory: &std::path::Path) -> Output {
    host.command(&["build", "--profile", "axpc", "--config"])
        .arg(&host.config)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn spawn_build(host: &Host) -> Started {
    let config = host.config.to_str().unwrap().to_string();
    host.spawn(&["build", "--profile", "axpc", "--config", &config])
}

/// the fake engine's images, as `reference id` pairs.
fn images(host: &Host) -> Vec<(String, String)> {
    fs::read_to_string(host.docker().join("images"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            Some((fields.next()?.to_string(), fields.next()?.to_string()))
        })
        .collect()
}

fn id_of(host: &Host, reference: &str) -> Option<String> {
    images(host)
        .into_iter()
        .find(|(have, _)| have == reference)
        .map(|(_, id)| id)
}

fn temporary_tags(host: &Host) -> Vec<String> {
    images(host)
        .into_iter()
        .map(|(reference, _)| reference)
        .filter(|reference| reference.contains(":marshal-build-"))
        .collect()
}

fn calls_of(host: &Host, first: &str) -> Vec<Vec<String>> {
    host.invocations()
        .into_iter()
        .filter(|call| call.first().map(String::as_str) == Some(first))
        .collect()
}

/// waits until the fake `docker` records a call that starts with `words`.
fn wait_for(host: &Host, words: &[&str], within: Duration) {
    let started = Instant::now();
    let starts_with_words = |call: &Vec<String>| {
        call.len() >= words.len() && call.iter().zip(words).all(|(have, want)| have == want)
    };
    while !host.invocations().iter().any(starts_with_words) {
        assert!(
            started.elapsed() < within,
            "no {words:?} call; the calls: {:?}",
            host.invocations()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// the standard input of every `build` call, in order.
fn contexts(host: &Host) -> Vec<Vec<u8>> {
    let mut files: Vec<PathBuf> = fs::read_dir(host.docker())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "stdin")
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| fs::read(path).unwrap())
        .collect()
}

/// the files of a ustar archive, as `(path, bytes)`, read independently of the
/// writer under test.
fn archive_files(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut at = 0;
    loop {
        let header = &archive[at..at + 512];
        if header.iter().all(|byte| *byte == 0) {
            assert!(archive[at..].iter().all(|byte| *byte == 0));
            assert_eq!(archive.len() - at, 1024, "two zero blocks end it");
            return files;
        }
        let field = |start: usize, width: usize| {
            let raw = &header[start..start + width];
            let end = raw.iter().position(|byte| *byte == 0).unwrap_or(width);
            String::from_utf8(raw[..end].to_vec()).unwrap()
        };
        assert_eq!(field(156, 1), "0", "a regular file");
        assert_eq!(field(100, 8), "0000644");
        assert_eq!(field(108, 8), "0000000");
        assert_eq!(field(116, 8), "0000000");
        assert_eq!(field(136, 12), "00000000000");
        let size = usize::from_str_radix(&field(124, 12), 8).unwrap();
        let data = archive[at + 512..at + 512 + size].to_vec();
        files.push((field(0, 100), data));
        at += 512 + size.div_ceil(512) * 512;
    }
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn assert_temporary_tag(tag: &str, repository: &str) {
    let digits = tag
        .strip_prefix(&format!("{repository}:marshal-build-"))
        .unwrap_or_else(|| panic!("{tag} is not a temporary tag of {repository}"));
    assert_eq!(digits.len(), 17, "{tag}");
    assert!(digits.bytes().all(|byte| byte.is_ascii_digit()), "{tag}");
}

#[test]
fn build_sends_one_docker_build_with_both_labels_the_temporary_tag_and_the_definition() {
    let host = host();
    let empty = TempDir::new();
    let before = now_seconds();
    let output = build_from(&host, empty.path());
    let after = now_seconds();
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("Building actions-runner:local."),
        "{stderr}"
    );

    let builds = calls_of(&host, "build");
    assert_eq!(builds.len(), 1);
    let build = &builds[0];
    assert_eq!(
        build[..5],
        [
            "build",
            "--pull",
            "--no-cache",
            "--label",
            "com.github.axross.runners.image=actions-runner"
        ]
    );
    assert_eq!(build[5], "--label");
    let built: u64 = build[6]
        .strip_prefix("com.github.axross.runners.built=")
        .unwrap()
        .parse()
        .unwrap();
    assert!((before..=after).contains(&built), "{built}");
    assert_eq!(build[7], "--tag");
    assert_temporary_tag(&build[8], "actions-runner");
    assert_eq!(build[9], "-");
    assert_eq!(build.len(), 10);

    let definition = repository_root().join("images/actions-runner");
    let expected: Vec<(String, Vec<u8>)> =
        ["Dockerfile", "install-ndks.sh", "runner-diagnostics.cpp"]
            .iter()
            .map(|path| {
                (
                    (*path).to_string(),
                    fs::read(definition.join(path)).unwrap(),
                )
            })
            .collect();
    let first = contexts(&host).remove(0);
    assert_eq!(archive_files(&first), expected);

    assert_eq!(build_from(&host, empty.path()).status.code(), Some(0));
    let all = contexts(&host);
    assert_eq!(all.len(), 2);
    assert_eq!(all[1], first);
}

#[test]
fn a_successful_build_tags_the_name_removes_the_temporary_tag_and_prunes_by_the_label() {
    let host = host();
    let output = build(&host);
    assert_eq!(output.status.code(), Some(0), "{}", texts(&output).1);
    let tag = calls_of(&host, "build")[0][8].clone();
    let words: Vec<Vec<String>> = host.invocations();
    let firsts: Vec<String> = words
        .iter()
        .map(|call| call[..2.min(call.len())].join(" "))
        .collect();
    let tag_call = format!("tag {tag}");
    assert_eq!(
        firsts,
        [
            "info",
            "image ls",
            "build --pull",
            tag_call.as_str(),
            "image ls",
            "image rm",
            "image prune"
        ]
    );
    assert_eq!(words[3], ["tag", tag.as_str(), "actions-runner:local"]);
    assert_eq!(words[5], ["image", "rm", tag.as_str()]);
    assert_eq!(
        words[6],
        [
            "image",
            "prune",
            "--force",
            "--filter",
            "label=com.github.axross.runners.image=actions-runner"
        ]
    );
    assert!(
        id_of(&host, "actions-runner:local")
            .unwrap()
            .starts_with("sha256:built-")
    );
    assert!(temporary_tags(&host).is_empty());
}

#[test]
fn the_temporary_tag_keeps_a_registry_port_and_drops_only_the_tag() {
    for (image, repository) in [
        (
            "localhost:5000/team/runner:v2",
            "localhost:5000/team/runner",
        ),
        ("localhost:5000/runner", "localhost:5000/runner"),
    ] {
        let host = host_for(image);
        let output = build(&host);
        assert_eq!(output.status.code(), Some(0), "{}", texts(&output).1);
        let tag = calls_of(&host, "build")[0][8].clone();
        assert_temporary_tag(&tag, repository);
        assert_eq!(calls_of(&host, "tag")[0], ["tag", tag.as_str(), image]);
        assert!(id_of(&host, image).is_some());
    }
}

#[test]
fn a_digest_image_name_is_refused_before_any_docker_call() {
    let host = host_for(
        "actions-runner@sha256:e5496277be5d09bc968b3d64911b74e219ac4a3f2edce956a3ecf9271bea1ef4",
    );
    let output = build(&host);
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("names a digest, which cannot be tagged"),
        "{stderr}"
    );
    assert!(host.invocations().is_empty());
}

#[test]
fn a_leftover_temporary_tag_of_this_repository_is_removed_before_the_build() {
    let host = host();
    let label = "com.github.axross.runners.image=actions-runner";
    host.script(
        "images",
        &format!(
            "{PREVIOUS}\n\
             actions-runner:marshal-build-20240101000000000\tsha256:old\t{label}\n\
             actions-runner:marshal-build-20240101000000001\tsha256:unlabelled\n\
             actions-runner:marshal-build-notastamp\tsha256:odd\t{label}\n\
             other:marshal-build-20240101000000000\tsha256:other\t{label}"
        ),
    );
    let output = build(&host);
    assert_eq!(output.status.code(), Some(0), "{}", texts(&output).1);
    let words = host.invocations();
    assert_eq!(
        words[1],
        [
            "image",
            "ls",
            "--filter",
            "reference=actions-runner:marshal-build-*",
            "--filter",
            "label=com.github.axross.runners.image=actions-runner",
            "--format",
            "{{.Repository}}:{{.Tag}}"
        ]
    );
    assert_eq!(
        words[2],
        [
            "image",
            "rm",
            "actions-runner:marshal-build-20240101000000000"
        ]
    );
    assert_eq!(words[3][0], "build", "the removal came before the build");
    let left = temporary_tags(&host);
    assert_eq!(
        left,
        [
            "actions-runner:marshal-build-20240101000000001",
            "actions-runner:marshal-build-notastamp",
            "other:marshal-build-20240101000000000"
        ]
    );
}

#[test]
fn a_failed_tag_removal_after_the_retag_warns_with_the_tag_and_exits_0() {
    let host = host();
    host.script("image_rm_fails", "");
    let output = build(&host);
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let tag = calls_of(&host, "build")[0][8].clone();
    assert!(
        stderr.contains(&format!(
            "WARNING: The temporary tag {tag} could not be removed"
        )),
        "{stderr}"
    );
    assert!(
        id_of(&host, "actions-runner:local")
            .unwrap()
            .starts_with("sha256:built-")
    );
    assert_eq!(calls_of(&host, "image").last().unwrap()[1], "prune");
}

#[test]
fn a_failed_prune_warns_and_exits_0() {
    let host = host();
    host.script("prune_fails", "");
    let output = build(&host);
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("WARNING: The old runner images could not be pruned"),
        "{stderr}"
    );
}

#[test]
fn a_failed_build_leaves_the_name_untouched_and_exits_1() {
    let host = host();
    host.script("build_fails", "");
    let output = build(&host);
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("actions-runner:local keeps its previous image"),
        "{stderr}"
    );
    assert_eq!(
        id_of(&host, "actions-runner:local").as_deref(),
        Some("sha256:previous")
    );
    assert!(temporary_tags(&host).is_empty());
    assert!(calls_of(&host, "tag").is_empty());
}

#[test]
fn a_failed_retag_removes_the_temporary_tag_leaves_the_name_untouched_and_exits_1() {
    let host = host();
    host.script("tag_fails", "");
    let output = build(&host);
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("error: tagging the build as actions-runner:local failed"),
        "{stderr}"
    );
    assert!(
        stderr.contains("actions-runner:local keeps its previous image"),
        "{stderr}"
    );
    let tag = calls_of(&host, "build")[0][8].clone();
    assert!(
        calls_of(&host, "image").contains(&vec!["image".to_string(), "rm".to_string(), tag]),
        "the temporary tag was removed"
    );
    assert!(temporary_tags(&host).is_empty(), "{:?}", images(&host));
    assert_eq!(
        id_of(&host, "actions-runner:local").as_deref(),
        Some("sha256:previous")
    );
    assert!(
        calls_of(&host, "image")
            .iter()
            .all(|call| call[1] != "prune"),
        "a build that did not commit prunes nothing"
    );
}

#[test]
fn docker_that_does_not_answer_within_30_s_ends_the_build_before_it_starts() {
    let host = host();
    host.script("info_hangs", "");
    let started = Instant::now();
    let output = build(&host);
    let took = started.elapsed();
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("Docker did not answer within 30 s"),
        "{stderr}"
    );
    assert!(
        took >= Duration::from_secs(29) && took < Duration::from_secs(40),
        "{took:?}"
    );
    assert!(calls_of(&host, "build").is_empty());
}

#[test]
fn docker_that_fails_to_answer_ends_the_build_at_once() {
    let host = host();
    host.script("info_failures", "1");
    let output = build(&host);
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("`docker info` failed (Cannot connect to the Docker daemon."),
        "{stderr}"
    );
    assert!(!stderr.contains("within 30 s"), "{stderr}");
    assert_eq!(host.invocations(), [vec!["info".to_string()]]);
}

#[test]
fn the_axmc_profile_validates_then_exits_1_naming_phase_6_and_a_usage_error_exits_2() {
    let host = host();
    let fixture =
        repository_root().join("tests/host-configuration/rust/accepted/axmc-defaults.json");
    let axmc = host.root.write("axmc.json", &fs::read(fixture).unwrap());
    let output = host
        .command(&["build", "--profile", "axmc", "--config"])
        .arg(&axmc)
        .output()
        .unwrap();
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("comes in Phase 6"), "{stderr}");
    assert_eq!(host.client(&["build"]).status.code(), Some(2));
    assert_eq!(
        host.client(&["build", "--config", "x", "--profile", "other"])
            .status
            .code(),
        Some(2)
    );
    let invalid = host.root.write("invalid.json", b"{}");
    let output = host
        .command(&["build", "--profile", "axpc", "--config"])
        .arg(&invalid)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(texts(&output).1.contains("Invalid host configuration"));
    assert!(host.invocations().is_empty());
}

mod abort {
    use super::*;

    /// waits for the build to reach a call that starts with `words`, asks `stop`,
    /// and returns how the build and `stop` ended and how long it took after the stop.
    fn stop_at(host: &Host, words: &[&str]) -> (Output, support::host::Finished, Duration) {
        let started = spawn_build(host);
        wait_for(host, words, BUILT_WITHIN);
        let asked = Instant::now();
        let stop = host.client(&["stop"]);
        let finished = started.finish(Duration::from_secs(40));
        (stop, finished, asked.elapsed())
    }

    #[test]
    fn a_stop_before_the_retag_removes_the_temporary_tag_and_exits_1_within_30_s() {
        let host = host();
        host.script("build_hangs", "tagged");
        let (stop, finished, took) = stop_at(&host, &["build"]);
        assert_eq!(stop.status.code(), Some(0), "{}", texts(&stop).1);
        assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
        assert!(took < Duration::from_secs(30), "{took:?}");
        assert!(
            finished
                .stderr
                .contains("actions-runner:local keeps its previous image"),
            "{}",
            finished.stderr
        );
        assert_eq!(
            id_of(&host, "actions-runner:local").as_deref(),
            Some("sha256:previous")
        );
        assert!(temporary_tags(&host).is_empty(), "{:?}", images(&host));
        assert!(calls_of(&host, "tag").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn sigint_before_the_retag_aborts_the_build_the_same_way() {
        let host = host();
        host.script("build_hangs", "tagged");
        let started = spawn_build(&host);
        wait_for(&host, &["build"], BUILT_WITHIN);
        support::host::send_signal(started.pid(), "INT");
        let finished = started.finish(Duration::from_secs(30));
        assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
        assert_eq!(
            id_of(&host, "actions-runner:local").as_deref(),
            Some("sha256:previous")
        );
        assert!(temporary_tags(&host).is_empty(), "{:?}", images(&host));
    }

    #[test]
    fn a_stop_after_the_retag_exits_0_and_skips_the_prune() {
        let host = host();
        host.script("prune_hangs", "");
        let (_, finished, _) = stop_at(&host, &["image", "prune"]);
        assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
        assert!(
            finished.stderr.contains("The stop ended the prune"),
            "{}",
            finished.stderr
        );
        assert!(
            id_of(&host, "actions-runner:local")
                .unwrap()
                .starts_with("sha256:built-")
        );
        assert!(temporary_tags(&host).is_empty());
    }

    #[test]
    fn a_stop_during_a_stalled_retag_that_did_not_tag_leaves_the_name_and_exits_1() {
        let host = host();
        host.script("tag_hangs", "");
        let (_, finished, took) = stop_at(&host, &["tag"]);
        assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
        assert!(
            took >= Duration::from_secs(4),
            "the tag got its grace: {took:?}"
        );
        assert!(took < Duration::from_secs(30), "{took:?}");
        assert_eq!(
            id_of(&host, "actions-runner:local").as_deref(),
            Some("sha256:previous")
        );
        assert!(temporary_tags(&host).is_empty());
    }

    #[test]
    fn a_stop_during_a_stalled_retag_that_tagged_decides_by_the_build_time_and_exits_0() {
        let host = host();
        host.script("tag_hangs", "after");
        let (_, finished, _) = stop_at(&host, &["tag"]);
        assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
        assert!(
            id_of(&host, "actions-runner:local")
                .unwrap()
                .starts_with("sha256:built-")
        );
        assert!(temporary_tags(&host).is_empty());
        assert!(
            calls_of(&host, "image")
                .iter()
                .all(|call| call[1] != "prune"),
            "the stop skips the prune"
        );
    }

    #[test]
    fn a_stop_during_a_stalled_retag_whose_inspect_stalls_reports_commit_unknown() {
        let host = host();
        host.script("tag_hangs", "");
        host.script("inspect_hangs", "");
        let (_, finished, took) = stop_at(&host, &["tag"]);
        assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
        assert!(
            finished.stderr.contains("commit unknown"),
            "{}",
            finished.stderr
        );
        assert!(
            finished.stderr.contains("actions-runner:local"),
            "{}",
            finished.stderr
        );
        assert!(took < Duration::from_secs(30), "{took:?}");
    }
}

mod instance {
    use super::*;

    /// waits until the fake engine holds the slot's job container, after which the
    /// instance calls `docker` no more until it is stopped.
    fn wait_for_job(host: &Host) {
        let started = Instant::now();
        host.wait_for_status("running container r-1-", BUILT_WITHIN);
        while !host
            .containers()
            .iter()
            .any(|name| name.starts_with("r-1-"))
        {
            assert!(
                started.elapsed() < BUILT_WITHIN,
                "the fake docker never recorded the job container; its calls: {:?}",
                host.invocations()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn build_is_refused_while_an_instance_runs_or_is_stopped_before_any_docker_call() {
        let host = host();
        fs::remove_file(host.docker().join("images")).unwrap();
        host.script("run_mode", "hold");
        let stub = Stub::start(vec![registered(), registered()]);
        let started = host.start(&stub);
        wait_for_job(&host);

        let before = host.invocations();
        let refused = build(&host);
        let (_, stderr) = texts(&refused);
        assert_eq!(refused.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("an instance exists and is running"),
            "{stderr}"
        );
        assert_eq!(host.invocations(), before, "the refused build ran nothing");

        let valid = fs::read(&host.config).unwrap();
        fs::write(&host.config, b"{}").unwrap();
        assert_eq!(host.client(&["restart"]).status.code(), Some(1));
        host.wait_for_status("state: stopped", Duration::from_secs(5));
        fs::write(&host.config, &valid).unwrap();
        let before = host.invocations();
        let refused = build(&host);
        let (_, stderr) = texts(&refused);
        assert_eq!(refused.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("an instance exists and is stopped"),
            "{stderr}"
        );
        assert_eq!(host.invocations(), before, "the refused build ran nothing");

        assert_eq!(host.client(&["stop"]).status.code(), Some(0));
        assert_eq!(
            started.finish(Duration::from_secs(40)).status.code(),
            Some(0)
        );
    }

    #[test]
    fn while_a_build_runs_start_restart_and_diagnostics_are_refused_and_status_shows_it() {
        let host = host();
        host.script("build_hangs", "");
        let started = spawn_build(&host);
        wait_for(&host, &["build"], BUILT_WITHIN);

        let status = host.client(&["status"]);
        let (out, err) = texts(&status);
        assert_eq!(status.status.code(), Some(0), "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "state: building");
        assert_eq!(lines[1], "image: actions-runner:local");
        assert!(
            lines[2].starts_with("elapsed: ") && lines[2].ends_with(" s"),
            "{out}"
        );

        let start = host
            .command(&["start", "--profile", "axpc", "--config"])
            .arg(&host.config)
            .output()
            .unwrap();
        let (_, stderr) = texts(&start);
        assert_eq!(start.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("the instance is building actions-runner:local"),
            "{stderr}"
        );

        let restart = host.client(&["restart"]);
        let (_, stderr) = texts(&restart);
        assert_eq!(restart.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("the instance is building"), "{stderr}");

        let diagnostics = host.client(&["diagnostics"]);
        let (_, stderr) = texts(&diagnostics);
        assert_eq!(diagnostics.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("the instance is building"), "{stderr}");

        let second = build(&host);
        assert_eq!(second.status.code(), Some(1));
        assert!(
            texts(&second)
                .1
                .contains("an instance exists and is building")
        );

        assert_eq!(host.client(&["stop"]).status.code(), Some(0));
        let finished = started.finish(Duration::from_secs(30));
        assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
        assert_eq!(calls_of(&host, "build").len(), 1);
    }
}
