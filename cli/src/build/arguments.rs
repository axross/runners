//! the `docker` argument lists of `build`, and the temporary tag it builds under.

use crate::image::contract::{BUILT_LABEL, IMAGE_LABEL, IMAGE_LABEL_VALUE};
use crate::runtime::docker_args::strings;
use crate::runtime::name::{DIGITS_IN_STAMP, stamp};

const TEMPORARY_TAG_PREFIX: &str = "marshal-build-";

fn image_label() -> String {
    format!("{IMAGE_LABEL}={IMAGE_LABEL_VALUE}")
}

/// the image name without its tag, so a registry port stays.
#[must_use]
pub fn repository_of(image: &str) -> &str {
    let last_part_start = image.rfind('/').map_or(0, |slash| slash + 1);
    let tag_colon = image
        .get(last_part_start..)
        .and_then(|last| last.rfind(':'))
        .map(|colon| last_part_start + colon);
    tag_colon
        .and_then(|colon| image.get(..colon))
        .unwrap_or(image)
}

/// the tag a build made at `time` builds under: `<repository>:marshal-build-<stamp>`,
/// the stamp in UTC.
#[must_use]
pub fn temporary_tag(image: &str, time: std::time::SystemTime) -> String {
    format!(
        "{}:{TEMPORARY_TAG_PREFIX}{}",
        repository_of(image),
        stamp(time)
    )
}

/// whether `reference` is a temporary tag of `image`'s repository, and nothing
/// looser.
#[must_use]
pub fn is_temporary_tag(image: &str, reference: &str) -> bool {
    reference
        .strip_prefix(repository_of(image))
        .and_then(|rest| rest.strip_prefix(':'))
        .and_then(|tag| tag.strip_prefix(TEMPORARY_TAG_PREFIX))
        .is_some_and(|digits| {
            digits.len() == DIGITS_IN_STAMP && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// lists the temporary tags of `image`'s repository that carry the image label.
#[must_use]
pub fn leftover_list_arguments(image: &str) -> Vec<String> {
    strings(&[
        "image",
        "ls",
        "--filter",
        &format!("reference={}:{TEMPORARY_TAG_PREFIX}*", repository_of(image)),
        "--filter",
        &format!("label={}", image_label()),
        "--format",
        "{{.Repository}}:{{.Tag}}",
    ])
}

/// the arguments of the one `docker build`.
#[must_use]
pub fn build_arguments(temporary: &str, built: u64) -> Vec<String> {
    strings(&[
        "build",
        "--pull",
        "--no-cache",
        "--label",
        &image_label(),
        "--label",
        &format!("{BUILT_LABEL}={built}"),
        "--tag",
        temporary,
        "-",
    ])
}

#[must_use]
pub fn tag_arguments(source: &str, target: &str) -> Vec<String> {
    strings(&["tag", source, target])
}

#[must_use]
pub fn image_remove_arguments(reference: &str) -> Vec<String> {
    strings(&["image", "rm", reference])
}

/// removes the dangling images that carry the image label.
#[must_use]
pub fn prune_arguments() -> Vec<String> {
    strings(&[
        "image",
        "prune",
        "--force",
        "--filter",
        &format!("label={}", image_label()),
    ])
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn the_repository_is_the_name_before_the_tag_after_the_last_slash() {
        assert_eq!(repository_of("actions-runner:local"), "actions-runner");
        assert_eq!(repository_of("actions-runner"), "actions-runner");
        assert_eq!(
            repository_of("localhost:5000/team/runner:v2"),
            "localhost:5000/team/runner"
        );
        assert_eq!(
            repository_of("localhost:5000/team/runner"),
            "localhost:5000/team/runner"
        );
    }

    #[test]
    fn the_temporary_tag_is_the_repository_and_a_utc_stamp() {
        let time = UNIX_EPOCH + Duration::from_millis(1_709_618_828_009);
        assert_eq!(
            temporary_tag("localhost:5000/runner:local", time),
            "localhost:5000/runner:marshal-build-20240305060708009"
        );
        assert_eq!(
            temporary_tag("runner", time),
            "runner:marshal-build-20240305060708009"
        );
    }

    #[test]
    fn only_a_temporary_tag_of_the_same_repository_is_one() {
        let image = "localhost:5000/runner:local";
        assert!(is_temporary_tag(
            image,
            "localhost:5000/runner:marshal-build-20240305060708009"
        ));
        for other in [
            "localhost:5000/runner:marshal-build-2024030506070800",
            "localhost:5000/runner:marshal-build-20240305060708009x",
            "localhost:5000/runner:local",
            "localhost:5000/runner-other:marshal-build-20240305060708009",
            "other/localhost:5000/runner:marshal-build-20240305060708009",
        ] {
            assert!(!is_temporary_tag(image, other), "{other}");
        }
    }

    #[test]
    fn the_build_passes_both_labels_the_tag_and_the_context_on_standard_input() {
        assert_eq!(
            build_arguments("r:marshal-build-1", 42),
            [
                "build",
                "--pull",
                "--no-cache",
                "--label",
                "com.github.axross.runners.image=actions-runner",
                "--label",
                "com.github.axross.runners.built=42",
                "--tag",
                "r:marshal-build-1",
                "-"
            ]
        );
        assert_eq!(
            prune_arguments(),
            [
                "image",
                "prune",
                "--force",
                "--filter",
                "label=com.github.axross.runners.image=actions-runner"
            ]
        );
    }
}
