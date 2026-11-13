//! where Marshal runs from on macOS: it starts and controls an agent only from a bundle
//! directly inside `/Applications` or `~/Applications`. The rule reads the app's own
//! path alone, so a translocated copy, which runs from a random folder, is refused with
//! no translocation check, for which Apple offers no supported way.

use std::path::Path;

/// the variable that lets a debug build run from anywhere, for the tests; a release
/// build ignores it.
pub const ANY_LOCATION_VARIABLE: &str = "MARSHAL_TEST_ANY_LOCATION";

/// whether the executable at `executable` is the one of a bundle directly inside
/// `/Applications` or `home`'s `Applications`: `<folder>/<name>.app/Contents/MacOS/<file>`.
#[must_use]
pub fn in_applications(executable: &Path, home: Option<&Path>) -> bool {
    let mut up = executable.ancestors().skip(1);
    let (Some(macos), Some(contents), Some(bundle), Some(folder)) =
        (up.next(), up.next(), up.next(), up.next())
    else {
        return false;
    };
    let named = |path: &Path, name: &str| path.file_name().is_some_and(|found| found == name);
    let is_bundle = bundle
        .extension()
        .is_some_and(|extension| extension == "app");
    let allowed = folder == Path::new("/Applications")
        || home.is_some_and(|home| folder == home.join("Applications"));
    named(macos, "MacOS") && named(contents, "Contents") && is_bundle && allowed
}

/// whether this process may start and control an agent: always off macOS, and on macOS
/// only from Applications, or in a debug build that a test lets run anywhere.
#[must_use]
pub fn may_control_agent(executable: &Path) -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    if cfg!(debug_assertions) && std::env::var_os(ANY_LOCATION_VARIABLE).is_some() {
        return true;
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    in_applications(executable, home.as_deref())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::in_applications;

    #[test]
    fn the_profile_is_the_one_ci_asked_for() {
        let debug_profile = cfg!(debug_assertions);
        match std::env::var("MARSHAL_EXPECT_RELEASE").as_deref() {
            Ok("1") => assert!(!debug_profile, "this test was built in the debug profile"),
            Ok("0") => assert!(debug_profile, "this test was built in the release profile"),
            _ => {}
        }
    }

    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    #[test]
    fn a_release_build_ignores_the_variable() {
        use super::{ANY_LOCATION_VARIABLE, may_control_agent};

        // CI's release step sets it, so the test fails if a release build reads it
        if std::env::var("MARSHAL_EXPECT_RELEASE").as_deref() == Ok("1") {
            assert!(
                std::env::var_os(ANY_LOCATION_VARIABLE).is_some(),
                "the release step sets {ANY_LOCATION_VARIABLE}"
            );
        }
        let downloads = Path::new("/Users/someone/Downloads/Marshal.app/Contents/MacOS/m");
        assert!(!may_control_agent(downloads));
    }

    const HOME: &str = "/Users/someone";

    fn allowed(path: &str) -> bool {
        in_applications(Path::new(path), Some(Path::new(HOME)))
    }

    #[test]
    fn a_bundle_directly_in_either_applications_folder_is_allowed() {
        assert!(allowed(
            "/Applications/Marshal.app/Contents/MacOS/marshal-app"
        ));
        assert!(allowed(
            "/Users/someone/Applications/Marshal.app/Contents/MacOS/marshal-app"
        ));
    }

    #[test]
    fn every_other_place_is_refused() {
        for path in [
            // the mounted disk image
            "/Volumes/Marshal/Marshal.app/Contents/MacOS/marshal-app",
            // a translocated copy
            "/private/var/folders/x/T/AppTranslocation/ABCD/d/Marshal.app/Contents/MacOS/marshal-app",
            "/Users/someone/Downloads/Marshal.app/Contents/MacOS/marshal-app",
            // a subfolder of Applications
            "/Applications/Tools/Marshal.app/Contents/MacOS/marshal-app",
            // another user's Applications
            "/Users/other/Applications/Marshal.app/Contents/MacOS/marshal-app",
            // not a bundle's executable
            "/Applications/marshal-app",
            "/Applications/Marshal/Contents/MacOS/marshal-app",
            "/Applications/Marshal.app/Contents/Resources/marshal-app",
            "/Applications/Marshal.app/marshal-app",
        ] {
            assert!(!allowed(path), "{path}");
        }
    }

    #[test]
    fn without_a_home_only_the_system_folder_is_allowed() {
        let user = Path::new("/Users/someone/Applications/Marshal.app/Contents/MacOS/m");
        assert!(!in_applications(user, None));
        let system = Path::new("/Applications/Marshal.app/Contents/MacOS/m");
        assert!(in_applications(system, None));
    }
}
