//! the app's log: its own files in the agent's folder, continued at the size limit with
//! the newest ten kept, and never a token.

use std::fs;

use marshal_session::{AppLog, KEPT_FILES, Log, MAX_FILE_BYTES};

use crate::support::temp_dir;

fn names(directory: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn the_limits_are_ten_mib_and_ten_files() {
    assert_eq!(MAX_FILE_BYTES, 10 * 1024 * 1024);
    assert_eq!(KEPT_FILES, 10);
}

#[test]
fn a_file_that_would_pass_its_limit_is_continued_in_a_new_one_and_ten_are_kept() {
    let directory = temp_dir("log-rolls");
    let logs = directory.join("logs");
    let log = AppLog::open(&logs, 512).unwrap();
    for index in 0..400 {
        log.info(&format!("line {index} {}", "x".repeat(40)));
    }
    let names = names(&logs);
    assert_eq!(names.len(), 10, "{names:?}");
    for name in &names {
        assert!(name.starts_with("app-") && name.ends_with(".log"), "{name}");
        assert!(
            fs::metadata(logs.join(name)).unwrap().len() <= 512,
            "{name}"
        );
    }
    let last = fs::read_to_string(logs.join(names.last().unwrap())).unwrap();
    assert!(last.contains("line 399"), "{last}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn the_agents_files_are_never_pruned_and_never_count() {
    let directory = temp_dir("log-agents");
    let logs = directory.join("logs");
    fs::create_dir_all(&logs).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    }
    // the agent's files, named as the agent names them
    for index in 0..12 {
        fs::write(
            logs.join(format!("marshal-2026110400000{index:04}-1.log")),
            b"agent",
        )
        .unwrap();
    }
    let log = AppLog::open(&logs, 256).unwrap();
    for index in 0..200 {
        log.warn(&format!("line {index} {}", "y".repeat(60)));
    }
    let names = names(&logs);
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("marshal-"))
            .count(),
        12
    );
    assert_eq!(
        names.iter().filter(|name| name.starts_with("app-")).count(),
        10
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_word_shaped_like_a_token_never_reaches_the_file() {
    let directory = temp_dir("log-token");
    let logs = directory.join("logs");
    let log = AppLog::open(&logs, MAX_FILE_BYTES).unwrap();
    let token = format!("{}{}_{}", "gh", "p", "abcdefghijklmnopqrstuvwx");
    log.info(&format!("the value is {token} and the next"));
    log.warn(&format!("another {token}"));
    for name in names(&logs) {
        let text = fs::read_to_string(logs.join(name)).unwrap();
        assert!(!text.contains("abcdefghijklmnop"), "{text}");
        assert!(text.contains("the value is <token> and the next"), "{text}");
    }
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn a_new_file_is_0600_in_a_0700_directory() {
    use std::os::unix::fs::PermissionsExt;
    let directory = temp_dir("log-private");
    let logs = directory.join("logs");
    let log = AppLog::open(&logs, MAX_FILE_BYTES).unwrap();
    log.info("a line");
    let mode = |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&logs), 0o700);
    let file = logs.join(&names(&logs)[0]);
    assert_eq!(mode(&file), 0o600);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_line_carries_its_time_and_a_warning_says_so() {
    let directory = temp_dir("log-lines");
    let logs = directory.join("logs");
    let log = AppLog::open(&logs, MAX_FILE_BYTES).unwrap();
    log.info("hello");
    log.warn("careful");
    let text = fs::read_to_string(logs.join(&names(&logs)[0])).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].ends_with(" hello") && lines[0].len() > "hello".len() + 17,
        "{}",
        lines[0]
    );
    assert!(lines[1].ends_with(" WARNING: careful"), "{}", lines[1]);
    fs::remove_dir_all(directory).unwrap();
}
