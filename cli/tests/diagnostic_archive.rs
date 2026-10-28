//! the strict archive subset a diagnostic copy is read through: the PowerShell
//! exporter test's cases, then the header, name and entry-count cases it does not
//! build.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io;

use marshal_cli::diagnostics::archive::{
    ArchiveError, EntrySink, MAX_ENTRIES, MAX_PAYLOAD_BYTES, read_archive,
};

const MIB: u64 = 1024 * 1024;

#[derive(Default)]
struct Recorded {
    files: Vec<(String, Vec<u8>)>,
}

impl EntrySink for Recorded {
    async fn open(&mut self, name: &str) -> io::Result<()> {
        self.files.push((name.to_string(), Vec::new()));
        Ok(())
    }

    async fn write(&mut self, bytes: Vec<u8>) -> io::Result<()> {
        self.files.last_mut().unwrap().1.extend(bytes);
        Ok(())
    }
}

impl Recorded {
    fn total(&self) -> u64 {
        self.files.iter().map(|(_, bytes)| bytes.len() as u64).sum()
    }
}

/// one header as the PowerShell test's `Get-DiagnosticTar` writes it: the name,
/// the size as eleven octal digits, the type, and a six-digit checksum.
fn header(name: &str, size: u64, kind: u8) -> Vec<u8> {
    let mut header = vec![0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
    header[156] = kind;
    seal(&mut header);
    header
}

fn seal(header: &mut [u8]) {
    header[148..156].fill(b' ');
    let sum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
    header[148..154].copy_from_slice(format!("{sum:06o}").as_bytes());
    header[154] = 0;
}

/// `Get-DiagnosticTar`: one entry of `size` declared bytes with `actual` bytes
/// present, and the padding and end of the archive only when the two agree.
fn diagnostic_tar(name: &str, size: u64, kind: u8, actual: Option<u64>) -> Vec<u8> {
    let actual = actual.unwrap_or(size);
    let mut archive = header(name, size, kind);
    archive.resize(archive.len() + usize::try_from(actual).unwrap(), 0);
    if actual == size {
        let padding = (512 - size % 512) % 512 + 1024;
        archive.resize(archive.len() + usize::try_from(padding).unwrap(), 0);
    }
    archive
}

/// a regular file entry with its contents and padding, without an end.
fn entry(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut entry = header(name, contents.len() as u64, b'0');
    entry.extend(contents);
    entry.resize(entry.len().div_ceil(512) * 512, 0);
    entry
}

fn end() -> Vec<u8> {
    vec![0; 1024]
}

async fn read(archive: &[u8], raw_records: bool) -> (Result<(), ArchiveError>, Recorded) {
    let mut sink = Recorded::default();
    let mut stream = archive;
    let result = read_archive(&mut stream, raw_records, &mut sink).await;
    (result, sink)
}

/// one `Test-DiagnosticTarCase` call: the archive `Get-DiagnosticTar` builds from
/// these arguments, and whether the reader accepts it.
struct Case {
    case: &'static str,
    name: &'static str,
    size: u64,
    kind: u8,
    actual: Option<u64>,
    raw: bool,
    accepted: bool,
}

/// a case with the PowerShell test's defaults: a regular file, every declared byte
/// present, raw records off, and refused.
const fn refused(case: &'static str, name: &'static str, size: u64) -> Case {
    Case {
        case,
        name,
        size,
        kind: b'0',
        actual: None,
        raw: false,
        accepted: false,
    }
}

/// the `Test-DiagnosticTarCase` cases, each with its outcome and the payload
/// budget that holds whatever the outcome.
#[tokio::test(flavor = "current_thread")]
async fn the_powershell_archive_cases_give_the_same_outcomes() {
    let cases = [
        Case {
            accepted: true,
            ..refused("accepts regular metrics", "metrics.txt", 15)
        },
        Case {
            accepted: true,
            ..refused(
                "accepts Docker directory-relative metrics",
                "./metrics.txt",
                0,
            )
        },
        Case {
            accepted: true,
            ..refused("accepts 8 MiB metrics boundary", "metrics.txt", 8 * MIB)
        },
        Case {
            actual: Some(0),
            ..refused(
                "rejects oversized metrics before copy",
                "metrics.txt",
                8 * MIB + 1,
            )
        },
        refused("rejects raw without opt-in", "raw-1.log", 5),
        Case {
            actual: Some(0),
            raw: true,
            ..refused(
                "rejects raw over 32 MiB before copy",
                "raw-1.log",
                32 * MIB + 1,
            )
        },
        refused("rejects traversal", "../metrics.txt", 5),
        refused("rejects absolute path", "/metrics.txt", 5),
        refused("rejects arbitrary file", "configuration.txt", 5),
        Case {
            kind: b'2',
            ..refused("rejects symlink", "metrics.txt", 0)
        },
        Case {
            kind: b'1',
            ..refused("rejects hardlink", "metrics.txt", 0)
        },
        Case {
            actual: Some(5),
            ..refused(
                "rejects truncated copy but retains bounded partial",
                "metrics.txt",
                100,
            )
        },
    ];
    for Case {
        case,
        name,
        size,
        kind,
        actual,
        raw,
        accepted,
    } in cases
    {
        let (result, sink) = read(&diagnostic_tar(name, size, kind, actual), raw).await;
        assert_eq!(result.is_ok(), accepted, "{case}: {result:?}");
        assert!(
            sink.total() <= MAX_PAYLOAD_BYTES,
            "{case} stays within budget"
        );
        if case.contains("before copy") {
            assert!(sink.files.is_empty(), "{case}: nothing is written");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn each_refusal_names_its_reason() {
    for (name, size, kind, actual, raw, expected) in [
        (
            "metrics.txt",
            8 * MIB + 1,
            b'0',
            Some(0),
            false,
            ArchiveError::Budget,
        ),
        ("raw-1.log", 5, b'0', None, false, ArchiveError::Name),
        (
            "raw-1.log",
            32 * MIB + 1,
            b'0',
            Some(0),
            true,
            ArchiveError::Budget,
        ),
        ("../metrics.txt", 5, b'0', None, false, ArchiveError::Name),
        ("/metrics.txt", 5, b'0', None, false, ArchiveError::Name),
        (
            "configuration.txt",
            5,
            b'0',
            None,
            false,
            ArchiveError::Name,
        ),
        ("metrics.txt", 0, b'2', None, false, ArchiveError::Special),
        ("metrics.txt", 0, b'1', None, false, ArchiveError::Special),
        ("metrics.txt", 0, b'3', None, false, ArchiveError::Special),
        ("metrics.txt", 0, b'6', None, false, ArchiveError::Special),
        (
            "metrics.txt",
            100,
            b'0',
            Some(5),
            false,
            ArchiveError::Truncated,
        ),
    ] {
        let (result, _) = read(&diagnostic_tar(name, size, kind, actual), raw).await;
        assert_eq!(result, Err(expected), "{name} {kind}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_truncated_copy_keeps_the_bytes_it_read_as_a_partial_file() {
    let (result, sink) = read(&diagnostic_tar("metrics.txt", 100, b'0', Some(5)), false).await;
    assert_eq!(result, Err(ArchiveError::Truncated));
    assert_eq!(sink.files.len(), 1);
    assert_eq!(sink.files[0].0, "metrics.txt");
    assert!(sink.files[0].1.len() <= 5, "{}", sink.files[0].1.len());
    // a stream that ends inside a header is truncated too.
    let (result, _) = read(&[0u8; 100], false).await;
    assert_eq!(result, Err(ArchiveError::Truncated));
}

/// the PowerShell test's combined budget: 32 MiB of raw records, then metrics
/// that bring the bundle, with its 4 KiB reservation, to 40 MiB and one byte more.
#[tokio::test(flavor = "current_thread")]
async fn the_40_mib_budget_counts_the_reservation() {
    for over in [0, 1] {
        let mut archive = diagnostic_tar("raw-1.log", 32 * MIB, b'0', None);
        archive.truncate(archive.len() - 1024);
        archive.extend(diagnostic_tar(
            "metrics.txt",
            8 * MIB - 4096 + over,
            b'0',
            None,
        ));
        let (result, sink) = read(&archive, true).await;
        assert_eq!(result.is_ok(), over == 0, "{over}: {result:?}");
        if over == 1 {
            assert_eq!(result, Err(ArchiveError::Budget));
        }
        assert!(sink.total() <= MAX_PAYLOAD_BYTES);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn accepted_files_keep_their_bytes_under_their_fixed_names() {
    let mut archive = header("./", 0, b'5');
    archive.extend(entry("./metrics.txt", b"sample=1\nfinal=observed\n"));
    archive.extend(entry("./raw-1.log", b"first"));
    archive.extend(entry("runner-diagnostics/raw-22.log", b"second"));
    archive.extend(entry("raw-333.log", b""));
    archive.extend(end());
    let (result, sink) = read(&archive, true).await;
    assert_eq!(result, Ok(()));
    let files: Vec<(&str, &[u8])> = sink
        .files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    assert_eq!(
        files,
        [
            ("metrics.txt", b"sample=1\nfinal=observed\n".as_slice()),
            ("raw-1.log", b"first".as_slice()),
            ("raw-22.log", b"second".as_slice()),
            ("raw-333.log", b"".as_slice()),
        ]
    );
    for directory in [".", "runner-diagnostics/"] {
        let mut archive = header(directory, 0, b'5');
        archive.extend(entry("metrics.txt", b"a=1\n"));
        archive.extend(end());
        assert_eq!(read(&archive, false).await.0, Ok(()), "{directory}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn raw_names_outside_one_to_three_digits_are_refused() {
    for name in [
        "raw-.log",
        "raw-1234.log",
        "raw-1.txt",
        "raw-a.log",
        "xraw-1.log",
    ] {
        let mut archive = entry("metrics.txt", b"a=1\n");
        archive.extend(entry(name, b"x"));
        archive.extend(end());
        assert_eq!(
            read(&archive, true).await.0,
            Err(ArchiveError::Name),
            "{name}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_used_prefix_field_or_a_bad_size_is_a_header_refusal() {
    let mut prefixed = header("metrics.txt", 4, b'0');
    prefixed[345..350].copy_from_slice(b"../..");
    seal(&mut prefixed);
    prefixed.extend([0; 512]);
    prefixed.extend(end());
    assert_eq!(read(&prefixed, false).await.0, Err(ArchiveError::Header));

    for size in [*b"0000000008\0\0", *b"           \0", *b"777777777777"] {
        let mut bad = header("metrics.txt", 0, b'0');
        bad[124..136].copy_from_slice(&size);
        seal(&mut bad);
        bad.extend(end());
        assert_eq!(
            read(&bad, false).await.0,
            Err(ArchiveError::Header),
            "{size:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_bad_checksum_is_refused_before_anything_is_written() {
    let mut archive = diagnostic_tar("metrics.txt", 4, b'0', None);
    archive[0] = b'n';
    let (result, sink) = read(&archive, false).await;
    assert_eq!(result, Err(ArchiveError::Checksum));
    assert!(sink.files.is_empty());

    let mut unreadable = diagnostic_tar("metrics.txt", 4, b'0', None);
    unreadable[148..156].copy_from_slice(b"9999999\0");
    assert_eq!(
        read(&unreadable, false).await.0,
        Err(ArchiveError::Checksum)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_duplicate_is_refused_whatever_its_spelling() {
    for (first, second) in [
        ("metrics.txt", "metrics.txt"),
        ("metrics.txt", "./metrics.txt"),
        ("./raw-1.log", "runner-diagnostics/raw-1.log"),
    ] {
        let mut archive = entry("metrics.txt", b"a=1\n");
        if first != "metrics.txt" {
            archive.extend(entry(first, b"x"));
        }
        archive.extend(entry(second, b"y"));
        archive.extend(end());
        assert_eq!(
            read(&archive, true).await.0,
            Err(ArchiveError::Duplicate),
            "{first} then {second}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_directory_entry_with_a_size_or_another_name_is_refused() {
    let mut sized = header("./", 512, b'5');
    sized.extend([0; 512]);
    sized.extend(entry("metrics.txt", b"a=1\n"));
    sized.extend(end());
    assert_eq!(read(&sized, false).await.0, Err(ArchiveError::Directory));

    let mut other = header("elsewhere/", 0, b'5');
    other.extend(entry("metrics.txt", b"a=1\n"));
    other.extend(end());
    assert_eq!(read(&other, false).await.0, Err(ArchiveError::Special));
}

#[tokio::test(flavor = "current_thread")]
async fn an_archive_without_metrics_is_a_missing_collector() {
    assert_eq!(
        read(&end(), true).await.0,
        Err(ArchiveError::CollectorMissing)
    );
    let mut raw_only = entry("raw-1.log", b"x");
    raw_only.extend(end());
    assert_eq!(
        read(&raw_only, true).await.0,
        Err(ArchiveError::CollectorMissing)
    );
}

/// 132 headers are read, the end of the archive included, so `metrics.txt`, 130
/// directory entries and the end are accepted, and one more directory entry is not.
#[tokio::test(flavor = "current_thread")]
async fn more_than_132_headers_are_refused() {
    let skipped = header("./", 0, b'5');
    let metrics = entry("metrics.txt", b"a=1\n");
    let mut within = metrics.clone();
    for _ in 0..MAX_ENTRIES - 2 {
        within.extend(&skipped);
    }
    within.extend(end());
    assert_eq!(read(&within, false).await.0, Ok(()));

    let mut over = metrics;
    for _ in 0..MAX_ENTRIES - 1 {
        over.extend(&skipped);
    }
    over.extend(end());
    assert_eq!(
        read(&over, false).await.0,
        Err(ArchiveError::TooManyEntries)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn raw_records_share_one_32_mib_budget() {
    let mut archive = entry("metrics.txt", b"a=1\n");
    archive.extend(diagnostic_tar("raw-1.log", 16 * MIB, b'0', None));
    archive.truncate(archive.len() - 1024);
    archive.extend(diagnostic_tar("raw-2.log", 16 * MIB, b'0', None));
    assert_eq!(read(&archive, true).await.0, Ok(()));

    let mut over = entry("metrics.txt", b"a=1\n");
    over.extend(diagnostic_tar("raw-1.log", 16 * MIB, b'0', None));
    over.truncate(over.len() - 1024);
    over.extend(diagnostic_tar("raw-2.log", 16 * MIB + 1, b'0', Some(0)));
    let (result, sink) = read(&over, true).await;
    assert_eq!(result, Err(ArchiveError::Budget));
    assert_eq!(sink.files.len(), 2, "the second raw file is never started");
}
