//! The strict archive subset a diagnostic copy is read through, as the PowerShell
//! exporter reads it: ustar headers with matching checksums and no prefix, regular
//! files and the collector's own directory entry only, `metrics.txt` and, with raw
//! records on, `raw-<n>.log`, each once, within the entry and byte limits. Nothing
//! is extracted by a path the job chose: an accepted name is a fixed file name.

use std::future::Future;
use std::io;

use tokio::io::{AsyncRead, AsyncReadExt};

/// The most headers read, the end of the archive included.
pub const MAX_ENTRIES: usize = 132;

/// The largest `metrics.txt`.
pub const MAX_METRICS_BYTES: u64 = 8 * 1024 * 1024;

/// The most bytes of every raw file together.
pub const MAX_RAW_BYTES: u64 = 32 * 1024 * 1024;

/// The most bytes of a bundle, [`RESERVED_BYTES`] included.
pub const MAX_PAYLOAD_BYTES: u64 = 40 * 1024 * 1024;

/// The bytes a bundle keeps for the host's own files.
pub const RESERVED_BYTES: u64 = 4096;

/// The collector's metrics file, which every archive must hold.
pub const METRICS_FILE: &str = "metrics.txt";

const BLOCK: usize = 512;

/// The most bytes copied in one piece.
const PIECE_BYTES: u64 = 64 * 1024;

/// Why an archive was refused. Files already written stay, as a partial bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveError {
    /// The stream ended inside a header or a file.
    Truncated,
    /// A size is not octal, or the prefix field is used.
    Header,
    Checksum,
    /// The collector's directory entry has a size.
    Directory,
    /// A link, a device or any other record that is not a regular file.
    Special,
    /// A name other than the accepted ones, or a raw file with raw records off.
    Name,
    Duplicate,
    /// A file is over its limit, or the bundle over its payload.
    Budget,
    /// More than [`MAX_ENTRIES`] headers.
    TooManyEntries,
    /// The archive ended without `metrics.txt`.
    CollectorMissing,
    /// The sink could not take a file.
    Write(io::ErrorKind),
}

/// Where accepted files go.
pub trait EntrySink {
    /// Starts the file `name`, which never exists yet.
    fn open(&mut self, name: &str) -> impl Future<Output = io::Result<()>>;

    /// Appends to the file last started.
    fn write(&mut self, bytes: Vec<u8>) -> impl Future<Output = io::Result<()>>;
}

async fn read_exact<R: AsyncRead + Unpin>(
    stream: &mut R,
    count: u64,
) -> Result<Vec<u8>, ArchiveError> {
    let mut bytes = Vec::new();
    let read = stream
        .take(count)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ArchiveError::Truncated)?;
    if read as u64 != count {
        return Err(ArchiveError::Truncated);
    }
    Ok(bytes)
}

fn field(header: &[u8], start: usize, length: usize) -> &[u8] {
    header.get(start..start + length).unwrap_or_default()
}

/// The ASCII octal number in `bytes`, with NULs and spaces trimmed from both ends,
/// of at most `digits` digits.
fn octal(bytes: &[u8], digits: usize) -> Option<u64> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.trim_matches(['\0', ' ']);
    if text.is_empty()
        || text.len() > digits
        || !text.bytes().all(|byte| matches!(byte, b'0'..=b'7'))
    {
        return None;
    }
    u64::from_str_radix(text, 8).ok()
}

/// A header's name, size and type, once its size, prefix and checksum hold.
fn parse(header: &[u8]) -> Result<(String, u64, u8), ArchiveError> {
    let size = octal(field(header, 124, 12), 11);
    let prefix_used = field(header, 345, 155).iter().any(|byte| *byte != 0);
    let (Some(size), false) = (size, prefix_used) else {
        return Err(ArchiveError::Header);
    };
    let declared = octal(field(header, 148, 8), 7).ok_or(ArchiveError::Checksum)?;
    let sum: u64 = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                32
            } else {
                u64::from(*byte)
            }
        })
        .sum();
    if sum != declared {
        return Err(ArchiveError::Checksum);
    }
    let name = field(header, 0, 100);
    let end = name
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |last| last + 1);
    let name = std::str::from_utf8(name.get(..end).unwrap_or_default())
        .ok()
        .filter(|name| name.is_ascii())
        .ok_or(ArchiveError::Name)?;
    let kind = header.get(156).copied().unwrap_or_default();
    Ok((name.to_string(), size, kind))
}

fn is_raw(name: &str) -> bool {
    name.strip_prefix("raw-")
        .and_then(|rest| rest.strip_suffix(".log"))
        .is_some_and(|number| {
            (1..=3).contains(&number.len()) && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// Reads the archive on `stream` into `sink`, refusing anything outside the subset.
/// The caller bounds the time it takes.
///
/// # Errors
///
/// See [`ArchiveError`].
pub async fn read_archive<R: AsyncRead + Unpin, S: EntrySink>(
    stream: &mut R,
    raw_records: bool,
    sink: &mut S,
) -> Result<(), ArchiveError> {
    let mut payload = RESERVED_BYTES;
    let mut raw_payload = 0;
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..MAX_ENTRIES {
        let header = read_exact(stream, BLOCK as u64).await?;
        if header.iter().all(|byte| *byte == 0) {
            if seen.iter().any(|name| name == METRICS_FILE) {
                return Ok(());
            }
            return Err(ArchiveError::CollectorMissing);
        }
        let (name, size, kind) = parse(&header)?;
        if kind == b'5' && matches!(name.as_str(), "./" | "runner-diagnostics/" | ".") {
            if size != 0 {
                return Err(ArchiveError::Directory);
            }
            continue;
        }
        if kind != 0 && kind != b'0' {
            return Err(ArchiveError::Special);
        }
        let name = name.strip_prefix("./").unwrap_or(&name);
        let name = name.strip_prefix("runner-diagnostics/").unwrap_or(name);
        let raw = is_raw(name);
        if name != METRICS_FILE && !(raw_records && raw) {
            return Err(ArchiveError::Name);
        }
        if seen.iter().any(|have| have == name) {
            return Err(ArchiveError::Duplicate);
        }
        seen.push(name.to_string());
        if raw {
            raw_payload += size;
            if raw_payload > MAX_RAW_BYTES {
                return Err(ArchiveError::Budget);
            }
        } else if size > MAX_METRICS_BYTES {
            return Err(ArchiveError::Budget);
        }
        payload += size;
        if payload > MAX_PAYLOAD_BYTES {
            return Err(ArchiveError::Budget);
        }
        sink.open(name)
            .await
            .map_err(|error| ArchiveError::Write(error.kind()))?;
        let mut left = size;
        while left > 0 {
            let count = left.min(PIECE_BYTES);
            let bytes = read_exact(stream, count).await?;
            sink.write(bytes)
                .await
                .map_err(|error| ArchiveError::Write(error.kind()))?;
            left -= count;
        }
        let padding = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
        read_exact(stream, padding).await?;
    }
    Err(ArchiveError::TooManyEntries)
}
