#!/usr/bin/env bash
# a stand-in for the `gh` calls the documented install blocks make: a release
# download, the tag and compare API reads, and an attestation verify. the install
# block tests copy it to the front of PATH as `gh`. it records each call in
# $STANDIN_LOG and serves $STANDIN_RELEASE. an API read has a canned response body,
# built from the STANDIN_* variables below, and the caller's --jq expression is
# applied to it with jq, as gh does, so a block with a wrong expression gets a
# wrong value and fails its suite. it never contacts GitHub.
echo "gh $*" >>"$STANDIN_LOG"

jq_expression=""
previous=""
for argument in "$@"; do
  [[ "$previous" != --jq ]] || jq_expression="$argument"
  previous="$argument"
done

# respond <body>: prints the body, or what the --jq expression makes of it. jq -r
# prints a string raw, as gh does.
respond() {
  if [[ -z "$jq_expression" ]]; then
    printf '%s\n' "$1"
    return 0
  fi
  command -v jq >/dev/null || {
    echo "standin-gh: jq is required to apply --jq" >&2
    exit 98
  }
  printf '%s' "$1" | jq -r "$jq_expression" || exit 1
}

case "$1 $2" in
  "release download")
    patterns=() dir=.
    shift 2
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --pattern) patterns+=("$2"); shift 2 ;;
        --dir) dir="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    [[ -z "${STANDIN_DOWNLOAD_FAIL:-}" ]] || exit 1
    for pattern in "${patterns[@]}"; do
      /bin/cp "${STANDIN_RELEASE}/${pattern}" "${dir}/${pattern}" || exit 1
    done
    ;;
  "api repos/axross/runners/git/ref/tags/"*)
    [[ -z "${STANDIN_TAG_FAIL:-}" ]] || exit 1
    respond "{\"ref\":\"refs/tags/v0\",\"object\":{\"type\":\"${STANDIN_TAG_TYPE:-commit}\",\"sha\":\"${STANDIN_TAG_SHA}\"}}"
    ;;
  "api repos/axross/runners/git/tags/"*)
    [[ -z "${STANDIN_ANNOTATED_FAIL:-}" ]] || exit 1
    respond "{\"tag\":\"v0\",\"object\":{\"type\":\"commit\",\"sha\":\"${STANDIN_ANNOTATED_SHA:-$STANDIN_TAG_SHA}\"}}"
    ;;
  "api repos/axross/runners/compare/"*)
    [[ -z "${STANDIN_COMPARE_FAIL:-}" ]] || exit 1
    respond "{\"status\":\"${STANDIN_COMPARE:-identical}\",\"ahead_by\":0,\"behind_by\":0}"
    ;;
  "attestation verify")
    [[ -z "${STANDIN_ATTEST_FAIL:-}" ]] || exit 1
    ;;
  *) exit 99 ;;
esac
