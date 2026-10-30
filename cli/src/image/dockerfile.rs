//! the little of the Dockerfile syntax the contract tests read; it is test-only, since
//! the binary never parses the Dockerfile at run time.

/// the instructions, each as its upper-cased keyword and its arguments, with
/// continued lines joined and comment lines dropped, as Docker reads them.
pub fn instructions(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || (pending.is_empty() && trimmed.is_empty()) {
            continue;
        }
        match trimmed.strip_suffix('\\') {
            Some(continued) => {
                pending.push_str(continued);
                pending.push(' ');
            }
            None => {
                pending.push_str(trimmed);
                let (keyword, arguments) = pending
                    .split_once(char::is_whitespace)
                    .unwrap_or((&pending, ""));
                found.push((keyword.to_ascii_uppercase(), arguments.trim().to_string()));
                pending.clear();
            }
        }
    }
    found
}

/// the image the first stage starts from, as its `FROM` names it.
pub fn base_reference(text: &str) -> Option<String> {
    let (_, arguments) = instructions(text)
        .into_iter()
        .find(|(keyword, _)| keyword == "FROM")?;
    arguments
        .split_whitespace()
        .find(|word| !word.starts_with("--"))
        .map(str::to_string)
}

/// the tag of an image reference such as `name:tag@sha256:...`.
pub fn reference_tag(reference: &str) -> Option<&str> {
    let named = reference.split('@').next()?;
    let last = named.rsplit('/').next()?;
    last.split_once(':').map(|(_, tag)| tag)
}

/// the build-context files the `COPY` and `ADD` instructions read, every source of
/// one without `--from`; the JSON form, a wildcard, an absolute path or one that
/// leaves the context is an error.
pub fn local_copy_sources(text: &str) -> Result<Vec<String>, String> {
    let mut sources = Vec::new();
    for (keyword, arguments) in instructions(text) {
        if keyword != "COPY" && keyword != "ADD" {
            continue;
        }
        if arguments.starts_with('[') {
            return Err(format!("{keyword} {arguments}: the JSON form is not read"));
        }
        let words: Vec<&str> = arguments.split_whitespace().collect();
        if words.iter().any(|word| word.starts_with("--from=")) {
            continue;
        }
        let paths: Vec<&str> = words
            .into_iter()
            .filter(|word| !word.starts_with("--"))
            .collect();
        let Some((_, read)) = paths.split_last() else {
            return Err(format!("{keyword} {arguments}: no destination"));
        };
        for source in read {
            if source.contains(['*', '?', '['])
                || source.starts_with('/')
                || source.split('/').any(|part| part == "..")
            {
                return Err(format!("{keyword} {arguments}: {source} cannot be checked"));
            }
            sources.push(source.trim_start_matches("./").to_string());
        }
    }
    Ok(sources)
}

/// the labels the final stage declares, as `key="value"` pairs, in order.
pub fn final_stage_labels(text: &str) -> Vec<(String, String)> {
    let all = instructions(text);
    let final_stage = all
        .iter()
        .rposition(|(keyword, _)| keyword == "FROM")
        .map_or(&all[..], |start| &all[start..]);
    final_stage
        .iter()
        .filter(|(keyword, _)| keyword == "LABEL")
        .flat_map(|(_, arguments)| {
            arguments
                .split_whitespace()
                .filter_map(|pair| pair.split_once('='))
                .map(|(key, value)| (key.to_string(), value.trim_matches('"').to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# syntax comment
FROM --platform=linux/amd64 example.org/base:1.2.3@sha256:abc AS base

FROM base AS tools
COPY tool.sh /tmp/tool.sh
LABEL early=\"no\"

FROM base
# the labels
LABEL first=\"1\" \\
      second=\"two\"
COPY --from=tools /tmp /tmp
ADD --chown=a:b ./data.txt other.txt /srv/
";

    #[test]
    fn continued_lines_join_and_comments_drop() {
        let found = instructions(SAMPLE);
        assert_eq!(found.len(), 8);
        assert_eq!(
            found[5],
            (
                "LABEL".to_string(),
                "first=\"1\"  second=\"two\"".to_string()
            )
        );
    }

    #[test]
    fn the_base_reference_skips_the_flags_and_its_tag_is_read() {
        let base = base_reference(SAMPLE).unwrap();
        assert_eq!(base, "example.org/base:1.2.3@sha256:abc");
        assert_eq!(reference_tag(&base), Some("1.2.3"));
        assert_eq!(reference_tag("localhost:5000/base@sha256:abc"), None);
    }

    #[test]
    fn the_local_sources_skip_copies_from_a_stage() {
        assert_eq!(
            local_copy_sources(SAMPLE).unwrap(),
            ["tool.sh", "data.txt", "other.txt"]
        );
        assert!(local_copy_sources("FROM a\nCOPY *.sh /tmp/\n").is_err());
        assert!(local_copy_sources("FROM a\nCOPY [\"a\", \"/b\"]\n").is_err());
        assert!(local_copy_sources("FROM a\nCOPY ../a /b\n").is_err());
    }

    #[test]
    fn only_the_final_stage_labels_are_read() {
        assert_eq!(
            final_stage_labels(SAMPLE),
            [
                ("first".to_string(), "1".to_string()),
                ("second".to_string(), "two".to_string())
            ]
        );
    }
}
