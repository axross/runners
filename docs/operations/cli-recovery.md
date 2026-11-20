# CLI Recovery

This guide is for the maintainer collecting the final CLI installer evidence.
It covers the temporary release route and the CLI procedures kept with its tag,
not app installation. You need GitHub release access, `gh` 2.102 or later,
the existing signing prerequisites in [Development Workflow](./development-workflow.md),
and separately approved native hosts. No instruction here authorizes an agent
to dispatch, sign, change settings or operate a host.

## Release the Fixed CLI

The Temporary CLI Recovery section of [Security](../conventions/security.md)
governs the temporary release route. Normal app source and build behavior remain
available and tested, but app publication is suspended until the separate
restoration PR. This is not a second supported CLI product.

The build has two source identities:

- The complete program tree is fixed at
  `dc8e1ebcc0722997206487f3dcca2ae7e519cf02`, with its own CLI manifests, lockfile,
  toolchain, embedded runner-image inputs, fixtures and installer identities.
- The workflow and eventual tag use the actual dispatch commit on main. The
  optional download check pins that commit, not the historical program commit.
  Default provenance identifies the workflow source through OIDC; it does not
  independently prove each secondary checkout's input. The build records its
  checked historical HEAD and lock/source identities separately.

The reusable `cli-recovery-build.yaml` produces the unsigned
`marshal-cli-recovery` dry-run artifact, not a signed release.

For this bounded release, follow this order:

1. Verify the existing release prerequisites as Development Workflow describes.
   Identify the native Windows11 host, actual Smart App Control state and safe
   baseline separately; WSL is not that host.
