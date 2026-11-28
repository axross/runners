#!/usr/bin/env bash
# prints one file, line range or directory listing of this repository at a
# commit, read through GET requests to the contents API. the Claude CI review
# workflow copies it out of the workspace with its placeholders filled in, and
# the reviewer runs that root-owned copy, so a head file is read without
# checking the head out; docs/security/workflow-permissions.md says how.
set -euo pipefail
# a path character is judged byte by byte, so a multibyte one is refused
export LC_ALL=C

repository='@REPOSITORY@'
gh='@GH@'
jq='@JQ@'
base64='@BASE64@'
sed='@SED@'
wc='@WC@'
PATH='/usr/bin:/bin'

usage='usage: read-head <40-character lowercase commit SHA> <path> [<first line> <last line>]'
max_path_length=1024
max_line_digits=9
# the contents API lists at most this many entries of a directory
listing_limit=1000

# refuse leaves before any request is made
refuse() {
  echo "read-head: $*" >&2
  exit 2
}

fail() {
  echo "read-head: $*" >&2
  exit 1
}

case $repository in
  @*) refuse "this is the unconfigured source; run the copy the workflow installs" ;;
esac

[[ $# -eq 2 || $# -eq 4 ]] || refuse "$usage"
sha=$1
path=$2

[[ ${#sha} -eq 40 ]] || refuse "the commit must be 40 lowercase hexadecimal characters"
case $sha in
  *[!0-9a-f]*) refuse "the commit must be 40 lowercase hexadecimal characters" ;;
esac

validate_path() {
  local rest segment
  [[ -n $path ]] || refuse "the path is empty"
  [[ ${#path} -le $max_path_length ]] || refuse "the path is longer than ${max_path_length} characters"
  [[ $path != -* ]] || refuse "the path must not begin with -"
  case $path in
    *[!A-Za-z0-9._/-]*) refuse "the path may hold only ASCII letters, digits, . _ - and /" ;;
  esac
  [[ $path != . ]] || return 0
  rest=$path
  while :; do
    segment=${rest%%/*}
    case $segment in
      '') refuse "the path must be relative, with no empty segment or trailing /" ;;
      . | ..) refuse "the path must not hold a . or .. segment" ;;
    esac
    [[ $rest == */* ]] || break
    rest=${rest#*/}
  done
}

# a line number is a positive decimal integer with no leading zero
validate_line() {
  case $1 in
    '' | 0* | *[!0-9]*) refuse "a line number must be a positive decimal integer without leading zeros" ;;
  esac
  [[ ${#1} -le $max_line_digits ]] || refuse "a line number may have at most ${max_line_digits} digits"
}

validate_path

ranged=false
if [[ $# -eq 4 ]]; then
  first=$3
  last=$4
  validate_line "$first"
  validate_line "$last"
  [[ $first -le $last ]] || refuse "the first line is greater than the last"
  [[ $path != . ]] || refuse "a line range cannot be read from a directory"
  ranged=true
fi

api_path=
[[ $path == . ]] || api_path="/${path}"

# prints the response body, which a failed request leaves empty
request() {
  "$gh" api --method GET --hostname github.com \
    --header "Accept: $1" \
    "repos/${repository}/contents${api_path}?ref=${sha}"
}

object_type=application/vnd.github.object+json
raw_type=application/vnd.github.raw

# a name or target is data from the repository, so a control character or a
# backslash in it is written as \xNN: one entry cannot span lines, and an escape
# cannot be mistaken for a literal backslash sequence
escape='def escape:
  explode
  | map(if . < 32 or . == 92 or (. >= 127 and . <= 159)
      then ("\\x" + ("0123456789abcdef"[(. / 16 | floor):(. / 16 | floor) + 1] + "0123456789abcdef"[(. % 16):(. % 16) + 1]) | explode)
      else [.] end)
  | (add // [])
  | implode;'

field() {
  printf '%s' "$object" | "$jq" -r "$escape $1" || fail "the response could not be read"
}

object=$(request "$object_type") || fail "the request for ${path} at ${sha} failed"
kind=$(field '.type // empty | escape')

# prints the decoded file from the base64 text in $encoded
decode() {
  printf '%s' "$encoded" | "$base64" --decode
}

print_file() {
  local size encoding decoded_size total printed_last
  size=$(field '.size // empty | tostring')
  case $size in
    '' | *[!0-9]*) fail "the response gave no size for the file" ;;
  esac
  encoding=$(field '.encoding // empty | escape')
  if [[ $encoding == base64 ]]; then
    encoded=$(printf '%s' "$object" | "$jq" -j '.content // ""') || fail "the response could not be read"
  else
    encoded=$(request "$raw_type" | "$base64" --wrap=0) || fail "the second request for ${path} at ${sha} failed"
  fi
  decoded_size=$(decode | "$wc" --bytes) || fail "the file's content could not be decoded"
  [[ $decoded_size -eq $size ]] || fail "the file's bytes do not match its reported size of ${size}"
  if [[ $ranged == false ]]; then
    echo "read-head: ${size} bytes" >&2
    decode
    return 0
  fi
  total=$(decode | "$sed" -n '$=')
  total=${total:-0}
  [[ $first -le $total ]] || fail "the file has ${total} lines, so it has no line ${first}"
  printed_last=$last
  if [[ $last -gt $total ]]; then
    printed_last=$total
    echo "read-head: ${size} bytes; the file ends at line ${total}, so lines ${first} to ${total} are printed" >&2
  else
    echo "read-head: ${size} bytes; lines ${first} to ${last} of ${total}" >&2
  fi
  decode | "$sed" -n "${first},${printed_last}p"
}

print_directory() {
  local count listing
  count=$(field '.entries | if type == "array" then length else error("no entries") end')
  listing=$(field '.entries[] | "\(.type | escape)\t\(.name | escape)"')
  echo "read-head: ${count} entries" >&2
  if [[ $count -ge $listing_limit ]]; then
    echo "read-head: warning: the listing reached the API's limit of ${listing_limit} entries and may be incomplete" >&2
  fi
  printf '%s\n' "$listing"
}

[[ $ranged == false || $kind == file ]] || refuse "a line range can be read only from a file"

case $kind in
  file) print_file ;;
  dir) print_directory ;;
  symlink)
    echo "read-head: a symlink the API did not resolve" >&2
    printf 'symlink\t%s\n' "$(field '.target // empty | escape')"
    ;;
  submodule)
    echo "read-head: a submodule" >&2
    printf 'submodule\t%s\n' "$(field '.sha // empty | escape')"
    ;;
  *) fail "the API returned an unknown type for ${path}" ;;
esac
