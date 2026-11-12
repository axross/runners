#!/usr/bin/env bash
# writes, or with `check` compares, the licence notices of the app's executable for the
# Windows and the macOS tree: what cargo-about finds for every crate the app links or
# runs as a procedural macro, ring's licence file and the copyright notices in its
# source, then the files kept in this repository for what is not a crate. cargo-about is the version
# mise.toml pins, and its version is in each file, so another version fails the check.
#
# usage: generate.sh write | check
set -euo pipefail

mode="${1:-}"
if [[ $# -ne 1 || ( "$mode" != "write" && "$mode" != "check" ) ]]; then
  echo "Usage: generate.sh write | check" >&2
  exit 2
fi

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(dirname "$here")"
frontend="${app}/crates/app/frontend"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

# prints a licence file this repository keeps, and fails the run when it is missing or
# empty, so a notice cannot ship as a heading over nothing.
licence() {
  if [[ ! -s "$1" ]]; then
    echo "error: app/${1#"${app}"/} is missing or empty; a notice needs its licence text." >&2
    exit 1
  fi
  cat "$1"
}

# the template line cargo-about prints for a crate whose own copyright line it has not
# got, which --fail lets through; the finished notices must not hold it.
template="Copyright (c) <year> <copyright holders>"

# prints cargo-about's output $1 with the two groups of crates that ship no licence file
# given what they have instead of the template line: the webview2-rs crates take the
# copyright line of the repository's copy of their upstream licence, once the copy's
# words are the licence cargo-about printed, and the objc2 crates at exactly the versions
# listed below, whose licence at the commits they were published from names no
# copyright holder, have the licence without the line, after a sentence saying so. Any other crate keeps the template line, which
# the run refuses.
own_copyright_lines() {
  local copy="${here}/licences/webview2-com-sys-LICENSE.txt"
  licence "$copy" >/dev/null
  awk -v template="$template" \
    -v objc2_versions="block2 0.6.2,dispatch2 0.3.1,objc2 0.6.5,objc2-app-kit 0.3.2,objc2-core-foundation 0.3.2,objc2-core-graphics 0.3.2,objc2-encode 4.1.0,objc2-exception-helper 0.1.1,objc2-foundation 0.3.2,objc2-quartz-core 0.3.2,objc2-web-kit 0.3.2" \
    -v objc2="These crate versions ship no licence file, and the licence in their repository at the commits they were published from names no copyright holder; a later licence there, from commit ee9a7ad, names one." '
    BEGIN {
      count = split(objc2_versions, pairs, ",")
      for (i = 1; i <= count; i++) checked[pairs[i]] = 1
    }
    function words(text) {
      gsub(/[ \t\r]+/, " ", text)
      sub(/^ /, "", text)
      sub(/ $/, "", text)
      return text
    }
    function flush(   i, f, k, kind, line, start, text) {
      for (i = 1; i <= n; i++) if (block[i] == template) line = i
      for (i = 1; i <= n; i++) if (block[i] ~ /^  \* /) {
        split(block[i], f, " ")
        k = f[2] ~ /^webview2-com(-macros|-sys)?$/ ? "webview2" : \
          ((f[2] " " f[3]) in checked ? "objc2" : "other")
        kind = (kind == "" || kind == k) ? k : "mixed"
        start = i
      }
      if (line) {
        for (start++; start <= n && block[start] == ""; start++) {}
        if (kind == "webview2") {
          for (i = start; i <= n; i++) if (i != line) text = text " " block[i]
          if (words(text) == words(copy)) block[line] = holder
          else print "error: app/notices/licences/webview2-com-sys-LICENSE.txt no longer holds the licence cargo-about names for the webview2-rs crates." > "/dev/stderr"
        }
      }
      for (i = 1; i <= n; i++) {
        if (line && kind == "objc2") {
          if (i == start) { print objc2; print "" }
          if (i == line || (i == line + 1 && block[i] == "")) continue
        }
        print block[i]
      }
      n = 0
    }
    FNR == NR {
      if ($0 ~ /^Copyright /) holder = $0
      else copy = copy " " $0
      next
    }
    /^=+$/ && length($0) >= 40 { flush(); print; next }
    { block[++n] = $0 }
    END { flush() }
  ' "$copy" "$1"
}

# fails the run unless every crate about.toml clarifies, in cargo-about's output $1,
# has its own copyright line, held below as the SHA-256 of the line without its
# surrounding blanks so this script names no one, and appears under no more licence
# texts than its clarification names files (one, for a choice of licences). cargo-about drops a
# clarification whose checksum no longer matches with only a warning, and the crate
# then falls back to the files it finds, which this notices.
# sha256sum is in GNU coreutils; macOS without it has shasum, which prints the same.
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum; else shasum -a 256; fi
}

clarified() {
  local hashes="${out}/copyright-lines.txt"
  { grep -h 'Copyright' "$1" || true; } | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//' |
    LC_ALL=C sort -u | while IFS= read -r text; do
    printf '%s\t%s\n' "$(printf '%s' "$text" | sha256 | cut -d ' ' -f 1)" "$text"
  done >"$hashes"
  awk -v hashes="$hashes" -v about="${here}/about.toml" '
    BEGIN {
      line["windows"] = "7f7a9f0f246a7dca9c3337dad59f29abc61de9ebbaf8e7a778283007b4f3f6e7"
      line["dpi"] = "1cee0792a39a8b4052ac78110030989b9374b9ed91d53cfcbc0598e9f8c377b9"
      line["tauri-plugin-single-instance"] = "279bb0a6b9b8181aedb132d9045bf88de6569341a5222f7585ba26064c82fba4"
      line["tauri-plugin-updater"] = "51556d4ae4fc1be8af027239721b4df8e46684013543508e86610fb18819a734"
      line["minisign-verify"] = "e3779c5525d8d1e30bc3c56a5beb4e1a1be85affcc0a3a1a91033dda35d72f2d"
      line["ring"] = "b560151d3861940d7a754b25cb9753400462028037d7a8192bd46f5d3ebb8313"
    }
    function own(crate) { return crate ~ /^windows(-|$)/ ? line["windows"] : line[crate] }
    function flush(   c, i, text) {
      for (c in named) {
        texts[c]++
        for (i = 1; i <= n; i++) {
          text = block[i]
          gsub(/^[ \t\r]+|[ \t\r]+$/, "", text)
          if ((text in hash) && hash[text] == own(c)) found[c] = 1
        }
      }
      delete named
      n = 0
    }
    FILENAME == hashes {
      split($0, pair, "\t")
      hash[substr($0, length(pair[1]) + 2)] = pair[1]
      next
    }
    FILENAME == about {
      if (match($0, /^\[\[[^.]+\.clarify\.files\]\]$/)) files[substr($0, 3, index($0, ".") - 3)]++
      next
    }
    /^=+$/ && length($0) >= 40 { flush(); next }
    /^  \* / { split($0, f, " "); if (f[2] in files) named[f[2]] = 1 }
    { block[++n] = $0 }
    END {
      flush()
      for (c in files) {
        if (!(c in texts)) continue
        if (own(c) == "") {
          print "error: about.toml clarifies " c " but generate.sh names no copyright line of it to look for." > "/dev/stderr"
          failed = 1
        } else if (!found[c] || texts[c] > files[c]) {
          print "error: the clarification of " c " in about.toml did not apply; check its files against their checksums." > "/dev/stderr"
          failed = 1
        }
      }
      exit failed
    }
  ' "$hashes" "${here}/about.toml" "$1"
}

# prints ring's LICENSE in full, then every distinct copyright line in its source as
# published, outside its tests, and the MIT text of the once_cell polyfill the LICENSE
# names, from the source of the ring package in target $1's tree, which must hold one.
ring_part() {
  local dir
  dir="$(cd "$app" && cargo metadata --frozen --format-version 1 \
    --manifest-path crates/app/Cargo.toml --filter-platform "$1" |
    jq -r '.packages[] | select(.name == "ring") | .manifest_path | rtrimstr("/Cargo.toml")')"
  if [[ -z "$dir" || "$dir" == *$'\n'* ]]; then
    echo "error: the ${1} tree does not hold exactly one ring package; change generate.sh's ring part with it." >&2
    exit 1
  fi
  echo "PART 2. RING'S LICENCE FILE AND THE COPYRIGHT NOTICES IN ITS SOURCE"
  echo "-------------------------------------------------------------------"
  echo
  echo "ring ${dir##*/ring-}, under the ISC and Apache-2.0 texts in part 1. Its LICENSE file says"
  echo "which code each covers:"
  echo
  licence "${dir}/LICENSE"
  echo
  echo "The copyright notices in ring's source files, each once:"
  echo
  grep -rhIE --exclude-dir=tests '^[^A-Za-z]*(Portions )?Copyright (\(c\) )?[0-9]' "$dir" |
    sed -E 's/^[^A-Za-z]*//; s/[[:space:]]*(\*\/)?[[:space:]]*$//' | LC_ALL=C sort -u
  echo
  echo "The MIT text of the once_cell polyfill, src/polyfill/once_cell/LICENSE-MIT:"
  echo
  licence "${dir}/src/polyfill/once_cell/LICENSE-MIT"
  echo
}