2. Push or confirm the base tag `v0.1.1` on `954d349`
   (`954d349d8c31c30d7b4439aadfea6f75ee210a0f`) yourself, as
   [Version Rules](./development-workflow.md#version-rules) describes. Read it back
   with `git ls-remote --tags origin v0.1.1`. A tag on any other commit stops the
   release until you decide.
3. Verify main's merged-PR history as Development Workflow's
   [Each Release](./development-workflow.md#each-release) step 1 describes. Do it
   immediately before the dispatch.
4. Dispatch `release.yaml` on main yourself and use the exact version produced
   by its plan. Do not guess the next version from the historical commit. If the
   plan prints a version Version Rules does not expect, cancel the run before
   approving `sign-macos`.
5. Approve the signing deployment yourself. A failed build, signature,
   notarization or signed-package check prevents publication.
6. Check that the published release has exactly the Windows CLI `.exe`, CLI
   `.msi`, macOS bare CLI, signed/notarized `.pkg` and `SHA256SUMS`, all named
   `marshal-<version>-<platform>`. Record the run, tag/workflow commit, historical
   input, version and file hashes on #71.
7. Use the CLI procedures below with that exact version. Publish every required
   #71 step4 items2–4 measurement and native outcome there. Unknown or failed
   results remain blockers; hosted dry runs and signing stand-ins are not native
   evidence.

Only after that successful public evidence does the separate restoration PR
remove the temporary workflow/scripts and restore then-current app routing,
reconciling M2/W4 changes rather than overwriting them. Restoration closes #132,
not #71. A separate documents-only measured signing-record PR alone may close
F0's issue. Keep this tag's CLI procedures valid after restoration.

## Installing, Updating and Rolling Back

`marshal` is installed from a release, never built on the host. You open the
release's installer, the MSI on Windows or the pkg on macOS, and it places the
binary, puts it on `PATH`, and handles updates and removal, so nothing is copied or
added to `PATH` by hand. The pkg is signed with Developer ID and notarized, and the
MSI is not code-signed. Gatekeeper is expected to accept
the pkg with no block to pass, while Windows warns about the MSI, and the steps below
say which prompts to expect on Windows and how to pass them.

Checking the downloaded installer before you open it is optional but recommended.
The check block below tests its checksum against
`SHA256SUMS`, that the tag's commit is on the default branch, its attestation
against `release.yaml` run from main, and the version of the binary inside it. It
needs `gh` 2.102 or later, signed in (`gh auth status`). Without the check, you open
a file that nothing has checked, and the file runs before anything else can catch a
replaced one: an MSI runs as you, and a pkg runs as root once you enter the
administrator password.
[Security](../conventions/security.md)
records that risk.

Before copying a check block, replace its `version` placeholder with the actual
published recovery version, and set `file` to the downloaded installer. `v0.1.0`
has only bare binaries; its rollback blocks remain pinned to `0.1.0`. No installer
version or native recovery outcome is recorded yet.

CI runs these blocks against stand-ins in `mise run check` and the `CLI Recovery
Install Blocks` jobs, which also run them against the dry run's real installers.
The What the Install Tests Cover section below names the checks. They have not run
on the replacement native hosts. The dry run builds its pkg without signing it, so the signed pkg is
checked only in the release run, by `verify-macos`, and on the Mac, and neither has
happened. Every prompt below is described as expected, from the platform's
documentation, not as observed. The maintainer's first install of a release
with installers on each host records what the prompts showed, and this document then
states it.

The pkg is notarized, which you can see before opening it. This is information, not a
step. `spctl --assess --type install -vv <pkg>` is expected to print `accepted` with
`source=Notarized Developer ID`, and `pkgutil --check-signature <pkg>` to show a
Developer ID Installer certificate chained to Apple's root;
[CLI](./cli.md)'s Reading a Signature section says more; its measured record
remains pending and this guide does not invent a team or requirement.

The procedures below retain the historical CLI layout. They do not move the
current app installation or grant changes to a production host.

The historical install locations are:

| OS      | Installed by            | Install path                                  |
| ------- | ----------------------- | --------------------------------------------- |
| Windows | the MSI                 | `%LOCALAPPDATA%\Programs\marshal\marshal.exe` |
| macOS   | the pkg                 | `/usr/local/bin/marshal`                      |
| macOS   | by hand, before the pkg | `~/.local/bin/marshal`                        |
| Windows | by hand, before the MSI | `%LOCALAPPDATA%\Programs\marshal\marshal.exe` |

### Installing

`<version>` is the actual recovery release's version without the `v`.

Install the recovery release in this order:

1. Download `marshal-<version>-windows-x64.msi` or
   `marshal-<version>-macos-arm64.pkg` from the
   [release page](https://github.com/axross/runners/releases). If the browser warns
   that the file is uncommon or could be harmful, choose to keep it. For the pkg, a
   browser prompt is not expected, but this is not yet observed.
2. Optionally, and recommended, run the check block below on
   the downloaded file before opening it.
3. Before opening the installer, do what applies:
   - When updating, stop the running instance first with `marshal stop`.
   - If this host has a manual install from `v0.1.0`, which is
     `~/.local/bin/marshal` on macOS or a `marshal.exe` copied by hand to
     `%LOCALAPPDATA%\Programs\marshal` on Windows, run the
     migration block below. It stops the instance
     itself.
4. Open the downloaded file.
   - On Windows, if SmartScreen shows "Windows protected your PC", choose "More
     info", then "Run anyway". If another security warning names the file, choose
     to run it. Smart App Control, when On, may block an unsigned installer; this is
     not verified.
   - On macOS, Installer shows its usual window and asks for an administrator
     password. Gatekeeper is expected to accept the notarized package with no block
     to pass; this is expected, not yet observed on a Mac.
5. Close every terminal window, open a new one, and run `marshal --version`; it
   prints `marshal <version>`. A new tab in a window that was already open can keep
   the old environment. On macOS, `command -v marshal` prints
   `/usr/local/bin/marshal`.

If `command -v marshal` prints another path on macOS, a manual install from before
the installers is still in place. Run the
migration block below, then uninstall and install
again. On Windows the MSI replaces a manual copy at the install path, so step 5
prints the new version even when you skipped the block;
Migrating From a Manual Install below says what is
left behind.

The MSI installs for your user only. Historical CI at the fixed program source
used a GitHub-hosted Windows Server runner that
ran this as a standard local user without administrator rights. The user signed in
through an interactive logon and was not elevated. The user ran the check block,
which unpacks the MSI with `msiexec /a`. The same user then installed the MSI with
`msiexec /i` and no UI, ran the installed `marshal`, and ran the uninstall block.
Each step passed. For that run the test set the Windows Installer policy
`DisableMSI` to 0. A run without that change exited 1625, and its log reported
`DisableMsi` as 1. The runner has no Installer policy key, so the value is expected
to be the Windows Server default. Microsoft documents 0 as the behavior with no
policy for Windows XP, Vista and 7. It states no default for Windows 10 or 11.
Whether a Windows 11 PC with no Installer policy allows the install is not yet
measured. The maintainer must measure it on the replacement native Windows11 host. What Windows
shows when you open the file was not measured.

If `msiexec` exits 1625, its verbose log says "This installation is forbidden by
system policy. Contact your system administrator." A system policy on that PC, such
as the Windows Installer policy `DisableMSI`, blocks the install for your user.
Changing that policy is for whoever manages the PC.

Of the install steps, only the pkg needs the administrator password. It is expected
to create `/usr/local/bin` when it is absent; that case was not measured. On a
GitHub-hosted runner where `/usr/local/bin` already existed, it was owned by
`runner:admin` with mode 775 before the install, and the install and the uninstall
left that owner and mode unchanged. The package lists the directory as `root:wheel`
with mode 0755, so the listing was not applied to a directory that already existed.

### Checking the Download

The block opens with two assignment lines: `version`, and `file`, the path of the
downloaded installer, for example in the browser's download folder. Edit those two
lines, then paste the block whole into one shell. It does not check the macOS
package's signature or its team; [CLI](./cli.md)'s Reading a Signature section
describes that separate check. It downloads `SHA256SUMS` with `gh`, then
refuses unless:

1. the installer's SHA-256 matches the line of `SHA256SUMS` for the file name the
   version gives, whatever the downloaded file is called;
2. the tag's commit, with an annotated tag dereferenced, is on the default branch,
   which the compare API reports as `identical` to or `behind` it;
3. `gh attestation verify` passes, pinned to `release.yaml`, the branch
   `refs/heads/main`, and that commit, with self-hosted runners denied; and
4. the binary inside the installer prints `marshal <version>`. The attestation ties
   the installer to a commit on main, not to a version, so this step keeps an older
   genuine installer under a newer version's name from passing.

A check that fails prints its cause after `refused:` and ends the block there
without closing the shell. The block installs nothing and changes neither the
downloaded file nor its download mark. It reads the binary from a temporary copy
with the mark removed: on Windows `msiexec /a` unpacks the copy into a temporary
folder, and on macOS `pkgutil --expand-full` does. The block has no comment lines,
because an interactive zsh runs a line that starts with `#` as a command unless
`interactive_comments` is set.

On a Windows host with Smart App Control On, the block's last step is expected to be
blocked, because it runs the unsigned `marshal.exe` extracted from the installer; this
is not measured. The block then refuses with "the binary in the installer does not
run:" and the error. The macOS block's matching refusal is "the binary in the package
does not run". A refusal at this step is not evidence of a bad file, and you may skip
the optional check.

On macOS, the system is expected to ask first whether Terminal may access the
Downloads folder, which the default `file` path is in. Allow it, or the block refuses
because the file "could not be read". This is expected, not measured.

On macOS, in bash or zsh:

<!-- install-block: macos-check -->

```bash
(
  version=REPLACE_WITH_RELEASE_VERSION
  file="$HOME/Downloads/marshal-${version}-macos-arm64.pkg"
  set -euo pipefail
  refuse() { echo "refused: $*" >&2; exit 1; }
  name="marshal-${version}-macos-arm64.pkg"
  work="$(mktemp -d)" || refuse "no temporary directory"
  trap 'rm -rf "$work"' EXIT

  gh release download "v${version}" --repo axross/runners --pattern SHA256SUMS --dir "$work" \
    || refuse "SHA256SUMS could not be downloaded"

  listed="$(awk -v name="$name" '$2 == name { count++; hash = $1 } END { if (count == 1) print hash }' "${work}/SHA256SUMS")"
  [ -n "$listed" ] || refuse "SHA256SUMS has no single line for ${name}"
  actual="$(shasum -a 256 "$file" | awk '{ print $1 }')" || refuse "${file} could not be read"
  [ "$actual" = "$listed" ] || refuse "${file} does not match the ${name} line of SHA256SUMS"

  ref="$(gh api "repos/axross/runners/git/ref/tags/v${version}" --jq '.object.type + " " + .object.sha')" \
    || refuse "the tag v${version} could not be read"
  read -r type sha <<<"$ref"
  if [ "$type" = tag ]; then
    ref="$(gh api "repos/axross/runners/git/tags/${sha}" --jq '.object.type + " " + .object.sha')" \
      || refuse "the annotated tag v${version} could not be read"
    read -r type sha <<<"$ref"
  fi
  [ "$type" = commit ] || refuse "v${version} does not resolve to a commit"
  commit="$sha"

  comparison="$(gh api "repos/axross/runners/compare/main...${commit}" --jq .status)" \
    || refuse "${commit} could not be compared with main"
  case "$comparison" in
    identical | behind) echo "${commit} is on main." ;;
    *) refuse "${commit} is not on main (${comparison})" ;;
  esac

  gh attestation verify "$file" --repo axross/runners \
    --signer-workflow axross/runners/.github/workflows/release.yaml \
    --source-ref refs/heads/main --source-digest "$commit" \
    --deny-self-hosted-runners \
    || refuse "the attestation does not verify"

  copy="${work}/${name}"
  cp "$file" "$copy" || refuse "${file} could not be copied"
  if xattr -p com.apple.quarantine "$copy" >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$copy" || refuse "the quarantine attribute could not be removed from the copy"
  fi
  pkgutil --expand-full "$copy" "${work}/unpacked" || refuse "the package could not be unpacked"
  binary="${work}/unpacked/Payload/usr/local/bin/marshal"
  [ -f "$binary" ] || refuse "the package holds no usr/local/bin/marshal"
  chmod 0755 "$binary" || refuse "the unpacked binary could not be made executable"
  if xattr -p com.apple.quarantine "$binary" >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$binary" || refuse "the quarantine attribute could not be removed from the unpacked binary"
  fi
  printed="$("$binary" --version)" || refuse "the binary in the package does not run"
  [ "$printed" = "marshal ${version}" ] || refuse "the binary in the package is not marshal ${version}: ${printed}"

  echo "Passed: ${file} is the v${version} package. Nothing was installed."
)
```

On Windows, in PowerShell 5.1 or 7:

<!-- install-block: windows-check -->

```powershell
& {
  $version = 'REPLACE_WITH_RELEASE_VERSION'
  $file = "$env:USERPROFILE\Downloads\marshal-$version-windows-x64.msi"
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $name = "marshal-$version-windows-x64.msi"
  try {
    $work = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName()))
  } catch { Refuse "no temporary directory: $_" }
  try {
    gh release download "v$version" --repo axross/runners --pattern SHA256SUMS --dir $work
    if ($LASTEXITCODE -ne 0) { Refuse 'SHA256SUMS could not be downloaded' }

    try {
      $listed = @(Get-Content -LiteralPath (Join-Path $work 'SHA256SUMS') | Where-Object { ($_ -split '\s+')[1] -eq $name })
      $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLowerInvariant()
    } catch { Refuse "$file or SHA256SUMS could not be read: $_" }
    if ($listed.Count -ne 1 -or ($listed[0] -split '\s+')[0] -ne $actual) { Refuse "$file does not match the $name line of SHA256SUMS" }

    $ref = gh api "repos/axross/runners/git/ref/tags/v$version"
    if ($LASTEXITCODE -ne 0) { Refuse "the tag v$version could not be read" }
    $object = ($ref | ConvertFrom-Json).object
    if ($object.type -eq 'tag') {
      $ref = gh api "repos/axross/runners/git/tags/$($object.sha)"
      if ($LASTEXITCODE -ne 0) { Refuse "the annotated tag v$version could not be read" }
      $object = ($ref | ConvertFrom-Json).object
    }
    if ($object.type -ne 'commit') { Refuse "v$version does not resolve to a commit" }
    $commit = $object.sha

    $comparison = gh api "repos/axross/runners/compare/main...$commit" --jq .status
    if ($LASTEXITCODE -ne 0) { Refuse "$commit could not be compared with main" }
    if ($comparison -notin 'identical', 'behind') { Refuse "$commit is not on main ($comparison)" }
    Write-Host "$commit is on main."

    gh attestation verify $file --repo axross/runners `
      --signer-workflow axross/runners/.github/workflows/release.yaml `
      --source-ref refs/heads/main --source-digest $commit `
      --deny-self-hosted-runners
    if ($LASTEXITCODE -ne 0) { Refuse 'the attestation does not verify' }

    $copy = Join-Path $work $name
    try {
      Copy-Item -LiteralPath $file -Destination $copy
      Unblock-File -LiteralPath $copy
    } catch { Refuse "the temporary copy could not be made: $_" }
    $unpacked = Join-Path $work 'unpacked'
    $unpack = Start-Process -FilePath msiexec.exe -ArgumentList @('/a', "`"$copy`"", '/qn', "TARGETDIR=`"$unpacked`"") -Wait -PassThru
    $null = $unpack.Handle
    if ($unpack.ExitCode -ne 0) { Refuse "msiexec could not unpack the installer (exit code $($unpack.ExitCode))" }
    $binaries = @(Get-ChildItem -LiteralPath $unpacked -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'marshal.exe' })
    if ($binaries.Count -ne 1) { Refuse "the installer holds $($binaries.Count) files named marshal.exe, not 1" }
    $binary = $binaries[0].FullName
    try { $printed = & $binary --version } catch { Refuse "the binary in the installer does not run: $_" }
    if ($LASTEXITCODE -ne 0 -or "$printed" -ne "marshal $version") { Refuse "the binary in the installer is not marshal ${version}: $printed" }

    Write-Host "Passed: $file is the v$version installer. Nothing was installed."
  } finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
  }
}
```

### Migrating From a Manual Install

Before the first installer install, and again after any rollback to `v0.1.0`, which
restores the manual install, run the block for your OS. It stops the instance and
refuses if the instance does not stop, then removes the manual install:

- On Windows, the copied `marshal.exe`, its directory when that is left empty, and the
  user `PATH` entry for the directory. The block prints what it removed, or that there
  is nothing to migrate. It looks up the MSI's product by the MSI's fixed upgrade code
  and refuses if it cannot list the products. The manual copy and the MSI share one
  path, so when the product is installed the block removes the installed file too,
  says so, and leaves the product registered: uninstall it and install it again.
- On macOS, `~/.local/bin/marshal` when it is present. The block leaves `~/.local/bin`
  and any `PATH` entry for it alone.

Updating from `v0.1.0` is this block, then an install. If you skip it, the recovery
is this block, then an uninstall and an install again.

Installing the MSI over a manual copy at the same path, without the block, was run
in CI on a GitHub-hosted runner. `msiexec` exited 0 and listed one product. The
installer's binary replaced the manual copy, and `marshal --version` at the install
path printed the installer's version. The user `PATH` then listed the install
directory twice, once from the manual install and once from the MSI. Step 5 of the
install shows the new version. To get back to one `PATH` entry, run the block, then
uninstall and install again. The uninstall block after that install passed and
removed the file at the install path.

Installing the pkg beside a manual copy at `~/.local/bin/marshal`, without the block,
when `~/.local/bin` comes before `/usr/local/bin` on `PATH`, was run in CI on a
GitHub-hosted runner, with the same result under zsh and `/bin/bash`. The pkg
installed its own binary at `/usr/local/bin/marshal` and left the manual copy in
place. The shell kept finding the manual copy: `command -v marshal` printed the manual
copy's path, and `marshal --version` printed the manual copy's version. So step 5
shows the old version and the manual copy's path. The recovery is the migration block,
which removes the manual copy, then an uninstall and an install again. The uninstall
block after that install exited 0 and removed `/usr/local/bin/marshal`, and the manual
copy stayed.

On macOS, in bash or zsh:

<!-- install-block: macos-migrate -->

```bash
(
  set -euo pipefail
  refuse() { echo "refused: $*" >&2; exit 1; }
  manual="$HOME/.local/bin/marshal"
  if [ ! -e "$manual" ] && [ ! -L "$manual" ]; then
    echo "There is no manual install at ${manual}; nothing to migrate."
    exit 0
  fi

  if [ -x "$manual" ]; then
    stopped="$("$manual" stop 2>&1)" || [ "$stopped" = "No instance is running." ] \
      || refuse "the instance did not stop: ${stopped}"
  fi
  rm -f "$manual" || refuse "${manual} could not be removed"
  echo "Removed ${manual}."
)
```

On Windows, in PowerShell 5.1 or 7:

<!-- install-block: windows-migrate -->

```powershell
& {
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $upgradeCode = '{512F67C6-97ED-4562-84F3-2584FA3BA29C}'
  $directory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
  $target = Join-Path $directory 'marshal.exe'
  $removed = New-Object System.Collections.Generic.List[string]

  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $products = @($installer.GetType().InvokeMember('RelatedProducts', 'GetProperty', $null, $installer, @($upgradeCode)))
  } catch { Refuse "the installed products could not be listed: $_" }

  if (Test-Path -LiteralPath $target) {
    $stopped = & {
      $ErrorActionPreference = 'Continue'
      (& $target stop 2>&1 | ForEach-Object { "$_" }) -join "`n"
    }
    if ($LASTEXITCODE -ne 0 -and $stopped.Trim() -ne 'No instance is running.') { Refuse "the instance did not stop: $stopped" }
    try { Remove-Item -LiteralPath $target -Force } catch { Refuse "$target could not be removed: $_" }
    $removed.Add($target)
  }
  if ((Test-Path -LiteralPath $directory) -and -not (Get-ChildItem -LiteralPath $directory -Force)) {
    Remove-Item -LiteralPath $directory -Force
    $removed.Add("the empty directory $directory")
  }

  try {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    try {
      $entries = @(([string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)) -split ';' | Where-Object { $_.Trim() -ne '' })
      $kept = @($entries | Where-Object { [Environment]::ExpandEnvironmentVariables($_.Trim()).TrimEnd('\') -ine $directory.TrimEnd('\') })
      if ($kept.Count -ne $entries.Count) {
        $key.SetValue('Path', ($kept -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
        $removed.Add("the user PATH entry for $directory")
      }
    } finally { $key.Close() }
  } catch { Refuse "the user PATH could not be updated: $_" }
  if ($removed.Count -eq 0) {
    Write-Host "There is no manual install at $directory; nothing to migrate."
  } else {
    Write-Host "Removed $($removed -join ', ')."
  }
  if ($products.Count -gt 0 -and $removed.Count -gt 0) {
    Write-Host 'The marshal installer is installed, and what was removed was part of it too; uninstall it and install it again.'
  }
}
```

### Updating, Repairing, Uninstalling and Rolling Back

- **Update.** Install a newer version, steps 1 to 5 above. Stop the instance first,
  then rebuild the image with `marshal build` and start again with `marshal start`.
- **Upgrading while a program runs from the install path** on Windows. In CI, on a
  GitHub-hosted runner, an upgrade with no UI (`/qn`) ran while a stand-in console
  program ran from the install path as `marshal.exe`. `msiexec` exited 0. The
  upgrade's log shows Restart Manager shutting down the applications that held files
  in use. The file at the install path was then the new version, the program was no
  longer running, one product was listed at the new version, and no rename was left
  pending for a reboot. So the upgrade ends a `marshal` that runs from the install
  path and replaces the file with no reboot. Stop the instance first, as the update
  bullet says; if you did not, start it again after the upgrade. What the interactive
  installer window shows was not measured.
- **A lower pkg over a higher one** on macOS. In CI, on a GitHub-hosted runner, the
  installer exited 0, the lower version's file replaced the higher version's, and the
  receipt reported the lower version. So macOS Installer accepts the downgrade, and
  installing an older pkg is a working rollback there.
- **Repair.** Uninstall, then install.
- **Uninstall.** The block for your OS stops the instance and refuses if it does not
  stop. The logs stay, in `%LOCALAPPDATA%\marshal\logs\` on Windows and
  `~/Library/Logs/marshal/` on macOS. On Windows the block finds the installed
  product by the MSI's fixed upgrade code and removes it with `msiexec /x`, and
  Settings, Apps removes it too, after you stop the instance. macOS has no pkg
  uninstall, so when `pkgutil --pkg-info` finds the pkg's receipt the block removes
  `/usr/local/bin/marshal` and forgets the receipt with `pkgutil --forget`, which
  removes no file. When it finds no receipt the block says the package is not
  installed, runs no `sudo`, and leaves any file at `/usr/local/bin/marshal` in place,
  because the package did not install it. If the block reports such a file and you do
  not want it, remove it with `sudo rm /usr/local/bin/marshal`.
- **Roll back.** Before uninstalling, verify that the older release supports the
  host's profile and configuration. No verified older M1-capable release is
  established here for axmc, so this guide provides no supported axmc rollback;
  keep its current CLI until a compatible baseline is verified. For a supported
  target, uninstall, then install the older version, steps 1 to 5. The MSI
  refuses a lower version while a newer one is installed, with the message "A newer
  version of marshal is already installed.", and the installed version stays, so
  uninstall first. The pkg does not refuse, as the lower pkg bullet above says, so on
  macOS installing the older pkg over the newer one works too. Uninstalling first
  stays the documented route on both.
  The Developer ID certificates are expected to last one year, and whether a
  notarized pkg still installs once its Installer certificate has expired is not
  confirmed, so a rollback through an older release's pkg may fail. The bare-binary
  route to a compatible release avoids that certificate risk.
- **Roll back to `v0.1.0`.** This version predates M1 and refuses `--profile axmc`;
  do not uninstall axmc's CLI for this route. For a configuration that `v0.1.0`
  supports, uninstall, then use the bare-binary
  blocks below and add the install directory to the user's `PATH`
  by hand, as the manual install always needed. On Windows, add
  `%LOCALAPPDATA%\Programs\marshal` to the user `Path` under **Settings, System,
  About, Advanced system settings, Environment Variables**. On macOS, `~/.local/bin`
  is not on the default `PATH`, so add it in the shell's startup file. Updating from
  `v0.1.0` again starts with the migration block.

Immutable releases keep a published release's tag from moving, so a version you
install again is the file you installed before.

On macOS, in bash or zsh:

<!-- install-block: macos-uninstall -->

```bash
(
  set -euo pipefail
  refuse() { echo "refused: $*" >&2; exit 1; }
  installed=/usr/local/bin/marshal
  identifier=app.axross.marshal

  if ! pkgutil --pkg-info "$identifier" >/dev/null 2>&1; then
    echo "The marshal package is not installed; nothing to uninstall."
    if [ -e "$installed" ] || [ -L "$installed" ]; then
      echo "${installed} exists but the package did not install it, so it is left in place."
    fi
    exit 0
  fi

  if [ -x "$installed" ]; then
    stopped="$("$installed" stop 2>&1)" || [ "$stopped" = "No instance is running." ] \
      || refuse "the instance did not stop: ${stopped}"
  fi
  sudo rm -f "$installed" || refuse "${installed} could not be removed"
  sudo pkgutil --forget "$identifier" || refuse "the receipt for ${identifier} could not be forgotten"
  echo "Uninstalled marshal. The logs in ~/Library/Logs/marshal stay."
)
```

On Windows, in PowerShell 5.1 or 7:

<!-- install-block: windows-uninstall -->

```powershell
& {
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $upgradeCode = '{512F67C6-97ED-4562-84F3-2584FA3BA29C}'
  $target = Join-Path $env:LOCALAPPDATA 'Programs\marshal\marshal.exe'

  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $products = @($installer.GetType().InvokeMember('RelatedProducts', 'GetProperty', $null, $installer, @($upgradeCode)))
  } catch { Refuse "the installed products could not be listed: $_" }
  if ($products.Count -eq 0) {
    Write-Host 'The marshal installer is not installed; nothing to uninstall.'
    return
  }

  if (Test-Path -LiteralPath $target) {
    $stopped = & {
      $ErrorActionPreference = 'Continue'
      (& $target stop 2>&1 | ForEach-Object { "$_" }) -join "`n"
    }
    if ($LASTEXITCODE -ne 0 -and $stopped.Trim() -ne 'No instance is running.') { Refuse "the instance did not stop: $stopped" }
  }
  foreach ($product in $products) {
    $removal = Start-Process -FilePath msiexec.exe -ArgumentList @('/x', $product, '/qn', '/norestart') -Wait -PassThru
    $null = $removal.Handle
    if ($removal.ExitCode -ne 0) { Refuse "msiexec could not remove $product (exit code $($removal.ExitCode))" }
  }
  Write-Host 'Uninstalled marshal. The logs in %LOCALAPPDATA%\marshal\logs stay.'
}
```

### Rolling Back to v0.1.0

Before uninstalling, verify that `v0.1.0` supports your configuration. It predates
M1 and refuses `--profile axmc`, so these blocks are not an axmc rollback. Keep
axmc's current CLI; no verified compatible rollback baseline is established here.

`v0.1.0` has no installer, so install it from a bare binary only on a supported
configuration. Remove any installed MSI or pkg with the uninstall block first.
On macOS, a remaining
`/usr/local/bin/marshal` comes first on `PATH` unless you add `~/.local/bin` ahead
of `/usr/local/bin`, so remove it first. The blocks do not touch `PATH`: add
`%LOCALAPPDATA%\Programs\marshal` to the user `Path` on Windows, or `~/.local/bin`
in the shell's startup file on macOS, as the
Roll back to `v0.1.0` bullet above
describes. The procedure accepts a binary only when its checksum matches
`SHA256SUMS`, the tag's commit is on the default branch, and its attestation
verifies against `release.yaml` run from main, at that commit. It needs `gh` 2.102
or later, signed in. The blocks below pin `version=0.1.0` and are the only ones that
copy a binary into place.

Paste a block below whole into one shell. A check that fails prints its cause after
`refused:` and ends the block there without closing the shell, so nothing after it
runs, and a refusal before the rename in step 7 leaves the install path as it was.
The block's variables and working directory do not outlive it, so each run starts
clean.

The block performs these steps:

1. Download the binary and `SHA256SUMS`.
2. Refuse unless the binary's SHA-256 matches its line in `SHA256SUMS`.
3. Resolve the commit the tag points to, dereferencing an annotated tag.
4. Refuse unless the compare API reports that commit `identical` to or `behind`
   the default branch, which means it is on it.
5. Refuse unless `gh attestation verify` passes, pinned to `release.yaml`, the
   branch `refs/heads/main`, and the commit from step 3, with self-hosted runners
   denied. An attestation made by any other workflow, branch, or commit fails here.
6. Refuse unless the downloaded binary's `--version` prints `marshal <version>`.
   The attestation ties the binary to a commit on main, not to a version, so this
   step keeps an older genuine binary under a newer version's name from replacing
   the installed one. On macOS, make the file executable and remove its quarantine
   attribute, if present, before running it.
7. Stop the instance if one is running, refusing if it does not stop, then replace
   the binary in the install directory.
8. Refuse unless the installed binary's `--version` prints `marshal <version>`.

On macOS, in bash or zsh. The block has no comment lines, because an interactive zsh
runs a line that starts with `#` as a command unless `interactive_comments` is set:

<!-- install-block: macos-bare -->

```bash
(
  set -euo pipefail
  refuse() { echo "refused: $*" >&2; exit 1; }
  version=0.1.0
  asset="marshal-${version}-macos-arm64"
  work="$(mktemp -d)" || refuse "no temporary directory"
  trap 'rm -rf "$work"' EXIT
  cd "$work"

  gh release download "v${version}" --repo axross/runners --pattern "$asset" --pattern SHA256SUMS \
    || refuse "the release files could not be downloaded"

  awk -v file="$asset" '$2 == file' SHA256SUMS >"${asset}.sha256"
  { [ -s "${asset}.sha256" ] && shasum -a 256 -c "${asset}.sha256"; } \
    || refuse "${asset} does not match SHA256SUMS"

  ref="$(gh api "repos/axross/runners/git/ref/tags/v${version}" --jq '.object.type + " " + .object.sha')" \
    || refuse "the tag v${version} could not be read"
  read -r type sha <<<"$ref"
  if [ "$type" = tag ]; then
    ref="$(gh api "repos/axross/runners/git/tags/${sha}" --jq '.object.type + " " + .object.sha')" \
      || refuse "the annotated tag v${version} could not be read"
    read -r type sha <<<"$ref"
  fi
  [ "$type" = commit ] || refuse "v${version} does not resolve to a commit"
  commit="$sha"

  comparison="$(gh api "repos/axross/runners/compare/main...${commit}" --jq .status)" \
    || refuse "${commit} could not be compared with main"
  case "$comparison" in
    identical | behind) echo "${commit} is on main." ;;
    *) refuse "${commit} is not on main (${comparison})" ;;
  esac

  gh attestation verify "$asset" --repo axross/runners \
    --signer-workflow axross/runners/.github/workflows/release.yaml \
    --source-ref refs/heads/main --source-digest "$commit" \
    --deny-self-hosted-runners \
    || refuse "the attestation does not verify"

  chmod 0755 "$asset" || refuse "${asset} could not be made executable"
  if xattr -p com.apple.quarantine "$asset" >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$asset" || refuse "the quarantine attribute could not be removed from ${asset}"
  fi
  downloaded="$(./"$asset" --version)" || refuse "the downloaded binary does not run"
  [ "$downloaded" = "marshal ${version}" ] || refuse "the downloaded binary is not marshal ${version}: ${downloaded}"

  if [ -x ~/.local/bin/marshal ]; then
    stopped="$(~/.local/bin/marshal stop 2>&1)" || [ "$stopped" = "No instance is running." ] \
      || refuse "the instance did not stop: ${stopped}"
  fi
  { mkdir -p ~/.local/bin && cp "$asset" ~/.local/bin/marshal.new \
    && chmod 0755 ~/.local/bin/marshal.new && mv -f ~/.local/bin/marshal.new ~/.local/bin/marshal; } \
    || refuse "the binary could not be installed in ~/.local/bin"

  installed="$(~/.local/bin/marshal --version)" || refuse "the installed binary does not run"
  [ "$installed" = "marshal ${version}" ] || refuse "the installed binary is not marshal ${version}: ${installed}"
  echo "Installed marshal ${version} at ~/.local/bin/marshal."
)
```

On Windows, in PowerShell:

<!-- install-block: windows-bare -->

```powershell
& {
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $version = '0.1.0'
  $asset = "marshal-$version-windows-x64.exe"
  try {
    $work = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName()))
  } catch { Refuse "no temporary directory: $_" }
  try {
    $file = Join-Path $work $asset
    $sums = Join-Path $work 'SHA256SUMS'

    gh release download "v$version" --repo axross/runners --pattern $asset --pattern SHA256SUMS --dir $work
    if ($LASTEXITCODE -ne 0) { Refuse 'the release files could not be downloaded' }

    try {
      $listed = @(Get-Content -LiteralPath $sums | Where-Object { ($_ -split '\s+')[1] -eq $asset })
      $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLowerInvariant()
    } catch { Refuse "$asset or SHA256SUMS could not be read: $_" }
    if ($listed.Count -ne 1 -or ($listed[0] -split '\s+')[0] -ne $actual) { Refuse "$asset does not match SHA256SUMS" }

    $ref = gh api "repos/axross/runners/git/ref/tags/v$version"
    if ($LASTEXITCODE -ne 0) { Refuse "the tag v$version could not be read" }
    $object = ($ref | ConvertFrom-Json).object
    if ($object.type -eq 'tag') {
      $ref = gh api "repos/axross/runners/git/tags/$($object.sha)"
      if ($LASTEXITCODE -ne 0) { Refuse "the annotated tag v$version could not be read" }
      $object = ($ref | ConvertFrom-Json).object
    }
    if ($object.type -ne 'commit') { Refuse "v$version does not resolve to a commit" }
    $commit = $object.sha

    $comparison = gh api "repos/axross/runners/compare/main...$commit" --jq .status
    if ($LASTEXITCODE -ne 0) { Refuse "$commit could not be compared with main" }
    if ($comparison -notin 'identical', 'behind') { Refuse "$commit is not on main ($comparison)" }
    Write-Host "$commit is on main."

    gh attestation verify $file --repo axross/runners `
      --signer-workflow axross/runners/.github/workflows/release.yaml `
      --source-ref refs/heads/main --source-digest $commit `
      --deny-self-hosted-runners
    if ($LASTEXITCODE -ne 0) { Refuse 'the attestation does not verify' }

    try { $downloaded = & $file --version } catch { Refuse "the downloaded binary does not run: $_" }
    if ($LASTEXITCODE -ne 0 -or "$downloaded" -ne "marshal $version") { Refuse "the downloaded binary is not marshal ${version}: $downloaded" }

    $directory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
    $target = Join-Path $directory 'marshal.exe'
    if (Test-Path -LiteralPath $target) {
      $stopped = & {
        $ErrorActionPreference = 'Continue'
        (& $target stop 2>&1 | ForEach-Object { "$_" }) -join "`n"
      }
      if ($LASTEXITCODE -ne 0 -and $stopped.Trim() -ne 'No instance is running.') { Refuse "the instance did not stop: $stopped" }
    }
    try {
      New-Item -ItemType Directory -Force -Path $directory | Out-Null
      Copy-Item -LiteralPath $file -Destination "$target.new" -Force
      Move-Item -LiteralPath "$target.new" -Destination $target -Force
    } catch { Refuse "the binary could not be installed in ${directory}: $_" }

    try { $installed = & $target --version } catch { Refuse "the installed binary does not run: $_" }
    if ($LASTEXITCODE -ne 0 -or "$installed" -ne "marshal $version") { Refuse "the installed binary is not marshal ${version}: $installed" }
    Write-Host "Installed marshal $version at $target."
  } finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
  }
}
```

The stop runs with `$ErrorActionPreference` at `Continue` because Windows PowerShell 5.1
turns a native command's redirected standard error into an error, and `Stop` would end
the block at the "No instance is running." line.

The following observations concern the old Windows axpc, not today's WSL2 host
or a replacement native Windows11 baseline.

Windows cannot replace an executable that is running, so step 7 also fails while an
instance still runs from the install path. The procedure's refusals on Windows are
not yet verified on axpc ([#56](https://github.com/axross/runners/issues/56)): the
refusal checks were not run there. A binary fetched with
`gh release download` is assumed to carry no Mark of the Web, and the procedure
does not clear one. On axpc the installed copy had no `Zone.Identifier`
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511048211)), and
SmartScreen did not prompt, with "Check apps and files" On and Smart App Control
Off ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511120441)).
So with Smart App Control Off, the unsigned binary this procedure installs, which
carries no Mark of the Web, is not blocked on axpc. A copy that carries one, such
as a browser download, or a host with Smart App Control On, could be.

### What the Install Tests Cover

Each block above has a marker comment, `<!-- install-block: <id> -->`, before its
fence, and the tests extract the block from this document, so the text you paste is
the text under test. A test replaces a block's `version` assignment line, and a check
block's `file` assignment line, and the macOS uninstall block's `installed`
assignment line, and changes nothing else.

`mise run test:cli-recovery-install-blocks` runs the macOS blocks under bash with stand-ins first
on `PATH` for `gh`, `pkgutil`, `xattr`, `sudo`, `installer`, `cp`, and `marshal`, each
recording its calls. Every refusal is produced, and each case asserts what ran and
what did not, so a failed check is shown to install nothing. The suite includes the
bare-binary blocks' refusals, the uninstall and migration refusals when the
instance does not stop, and the uninstall refusals when `sudo rm` or
`pkgutil --forget` fails. The `gh` stand-ins, here and in the Windows suite, serve a
canned API response body and apply the block's `--jq` expression to it with `jq`, so
a wrong expression fails the suite. `mise.toml` pins `jq` for `mise run check`, and
the `CLI Recovery Install Blocks` jobs run the suites outside mise, so each uses its runner's own
`jq`; the suites stop with one line when `jq` is missing. The macOS suite finishes in
well under a minute on Linux. The `CLI Recovery Install Blocks` jobs in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) runs the same suite
under zsh on macOS, and the Windows suite
(`.github/scripts/cli-recovery-install-tests/test-install-blocks.ps1`)
under Windows PowerShell 5.1 and PowerShell 7 on Windows.

Each job needs the recovery dry run and downloads its `marshal-cli-recovery` artifact, then
runs the documented blocks against the real installers with only `gh` stubbed, to
serve the artifact's real `SHA256SUMS`, a tag on main, and a passing attestation.
The dry run's pkg is built without signing, so the blocks never see a signed package,
and the check block works on it because it checks no signature; the signed package is
exercised only by `verify-macos` in a release run:

- the check block passes a genuine installer given as a renamed copy that carries a
  seeded download mark (`Zone.Identifier` on Windows, `com.apple.quarantine` on
  macOS), installs nothing, and leaves the file and its mark unchanged;
- the check block refuses a genuine installer served under another version's name,
  with a matching checksum line and a passing attestation, at the version step;
- the migration block removes a seeded manual install that differs from the
  installer's binary, after which the installer installs and `marshal --version` at
  the install path prints the build's version;
- installing over such a manual copy without migration is recorded as a `MEASURED:`
  line;
- the migration block, run while the MSI is installed, removes the installed file
  too, after which the uninstall block and a second install leave the installer's
  binary at the install path;
- the uninstall block removes the installation.

macOS runs under zsh and the system `/bin/bash`; Windows runs under Windows
PowerShell 5.1. On the Windows leg a standard local user without administrator
rights then runs the check block and installs the MSI through a secondary logon,
which is an interactive logon, and the outcome is a `MEASURED:` line. The test
sets the Windows Installer policy `DisableMSI` to 0 for that run, because the
GitHub-hosted Windows Server image blocks a non-elevated per-user install, and
restores it at the end.
