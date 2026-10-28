//! the sink checks and bundle admission: a sink must be an existing directory outside
//! the checkout, refused when missing and never created, with no link in its path and,
//! on Unix, private to the current user (on Windows its owner and rules are not
//! checked and a bundle inherits the sink's); the checks never repair what they
//! refuse; see `docs/operations/cli.md`.

use std::fmt;
use std::fs;
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::profile::Profile;
use crate::validate::is_inside_checkout;

/// why a sink or a bundle was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkError {
    Missing(PathBuf),
    NotADirectory(PathBuf),
    /// the path, or a directory above it, is a link or a reparse point.
    Link(PathBuf),
    InsideCheckout(PathBuf),
    Unreadable(PathBuf, io::ErrorKind),
    #[cfg(unix)]
    NotPrivate(crate::private::PrivateError),
}

impl fmt::Display for SinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(path) => write!(f, "{} does not exist", path.display()),
            Self::NotADirectory(path) => write!(f, "{} is not a directory", path.display()),
            Self::Link(path) => write!(f, "{} is a link or a reparse point", path.display()),
            Self::InsideCheckout(path) => {
                write!(f, "{} is inside the checkout", path.display())
            }
            Self::Unreadable(path, kind) => {
                write!(f, "{} cannot be examined ({kind})", path.display())
            }
            #[cfg(unix)]
            Self::NotPrivate(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SinkError {}

/// the profile whose path rules this OS's paths follow, for the checkout rule.
#[cfg(windows)]
const PATH_RULES: Profile = Profile::Axpc;
#[cfg(not(windows))]
const PATH_RULES: Profile = Profile::Axmc;

fn examine(path: &Path) -> Result<fs::Metadata, SinkError> {
    fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            SinkError::Missing(path.to_path_buf())
        } else {
            SinkError::Unreadable(path.to_path_buf(), error.kind())
        }
    })
}

