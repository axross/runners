---
type: Procedure
title: "Checking and installing the Windows app MSI"
description: "How does an operator verify, install, repair or remove an app release?"
---

# Checking and installing the Windows app MSI

The app MSI procedure applies after restoration of app publication. While the release workflow builds the historical CLI, use [CLI Recovery](../runner-recovery/index.md) for the actual release.

## Installing, Updating and Rolling Back

Marshal is installed from a release, never built on the host. You open the
release's MSI, and it places the app, its licence notices and a Start menu entry,
and handles updates and removal. It adds nothing to `PATH`: Marshal has no command
line. The MSI is not code-signed ([Releases](./release-artifacts.md#releases)), so Windows warns about it,
and the steps below say which prompts to expect and how to pass them. macOS has no
release of the app until M2's disk image.

Checking the downloaded MSI before you open it is optional but recommended. The
[check block](#checking-the-download) tests its checksum against `SHA256SUMS`, that
the tag's commit is on the default branch, its attestation against `release.yaml`
run from main, and that the app inside it is the release's app. It needs `gh` 2.102
or later, signed in (`gh auth status`). Without the check, you open a file that
nothing has checked, and an MSI runs as you before anything else can catch a
replaced one.
[Security](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
records that risk.

The last command-line release is the F0 recovery release, expected to be `0.2.0`,
and every release after #132's restoration carries the app. The maintainer decided
to keep releases on `0.x`, as
[Version Rules](../releases/versions.md#version-rules) describes, so no major
version marks the change. The check block below names `0.3.0` as an example. To
check another release, change the block's `version` and `file` assignment lines. If
the recovery release or the first release of the app gets another version, a later
change corrects this paragraph and those lines. A host that runs the command line
moves to the app according to [The Last Command-Line Release](./release-artifacts.md#the-last-command-line-release).

CI runs these blocks against stand-ins and against the dry run's real MSI in the
`Install Blocks` job ([What the Install Tests Cover](#what-the-install-tests-cover)).
They have not run on axpc. Every prompt below is described as expected, from the
platform's documentation, not as observed. The maintainer's first install of a
release of the app records what the prompts showed, and this document then states
it.

[Windows Runner Host](../runner-hosts/index.md#windows-with-docker-desktop) is axpc's operator procedure, which
adds the configuration and the image around this one.

| What                   | Where                                                      |
| ---------------------- | ---------------------------------------------------------- |
| Marshal                | `%LOCALAPPDATA%\Programs\marshal\marshal-app.exe`          |
| Licence notices        | `%LOCALAPPDATA%\Programs\marshal\THIRD-PARTY-NOTICES.txt`  |
| Start menu entry       | Marshal, starting the app                                  |
| Logs and configuration | `%LOCALAPPDATA%\marshal\`, kept by an uninstall            |
| Tokens                 | Credential Manager, kept by an uninstall                   |
| Web view data          | `%LOCALAPPDATA%\app.axross.marshal\`, kept by an uninstall |

### Installing

`<version>` is the release's version without the `v`, such as `0.3.0`.

1. Download `marshal-<version>-windows-x64.msi` from the
   [release page](https://github.com/axross/runners/releases). If the browser warns
   that the file is uncommon or could be harmful, choose to keep it.
2. Optionally, and recommended, run the [check block](#checking-the-download) on
   the downloaded file before opening it.
3. When updating, quit Marshal from its menu first, so that its runners stop.
4. Open the downloaded file. If SmartScreen shows "Windows protected your PC",
   choose "More info", then "Run anyway". If another security warning names the
   file, choose to run it. Smart App Control, when On, may block an unsigned
   installer; this is not verified.
5. Start Marshal from the Start menu entry Marshal.

The MSI installs for your user only and asks for no administrator rights. In CI, a
GitHub-hosted Windows Server runner ran the command line's MSI, built the same way,
as a standard local user without administrator rights, through an interactive logon
and not elevated, and the `Install Blocks` job runs the same for this MSI. For that
run the test sets the Windows Installer policy `DisableMSI` to 0, because a run
without that change exited 1625 on the runner image. Microsoft states no default for
Windows 10 or 11, so whether a Windows 11 PC with no Installer policy allows the
install is not yet measured; the follow-up after the first release measures it on
axpc. What Windows shows when you open the file was not measured.

If `msiexec` exits 1625, its verbose log says "This installation is forbidden by
system policy. Contact your system administrator." A system policy on that PC, such
as the Windows Installer policy `DisableMSI`, blocks the install for your user.
Changing that policy is for whoever manages the PC.

### Checking the Download

The block opens with two assignment lines: `version`, and `file`, the path of the
downloaded MSI, for example in the browser's download folder. Edit those two lines,
then paste the block whole into one shell. It downloads `SHA256SUMS` with `gh`, then
refuses unless:

1. the MSI's SHA-256 matches the line of `SHA256SUMS` for the file name the version
   gives, whatever the downloaded file is called;
2. the tag's commit, with an annotated tag dereferenced, is on the default branch,
   which the compare API reports as `identical` to or `behind` it;
3. `gh attestation verify` passes, pinned to `release.yaml`, the branch
   `refs/heads/main`, and that commit, with self-hosted runners denied; and
4. the `marshal-app.exe` inside the MSI, extracted without elevation, matches the
   line of `SHA256SUMS` for the version's `.exe`. The attestation ties the MSI to a
   commit on main, not to a version, so this step keeps an older genuine MSI under a
   newer version's name from passing.

A check that fails prints its cause after `refused:` and ends the block there
without closing the shell. The block installs nothing, runs nothing it extracts, and
changes neither the downloaded file nor its download mark. It reads the app from a
temporary copy with the mark removed, which `msiexec /a` unpacks into a temporary
folder. The block has no comment lines, as the earlier blocks had none.

In PowerShell 5.1 or 7:

<!-- install-block: windows-check -->

```powershell
& {
  $version = '0.3.0'
  $file = "$env:USERPROFILE\Downloads\marshal-0.3.0-windows-x64.msi"
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $name = "marshal-$version-windows-x64.msi"
  $app = "marshal-$version-windows-x64.exe"
  try {
    $work = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName()))
  } catch { Refuse "no temporary directory: $_" }
  try {
    gh release download "v$version" --repo axross/runners --pattern SHA256SUMS --dir $work
    if ($LASTEXITCODE -ne 0) { Refuse 'SHA256SUMS could not be downloaded' }

    try {
      $sums = @(Get-Content -LiteralPath (Join-Path $work 'SHA256SUMS'))
      $listed = @($sums | Where-Object { ($_ -split '\s+')[1] -eq $name })
      $listedApp = @($sums | Where-Object { ($_ -split '\s+')[1] -eq $app })
      $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLowerInvariant()
    } catch { Refuse "$file or SHA256SUMS could not be read: $_" }
    if ($listed.Count -ne 1 -or ($listed[0] -split '\s+')[0] -ne $actual) { Refuse "$file does not match the $name line of SHA256SUMS" }
    if ($listedApp.Count -ne 1) { Refuse "SHA256SUMS has no single line for $app" }

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
    $apps = @(Get-ChildItem -LiteralPath $unpacked -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'marshal-app.exe' })
    if ($apps.Count -ne 1) { Refuse "the installer holds $($apps.Count) files named marshal-app.exe, not 1" }
    $inside = (Get-FileHash -Algorithm SHA256 -LiteralPath $apps[0].FullName).Hash.ToLowerInvariant()
    if ($inside -ne ($listedApp[0] -split '\s+')[0]) { Refuse "the app in the installer does not match the $app line of SHA256SUMS" }

    Write-Host "Passed: $file is the v$version installer. Nothing was installed."
  } finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
  }
}
```

### Updating, Repairing, Uninstalling and Rolling Back

- **Update.** Install a newer version, steps 1 to 5 above. Then rebuild the image
  from the window if the release changed the runner, as
  [Updating the Runner](../runner-hosts/windows-image.md#updating-the-runner) describes.
- **Upgrading while Marshal runs.** An upgrade with no UI replaced a running program
  at the command line's install path with no reboot, after Restart Manager ended it,
  and the build leg's test measures the same for `marshal-app.exe` on every run. Quit
  Marshal first, as step 3 says; if you did not, start it again after the upgrade.
- **Repair.** Uninstall, then install.
- **Uninstall.** First remove the tokens in the window's Tokens tab, unless you will
  install Marshal again, then quit Marshal from its menu. The block below refuses
  while a `marshal-app.exe` from the install path runs. It finds the installed
  product by the MSI's fixed upgrade code and removes it with `msiexec /x`, and
  Settings, Apps removes it too. The uninstall removes the programs, the Start menu
  entry and the HKCU Run value `Marshal`. It keeps the configuration, the stored
  tokens, any old token files, the logs and the web view's data folder, and the
  command line's files when its MSI is installed too.
- **Roll back.** Complete [step 1 of the Windows host rollback procedure](../runner-hosts/windows-recovery.md#rolling-back). Keep the installed app if no compatible target is verified. Recover retained diagnostics with the installed release first if the target might handle them differently. For an older app release, uninstall and follow steps 1 to 5 to install that version. The MSI
  refuses a lower version while a newer one is installed, with the message "A newer
  version of Marshal is already installed.", and the installed version stays, so
  uninstall first.
- **Roll back to the command line.** Follow [Rolling Back](../marshal/rollback.md#rolling-back) to verify the target, prepare the JSON configuration and token files, and stop the agent. Use the [tagged CLI release and its installation instructions](./release-artifacts.md#the-last-command-line-release). App tokens remain in Credential Manager.

Immutable releases keep a published release's tag from moving, so a version you
install again is the file you installed before.

In PowerShell 5.1 or 7:

<!-- install-block: windows-uninstall -->

```powershell
& {
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $upgradeCode = '{F53C5F78-BA20-4941-9091-988AFDFD656B}'
  $target = Join-Path $env:LOCALAPPDATA 'Programs\marshal\marshal-app.exe'

  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $products = @($installer.GetType().InvokeMember('RelatedProducts', 'GetProperty', $null, $installer, @($upgradeCode)))
  } catch { Refuse "the installed products could not be listed: $_" }
  if ($products.Count -eq 0) {
    Write-Host 'Marshal is not installed; nothing to uninstall.'
    return
  }

  $running = @(Get-Process -Name 'marshal-app' -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -ieq $target })
  if ($running.Count -gt 0) { Refuse 'Marshal is running; quit it from its menu first' }
  foreach ($product in $products) {
    $removal = Start-Process -FilePath msiexec.exe -ArgumentList @('/x', $product, '/qn', '/norestart') -Wait -PassThru
    $null = $removal.Handle
    if ($removal.ExitCode -ne 0) { Refuse "msiexec could not remove $product (exit code $($removal.ExitCode))" }
  }
  Write-Host 'Uninstalled Marshal. The configuration and the logs in %LOCALAPPDATA%\marshal and the tokens in Credential Manager stay.'
}
```

### What the Install Tests Cover

Each block above has a marker comment, `<!-- install-block: <id> -->`, before its
fence, and the tests extract the block from this document, so the text you paste is
the text under test. A test replaces the check block's `version` and `file`
assignment lines, and changes nothing else.

The `Install Blocks` job in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) runs on
`windows-latest`. [`test-install-blocks.ps1`](../../.github/scripts/tests/test-install-blocks.ps1)
runs the blocks under Windows PowerShell 5.1 and PowerShell 7 with stand-ins for
`gh`, `msiexec` (through `Start-Process`), `Unblock-File`, `Get-Process` and the
Windows Installer COM object, each recording its calls. Every refusal is produced,
and each case asserts what ran and what did not, so a failed check is shown to
install nothing. The `gh` stand-in serves a canned API response body and applies the
block's `--jq` expression to it with `jq`, so a wrong expression fails the suite; the
suite stops with one line when `jq` is missing.

The job needs the release dry run and downloads its `marshal-release` artifact, then
[`test-install-blocks-real.ps1`](../../.github/scripts/tests/test-install-blocks-real.ps1)
runs the blocks against the real MSI under Windows PowerShell 5.1, with only `gh`
stubbed to serve the artifact's real `SHA256SUMS`, a tag on main, and a passing
attestation:

- the check block passes a genuine MSI given as a renamed copy that carries a seeded
  `Zone.Identifier` download mark, installs nothing, and leaves the file and its mark
  unchanged;
- the check block refuses a genuine MSI served under another version's name, with a
  matching checksum line and a passing attestation, at the app step;
- the MSI installs, and the installed `marshal-app.exe` matches its `SHA256SUMS`
  line;
- the uninstall block removes the installation, and passes when nothing is
  installed.

A standard local user without administrator rights then runs the check block,
installs the MSI and runs the uninstall block through a secondary logon, which is an
interactive logon, and the outcome is a `MEASURED:` line. The test sets the Windows
Installer policy `DisableMSI` to 0 for that run, because the GitHub-hosted Windows
Server image blocks a non-elevated per-user install, and restores it at the end.
