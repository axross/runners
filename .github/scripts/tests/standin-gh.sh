#!/usr/bin/env bash
# a stand-in for the `gh` calls the documented install blocks make: a release
# download, the tag and compare API reads, and an attestation verify. the install
# block tests copy it to the front of PATH as `gh`. it records each call in
# $STANDIN_LOG and serves $STANDIN_RELEASE; the STANDIN_* variables below choose
# what the API reads return and which calls fail. it never contacts GitHub.
echo "gh $*" >>"$STANDIN_LOG"
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
    echo "${STANDIN_TAG_TYPE:-commit} ${STANDIN_TAG_SHA}"
    ;;
  "api repos/axross/runners/git/tags/"*)
    [[ -z "${STANDIN_ANNOTATED_FAIL:-}" ]] || exit 1
    echo "commit ${STANDIN_ANNOTATED_SHA:-$STANDIN_TAG_SHA}"
    ;;
  "api repos/axross/runners/compare/"*)
    [[ -z "${STANDIN_COMPARE_FAIL:-}" ]] || exit 1
    echo "${STANDIN_COMPARE:-identical}"
    ;;
  "attestation verify")
    [[ -z "${STANDIN_ATTEST_FAIL:-}" ]] || exit 1
    ;;
  *) exit 99 ;;
esac