# one file for each tree: the crates differ, and so does what a tree links that is
# not a crate. $1 is the file's name, $2 the target, $3 the name of the platform.
notices() {
  local file="$1" target="$2" platform="$3"
  local crates="${out}/crates-${platform}.txt"
  (cd "$app" && cargo about generate --frozen --fail \
    --config notices/about.toml --manifest-path crates/app/Cargo.toml \
    --target "$target" --output-file "$crates" notices/notices.hbs)
  clarified "$crates"
  own_copyright_lines "$crates" >"${crates}.own"
  mv "${crates}.own" "$crates"
  {
    echo "THIRD-PARTY NOTICES: Marshal for ${platform} (${target})"
    echo
    echo "Generated by $(cargo about --version) from app/Cargo.lock and the files beside this"
    echo "text in the repository's app/notices folder. Marshal's own licence is not part of it."
    echo
    echo "PART 1. RUST CRATES"
    echo "-------------------"
    echo
    cat "$crates"
    echo
    ring_part "$target"
    echo "PART 3. IBM PLEX SANS, THE FONT BUNDLED FOR MARSHAL'S WINDOW PAGES"
    echo "------------------------------------------------------------------"
    echo
    echo "IBM Plex Sans 1.1.0, by IBM Corp., in the files IBMPlexSans-Regular.woff2,"
    echo "IBMPlexSans-Medium.woff2 and IBMPlexSans-SemiBold.woff2, under the SIL Open Font"
    echo "License 1.1 below. Source: https://github.com/IBM/plex"
    echo
    licence "${frontend}/fonts/LICENSE.txt"
    echo
    echo "PART 4. RADIX COLORS, THE COLOUR VALUES OF MARSHAL'S WINDOW PAGES"
    echo "-----------------------------------------------------------------"
    echo
    echo "@radix-ui/colors 3.0.0, by Radix, whose values the window's stylesheet copies."
    echo "Source: https://github.com/radix-ui/colors"
    echo
    licence "${frontend}/colors-LICENSE.txt"
    if [[ "$platform" == "Windows" ]]; then
      echo
      echo "PART 5. THE WEBVIEW2 LOADER"
      echo "---------------------------"
      echo
      echo "The crate webview2-com-sys, whose MIT notice is the first text below, links"
      echo "WebView2LoaderStatic.lib from the Microsoft.Web.WebView2 SDK 1.0.3800.47 into"
      echo "marshal-app.exe, under the second."
      echo
      echo "webview2-com-sys:"
      echo
      licence "${here}/licences/webview2-com-sys-LICENSE.txt"
      echo
      echo
      echo "Microsoft.Web.WebView2 SDK 1.0.3800.47:"
      echo
      licence "${here}/licences/webview2-loader-LICENSE.txt"
      echo
    fi
  } >"${out}/${file}"
}

notices THIRD-PARTY-NOTICES-windows.txt x86_64-pc-windows-msvc Windows
notices THIRD-PARTY-NOTICES-macos.txt aarch64-apple-darwin macOS

status=0
for file in THIRD-PARTY-NOTICES-windows.txt THIRD-PARTY-NOTICES-macos.txt; do
  if grep -n -e '<year>' -e '<copyright holders>' "${out}/${file}" >&2; then
    echo "error: app/notices/${file} would hold the template line '${template}' above, not a crate's own copyright line; give that crate its own in notices/about.toml or generate.sh." >&2
    exit 1
  fi
  if [[ "$mode" == "write" ]]; then
    cp "${out}/${file}" "${here}/${file}"
    echo "wrote app/notices/${file}"
  elif ! diff -u "${here}/${file}" "${out}/${file}"; then
    echo "error: app/notices/${file} does not match app/Cargo.lock; run 'mise run notices:app' and commit it." >&2
    status=1
  fi
done
exit "$status"