#[cfg(windows)]
fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// checks that `path` is a directory with no link in it and, on Unix, private to the
/// current user.
fn check_directory(path: &Path) -> Result<(), SinkError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() || ancestor.parent().is_none() {
            continue;
        }
        if is_link(&examine(ancestor)?) {
            return Err(SinkError::Link(ancestor.to_path_buf()));
        }
    }
    if !examine(path)?.is_dir() {
        return Err(SinkError::NotADirectory(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        let uid = crate::private::current_uid()
            .map_err(|error| SinkError::Unreadable(path.to_path_buf(), error.kind()))?;
        crate::private::check_private_directory(path, uid).map_err(SinkError::NotPrivate)?;
    }
    Ok(())
}

/// the sink `directory` names, made absolute from the working directory, once it
/// passes the checks.
pub fn checked_sink(directory: &str, checkout: Option<&Path>) -> Result<PathBuf, SinkError> {
    let path = std::path::absolute(directory)
        .map_err(|error| SinkError::Unreadable(PathBuf::from(directory), error.kind()))?;
    if let Some(checkout) = checkout {
        // the checkout may be named through a link, which the sink never is
        let resolved = fs::canonicalize(checkout).ok().map(without_verbatim_prefix);
        let named = path.to_string_lossy();
        if std::iter::once(checkout.to_path_buf())
            .chain(resolved)
            .any(|checkout| is_inside_checkout(&named, &checkout, PATH_RULES))
        {
            return Err(SinkError::InsideCheckout(path));
        }
    }
    check_directory(&path)?;
    Ok(path)
}

/// the path without the verbatim prefix `fs::canonicalize` gives it on Windows,
/// as a configuration names it.
#[cfg(windows)]
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    let plain = path.to_str().and_then(|text| {
        text.strip_prefix(r"\\?\UNC\")
            .map(|share| format!(r"\\{share}"))
            .or_else(|| text.strip_prefix(r"\\?\").map(str::to_string))
    });
    plain.map_or(path, PathBuf::from)
}

#[cfg(not(windows))]
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    path
}

/// checks a bundle again before a file goes into it, as the PowerShell exporter
/// does.
pub fn check_bundle(bundle: &Path) -> Result<(), SinkError> {
    check_directory(bundle)
}

fn bundle_prefix(pool: &str) -> String {
    format!("{pool}-bundle-")
}

/// how many entries of the sink belong to the pool's bundles, partial ones
/// included.
pub fn count_bundles(sink: &Path, pool: &str) -> io::Result<u32> {
    let prefix = bundle_prefix(pool);
    let mut count: u32 = 0;
    for entry in fs::read_dir(sink)? {
        if entry?.file_name().to_string_lossy().starts_with(&prefix) {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

/// 32 hexadecimal digits that differ for every call.
fn random_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let parts: Vec<u64> = (0..2)
        .map(|_| {
            RandomState::new().hash_one((
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now(),
            ))
        })
        .collect();
    parts.iter().map(|part| format!("{part:016x}")).collect()
}

/// creates a new bundle for the pool in the sink, `0700` on Unix, and checks it.
pub fn create_bundle(sink: &Path, pool: &str) -> Result<PathBuf, SinkError> {
    let bundle = sink.join(format!("{}{}", bundle_prefix(pool), random_id()));
    #[cfg(unix)]
    let created = {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .mode(crate::private::DIRECTORY_MODE)
            .create(&bundle)
            .and_then(|()| {
                // the mode is set again, since the creation mode is cut by the umask
                fs::set_permissions(
                    &bundle,
                    fs::Permissions::from_mode(crate::private::DIRECTORY_MODE),
                )
            })
    };
    #[cfg(not(unix))]
    let created = fs::DirBuilder::new().create(&bundle);
    created.map_err(|error| SinkError::Unreadable(bundle.clone(), error.kind()))?;
    check_bundle(&bundle)?;
    Ok(bundle)
}

/// creates a file that does not exist yet in the bundle, `0600` on Unix, after
/// checking the bundle again.
pub fn create_file(bundle: &Path, name: &str) -> io::Result<fs::File> {
    check_bundle(bundle).map_err(io::Error::other)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(crate::private::FILE_MODE);
    }
    options.open(bundle.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("sink-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        dir
    }

    #[test]
    fn a_missing_sink_is_refused_and_not_created() {
        let dir = scratch("missing");
        let missing = dir.join("missing");
        let error = checked_sink(missing.to_str().unwrap(), None).unwrap_err();
        assert_eq!(error, SinkError::Missing(missing.clone()));
        assert!(!missing.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_checkout_and_a_directory_inside_it_cannot_be_a_sink() {
        let dir = scratch("checkout");
        let inside = dir.join("inside");
        fs::create_dir(&inside).unwrap();
        for sink in [&dir, &inside] {
            assert_eq!(
                checked_sink(sink.to_str().unwrap(), Some(&dir)),
                Err(SinkError::InsideCheckout(sink.clone()))
            );
        }
        let sibling = PathBuf::from(format!("{}-out", dir.display()));
        assert!(
            checked_sink(sibling.to_str().unwrap(), Some(&dir))
                .is_err_and(|error| !matches!(error, SinkError::InsideCheckout(_)))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_sink_inside_a_checkout_named_through_a_link_is_refused() {
        let dir = scratch("checkout-link");
        let checkout = dir.join("checkout");
        let sink = checkout.join("evidence");
        fs::create_dir_all(&sink).unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        let named = link.join("checkout");
        assert_eq!(
            checked_sink(sink.to_str().unwrap(), Some(&named)),
            Err(SinkError::InsideCheckout(sink.clone()))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_verbatim_path_compares_as_a_configuration_names_it() {
        for (canonical, plain) in [
            (r"\\?\C:\marshal", r"C:\marshal"),
            (r"\\?\UNC\host\share\marshal", r"\\host\share\marshal"),
            (r"C:\marshal", r"C:\marshal"),
        ] {
            assert_eq!(
                without_verbatim_prefix(PathBuf::from(canonical)),
                PathBuf::from(plain)
            );
        }
    }

    #[test]
    fn a_bundle_counts_toward_the_pool_only_by_its_prefix() {
        let dir = scratch("count");
        for name in ["d-bundle-1", "d-bundle-partial", "dd-bundle-1", "other"] {
            fs::create_dir(dir.join(name)).unwrap();
        }
        assert_eq!(count_bundles(&dir, "d").unwrap(), 2);
        let bundle = create_bundle(&dir, "d").unwrap();
        let name = bundle.file_name().unwrap().to_string_lossy().into_owned();
        let id = name.strip_prefix("d-bundle-").unwrap();
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(count_bundles(&dir, "d").unwrap(), 3);
        assert_ne!(create_bundle(&dir, "d").unwrap(), bundle);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn on_unix_a_sink_must_be_private_and_a_bundle_and_its_files_are() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = scratch("private");
        let sink = dir.join("sink");
        fs::create_dir(&sink).unwrap();
        fs::set_permissions(&sink, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            checked_sink(sink.to_str().unwrap(), None),
            Err(SinkError::NotPrivate(
                crate::private::PrivateError::WrongMode { .. }
            ))
        ));
        assert!(
            fs::read_dir(&sink).unwrap().next().is_none(),
            "nothing is written"
        );
        fs::set_permissions(&sink, fs::Permissions::from_mode(0o700)).unwrap();
        let checked = checked_sink(sink.to_str().unwrap(), None).unwrap();
        let bundle = create_bundle(&checked, "d").unwrap();
        assert_eq!(fs::metadata(&bundle).unwrap().mode() & 0o7777, 0o700);
        drop(create_file(&bundle, "identity.txt").unwrap());
        assert_eq!(
            fs::metadata(bundle.join("identity.txt")).unwrap().mode() & 0o7777,
            0o600
        );
        assert!(
            create_file(&bundle, "identity.txt").is_err(),
            "a file is never replaced"
        );
        let owner = fs::metadata(&sink).unwrap().uid();
        assert!(matches!(
            crate::private::check_private_directory(&sink, owner + 1),
            Err(crate::private::PrivateError::OwnedByAnother { .. })
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_sink_that_is_or_lies_under_a_link_is_refused() {
        let dir = scratch("link");
        let sink = dir.join("sink");
        fs::create_dir(&sink).unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        let sink_link = dir.join("sink-link");
        std::os::unix::fs::symlink(&sink, &sink_link).unwrap();
        assert_eq!(
            checked_sink(sink_link.to_str().unwrap(), None),
            Err(SinkError::Link(sink_link.clone()))
        );
        let under = link.join("sink");
        assert_eq!(
            checked_sink(under.to_str().unwrap(), None),
            Err(SinkError::Link(link.clone()))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    /// a directory junction, which needs no privilege, made by `mklink /J`.
    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_a_sink_that_is_or_lies_under_a_reparse_point_is_refused() {
        let dir = scratch("junction");
        let sink = dir.join("sink");
        fs::create_dir(&sink).unwrap();
        assert_eq!(checked_sink(sink.to_str().unwrap(), None), Ok(sink.clone()));
        let sink_junction = dir.join("sink-junction");
        junction(&sink_junction, &sink);
        assert_eq!(
            checked_sink(sink_junction.to_str().unwrap(), None),
            Err(SinkError::Link(sink_junction.clone()))
        );
        let parent = dir.join("parent");
        junction(&parent, &dir);
        let under = parent.join("sink");
        assert_eq!(
            checked_sink(under.to_str().unwrap(), None),
            Err(SinkError::Link(parent.clone()))
        );
        // a symbolic link needs the privilege to create one, which CI runners have
        let link = dir.join("sink-link");
        std::os::windows::fs::symlink_dir(&sink, &link).unwrap();
        assert_eq!(
            checked_sink(link.to_str().unwrap(), None),
            Err(SinkError::Link(link.clone()))
        );
        for reparse in [&sink_junction, &parent, &link] {
            fs::remove_dir(reparse).unwrap();
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
