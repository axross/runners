//! the build context `build` streams to `docker build -`: a minimal ustar archive
//! of the definition's files, with fixed mode, owner and time so the same files give
//! the same bytes; see `docs/operations/cli.md`.

use super::definition::DefinitionFile;

const BLOCK: usize = 512;

/// the longest path the name field holds, leaving its terminating NUL.
const MAX_PATH_BYTES: usize = 99;

/// the largest file an 11-digit octal size field holds.
const MAX_FILE_BYTES: u64 = 0o77_777_777_777;

fn octal(value: u64, width: usize) -> Vec<u8> {
    let mut field = format!("{value:0digits$o}", digits = width - 1).into_bytes();
    field.push(0);
    field
}

fn padded(text: &[u8], width: usize) -> Vec<u8> {
    let mut field = text.to_vec();
    field.resize(width, 0);
    field
}

/// the 512-byte header of a regular file.
fn header(path: &str, size: u64) -> Vec<u8> {
    // the fields before the checksum: name, mode, uid, gid, size, mtime
    let mut before = padded(path.as_bytes(), 100);
    before.extend(octal(0o644, 8));
    before.extend(octal(0, 8));
    before.extend(octal(0, 8));
    before.extend(octal(size, 12));
    before.extend(octal(0, 12));
    // the fields after it: type, link name, magic, version, user and group
    // names, device numbers, prefix, padding
    let mut after = vec![b'0'];
    after.extend(padded(b"", 100));
    after.extend(b"ustar\0");
    after.extend(b"00");
    after.extend(padded(b"", 32));
    after.extend(padded(b"", 32));
    after.extend(octal(0, 8));
    after.extend(octal(0, 8));
    after.extend(padded(b"", 155 + 12));
    // the checksum is the byte sum with its own field read as spaces
    let sum: u64 = before
        .iter()
        .chain(after.iter())
        .map(|byte| u64::from(*byte))
        .sum::<u64>()
        + 8 * u64::from(b' ');
    let mut header = before;
    header.extend(format!("{sum:06o}\0 ").into_bytes());
    header.extend(after);
    header
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && path.bytes().all(|byte| byte.is_ascii_graphic())
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// the archive of `files` in their order, failing on a path the name field cannot
/// hold as a plain relative path or a file too large for the size field.
pub fn context_archive(files: &[DefinitionFile]) -> Result<Vec<u8>, String> {
    let mut archive = Vec::new();
    for file in files {
        if !valid_path(&file.path) {
            return Err(format!(
                "{:?} cannot be a path in the build context",
                file.path
            ));
        }
        let size = file.bytes.len() as u64;
        if size > MAX_FILE_BYTES {
            return Err(format!("{} is too large for the build context", file.path));
        }
        archive.extend(header(&file.path, size));
        archive.extend(&file.bytes);
        archive.resize(archive.len().next_multiple_of(BLOCK), 0);
    }
    archive.resize(archive.len() + 2 * BLOCK, 0);
    Ok(archive)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, bytes: &[u8]) -> DefinitionFile {
        DefinitionFile {
            path: path.to_string(),
            bytes: bytes.to_vec(),
        }
    }

    fn field(header: &[u8], start: usize, width: usize) -> &str {
        let raw = &header[start..start + width];
        let end = raw.iter().position(|byte| *byte == 0).unwrap_or(width);
        std::str::from_utf8(&raw[..end]).unwrap()
    }

    #[test]
    fn a_file_is_one_header_its_padded_bytes_and_the_two_end_blocks() {
        let archive = context_archive(&[file("Dockerfile", b"FROM x\n")]).unwrap();
        assert_eq!(archive.len(), 4 * BLOCK);
        let header = &archive[..BLOCK];
        assert_eq!(field(header, 0, 100), "Dockerfile");
        assert_eq!(field(header, 100, 8), "0000644");
        assert_eq!(field(header, 108, 8), "0000000");
        assert_eq!(field(header, 116, 8), "0000000");
        assert_eq!(field(header, 124, 12), "00000000007");
        assert_eq!(field(header, 136, 12), "00000000000");
        assert_eq!(header[156], b'0');
        assert_eq!(&header[257..265], b"ustar\x0000");
        assert_eq!(field(header, 345, 155), "", "no prefix");
        assert_eq!(&archive[BLOCK..BLOCK + 7], b"FROM x\n");
        assert!(archive[BLOCK + 7..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn the_checksum_is_the_header_sum_with_its_field_as_spaces() {
        let archive = context_archive(&[file("a.sh", b"echo\n")]).unwrap();
        let header = &archive[..BLOCK];
        let stored = u64::from_str_radix(field(header, 148, 7), 8).unwrap();
        assert_eq!(&header[154..156], b"\0 ");
        let sum: u64 = header
            .iter()
            .enumerate()
            .map(|(at, byte)| {
                if (148..156).contains(&at) {
                    32
                } else {
                    u64::from(*byte)
                }
            })
            .sum();
        assert_eq!(stored, sum);
    }

    #[test]
    fn the_same_files_give_the_same_bytes_and_keep_their_order() {
        let files = [file("b", &[7; 513]), file("a", b"")];
        let first = context_archive(&files).unwrap();
        assert_eq!(first, context_archive(&files).unwrap());
        assert_eq!(first.len(), (1 + 2 + 1 + 2) * BLOCK);
        assert_eq!(field(&first, 0, 100), "b");
        assert_eq!(field(&first[3 * BLOCK..], 0, 100), "a");
    }

    #[test]
    fn a_path_the_name_field_cannot_hold_plainly_is_refused() {
        for path in [
            "",
            "/abs",
            "../up",
            "a/../b",
            "a//b",
            "./a",
            "with space",
            &"x".repeat(100),
        ] {
            assert!(context_archive(&[file(path, b"")]).is_err(), "{path:?}");
        }
        assert!(context_archive(&[file(&"x".repeat(99), b"")]).is_ok());
        assert!(context_archive(&[file("dir/file", b"")]).is_ok());
    }
}
