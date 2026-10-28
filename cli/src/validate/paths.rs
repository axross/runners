//! path forms of the two hosts, and the rule that private paths lie outside the
//! checkout.

use std::path::Path;

use crate::fold;
use crate::profile::Profile;

/// reports whether the text is an absolute POSIX path: it starts with `/`, holds no
/// control character, and has no empty, `.` or `..` component.
#[must_use]
pub fn is_absolute_posix(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('/') else {
        return false;
    };
    !text.chars().any(char::is_control)
        && rest
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// reports whether the path has a `.` or `..` component between backslashes.
#[must_use]
pub fn has_windows_dot_component(text: &str) -> bool {
    text.split('\\').any(|part| part == "." || part == "..")
}

/// reports whether `directory` is the checkout or lies inside it, comparing
/// lexically with repeated separators collapsed and `.` and `..` resolved, never
/// following a link, and ignoring case on axpc only.
#[must_use]
pub fn is_inside_checkout(directory: &str, checkout: &Path, profile: Profile) -> bool {
    let separator = match profile {
        Profile::Axpc => '\\',
        Profile::Axmc => '/',
    };
    let directory = normalize(directory, separator);
    let checkout = normalize(&checkout.to_string_lossy(), separator);
    let prefix = format!("{checkout}{separator}");
    match profile {
        Profile::Axpc => {
            fold::eq_folded(&directory, &checkout) || fold::starts_with_folded(&directory, &prefix)
        }
        Profile::Axmc => directory == checkout || directory.starts_with(&prefix),
    }
}

/// collapses repeated separators, resolves `.` and `..` components lexically, and
/// drops a trailing separator; a leading separator is kept.
fn normalize(path: &str, separator: char) -> String {
    let rooted = path.starts_with(separator);
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(separator).filter(|part| !part.is_empty()) {
        match part {
            "." => {}
            ".." => {
                // the drive of a Windows path is never popped
                let floor = usize::from(separator == '\\' && !rooted);
                if parts.len() > floor {
                    parts.pop();
                }
            }
            _ => parts.push(part),
        }
    }
    let joined = parts.join(&separator.to_string());
    if rooted {
        format!("{separator}{joined}")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_paths_are_absolute_without_empty_or_dot_components() {
        for good in ["/a", "/a/b.token", "/a b/c", "/..a/b", "/a/.b"] {
            assert!(is_absolute_posix(good), "{good}");
        }
        for bad in [
            "", "/", "a", "a/b", "/a/", "//a", "/a//b", "/a/./b", "/a/..", "/a/../b", "/a\nb",
            "/a\u{7f}", "C:\\a",
        ] {
            assert!(!is_absolute_posix(bad), "{bad:?}");
        }
    }

    #[test]
    fn dot_components_sit_between_backslashes() {
        assert!(has_windows_dot_component("C:\\a\\.\\b"));
        assert!(has_windows_dot_component("C:\\a\\.."));
        assert!(!has_windows_dot_component("C:\\a\\..b"));
        assert!(!has_windows_dot_component("C:\\a.\\b"));
    }

    #[test]
    fn the_checkout_rule_is_lexical_and_respects_the_profile_case_rule() {
        let checkout = Path::new("/path/to/checkout");
        assert!(is_inside_checkout(
            "/path/to/checkout",
            checkout,
            Profile::Axmc
        ));
        assert!(is_inside_checkout(
            "/path/to/checkout/",
            checkout,
            Profile::Axmc
        ));
        assert!(is_inside_checkout(
            "/path/to//checkout/out",
            checkout,
            Profile::Axmc
        ));
        assert!(!is_inside_checkout(
            "/path/to/checkout-out",
            checkout,
            Profile::Axmc
        ));
        assert!(!is_inside_checkout("/path/to", checkout, Profile::Axmc));
        assert!(!is_inside_checkout(
            "/path/to/Checkout/out",
            checkout,
            Profile::Axmc
        ));
    }

    #[test]
    fn windows_directories_compare_without_case_and_resolve_dot_components() {
        let checkout = Path::new("C:\\path\\to\\checkout");
        assert!(is_inside_checkout(
            "c:\\PATH\\to\\CHECKOUT",
            checkout,
            Profile::Axpc
        ));
        assert!(is_inside_checkout(
            "C:\\path\\to\\checkout\\out\\",
            checkout,
            Profile::Axpc
        ));
        assert!(is_inside_checkout(
            "C:\\path\\to\\x\\..\\checkout\\out",
            checkout,
            Profile::Axpc
        ));
        assert!(is_inside_checkout(
            "C:\\path\\\\to\\checkout\\out",
            checkout,
            Profile::Axpc
        ));
        assert!(!is_inside_checkout(
            "C:\\path\\to\\checkout-out",
            checkout,
            Profile::Axpc
        ));
        assert!(!is_inside_checkout(
            "D:\\path\\to\\checkout",
            checkout,
            Profile::Axpc
        ));
    }
}
