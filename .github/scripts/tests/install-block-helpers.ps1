# helpers that the Windows install block tests dot-source: extracting a
# documented block from docs/operations/cli.md, replacing its assignment lines,
# and making a stand-in marshal.exe. written for Windows PowerShell 5.1 and 7.

function Test-RunningOnWindows {
    return [Environment]::OSVersion.Platform -eq 'Win32NT'
}

function Get-Sha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

# returns the code of the fenced block that follows the marker comment
# `<!-- install-block: <id> -->`, and throws unless the marker occurs exactly once
# and a non-empty fence follows it.
function Get-InstallBlock {
    param([Parameter(Mandatory = $true)][string]$Document, [Parameter(Mandatory = $true)][string]$Id)
    $marker = "<!-- install-block: $Id -->"
    $state = 'outside'
    $markers = 0
    $fences = 0
    $code = New-Object System.Collections.Generic.List[string]
    foreach ($line in (Get-Content -LiteralPath $Document)) {
        if ($state -eq 'outside') {
            if ($line -eq $marker) { $markers++; $state = 'marked' }
        } elseif ($state -eq 'marked') {
            if ($line.Trim() -eq '') { continue }
            if ($line.StartsWith('```')) { $state = 'inside' } else { $state = 'outside' }
        } elseif ($line.StartsWith('```')) {
            $fences++
            $state = 'outside'
        } else {
            $code.Add($line)
        }
    }
    if ($markers -ne 1 -or $fences -ne 1 -or $code.Count -eq 0) {
        throw "$Document does not hold exactly one non-empty fenced block after the marker for '$Id'."
    }
    return ($code.ToArray() -join "`n")
}

# replaces the one line that starts with the prefix (after indentation) by the new
# line, and throws unless exactly one line does.
function Edit-BlockLine {
    param(
        [Parameter(Mandatory = $true)][string]$Block,
        [Parameter(Mandatory = $true)][string]$Prefix,
        [Parameter(Mandatory = $true)][string]$Line
    )
    $replaced = 0
    $lines = foreach ($existing in ($Block -split "`n")) {
        if ($existing.TrimStart().StartsWith($Prefix)) { $replaced++; "  $Line" } else { $existing }
    }
    if ($replaced -ne 1) { throw "The block holds $replaced lines that start with '$Prefix', not 1." }
    return ($lines -join "`n")
}

function Assert-NoCommentLine {
    param([Parameter(Mandatory = $true)][string]$Id, [Parameter(Mandatory = $true)][string]$Block)
    if ($Block -match '(?m)^\s*#') {
        throw "The block $Id has a comment line."
    }
}

$script:StandInTemplate = $null

# compiles the stand-in program once, under the directory, before a test points
# TEMP elsewhere. on other systems the stand-in is a script and nothing is built.
function Initialize-StandInMarshal {
    param([Parameter(Mandatory = $true)][string]$Directory)
    if (-not (Test-RunningOnWindows)) { return }
    $source = @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
public static class StandInMarshal {
    public static int Main(string[] args) {
        string self = Assembly.GetExecutingAssembly().Location;
        string marker = "\n#STANDIN#\n";
        string text = System.Text.Encoding.GetEncoding(28591).GetString(File.ReadAllBytes(self));
        var settings = new Dictionary<string, string>();
        foreach (string line in text.Substring(text.LastIndexOf(marker, StringComparison.Ordinal) + marker.Length).Split('\n')) {
            int split = line.IndexOf('=');
            if (split > 0) { settings[line.Substring(0, split)] = line.Substring(split + 1); }
        }
        if (Environment.GetEnvironmentVariable("STANDIN_RECORD_CALLS") == "1") {
            File.AppendAllText(self + ".calls", string.Join(" ", args) + "\n");
        }
        if (args.Length > 0 && args[0] == "--version") {
            Console.Out.WriteLine("marshal " + settings["version"]);
            return int.Parse(settings["versionExit"]);
        }
        if (args.Length > 0 && args[0] == "stop") {
            Console.Error.WriteLine(settings["stopOutput"]);
            return int.Parse(settings["stopExit"]);
        }
        return 2;
    }
}
'@
    $templateDirectory = Join-Path $Directory 'standin-template'
    New-Item -ItemType Directory -Force -Path $templateDirectory | Out-Null
    $sourcePath = Join-Path $templateDirectory 'standin.cs'
    Set-Content -LiteralPath $sourcePath -Value $source -Encoding UTF8
    $compiler = Join-Path $env:windir 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    if (-not (Test-Path -LiteralPath $compiler)) { throw "$compiler does not exist." }
    $template = Join-Path $templateDirectory 'standin.exe'
    & $compiler /nologo /target:exe "/out:$template" $sourcePath | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $template)) { throw 'csc could not build the stand-in marshal.exe.' }
    $script:StandInTemplate = $template
}

# makes the stand-in marshal at the path. it answers --version and stop from the
# settings appended to the file itself, so a copy answers the same, and records
# every call in <path>.calls when STANDIN_RECORD_CALLS is 1. on Windows it is a
# small console program compiled with the .NET Framework's csc, because the
# blocks run marshal.exe by path; elsewhere it is a shell script.
function Write-StandInMarshal {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Version,
        [string]$StopOutput = 'No instance is running.',
        [int]$StopExit = 1,
        [int]$VersionExit = 0
    )
    $directory = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    if (Test-RunningOnWindows) {
        if ($null -eq $script:StandInTemplate) { throw 'Initialize-StandInMarshal has not run.' }
        Copy-Item -LiteralPath $script:StandInTemplate -Destination $Path -Force
    } else {
        $shellScript = @'
#!/bin/sh
get() { sed -n "s/^$1=//p" "$0"; }
[ "$STANDIN_RECORD_CALLS" = 1 ] && echo "$*" >>"$0.calls"
case "$1" in
  --version) echo "marshal $(get version)"; exit "$(get versionExit)" ;;
  stop) echo "$(get stopOutput)" >&2; exit "$(get stopExit)" ;;
esac
exit 2
'@
        [IO.File]::WriteAllText($Path, ($shellScript -replace "`r`n", "`n"))
        & chmod 0755 $Path
    }
    $settingsText = "`n#STANDIN#`n" + (@("version=$Version", "stopOutput=$StopOutput", "stopExit=$StopExit", "versionExit=$VersionExit") -join "`n") + "`n"
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Append)
    try {
        $bytes = [Text.Encoding]::GetEncoding(28591).GetBytes($settingsText)
        $stream.Write($bytes, 0, $bytes.Length)
    } finally {
        $stream.Dispose()
    }
    if (Test-Path -LiteralPath "$Path.calls") { Remove-Item -LiteralPath "$Path.calls" -Force }
}

function Get-StandInCall {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath "$Path.calls")) { return , @() }
    return , @(Get-Content -LiteralPath "$Path.calls")
}

# the stand-in for the gh calls the Windows blocks make, which the install block
# tests share. it reads $script:Scenario (Release, DownloadFail, TagFail, TagType,
# TagSha, AnnotatedFail, AnnotatedSha, CompareFail, Compare, AttestFail), records
# each call in $script:Calls, and sets $LASTEXITCODE as the real gh does. an API
# read has a canned response body, and the caller's --jq expression is applied to
# it with jq, as gh does, so a block with a wrong expression gets a wrong value.
function Write-ExitCode {
    param([int]$Code)
    $global:LASTEXITCODE = $Code
}

# returns the body as JSON, or what jq makes of it under the --jq expression.
function Get-StandInApiResponse {
    param([Parameter(Mandatory = $true)]$Body, [string]$JqExpression)
    $json = $Body | ConvertTo-Json -Compress -Depth 5
    if (-not $JqExpression) { return $json }
    if ($null -eq (Get-Command -Name jq -CommandType Application -ErrorAction SilentlyContinue)) {
        throw 'jq is not installed, and the gh stand-in applies --jq with it.'
    }
    $result = $json | & jq -r $JqExpression
    if ($LASTEXITCODE -ne 0) { Write-ExitCode 1; return }
    return $result
}

function gh {
    $script:Calls.Add('gh ' + ($args -join ' '))
    $scenario = $script:Scenario
    Write-ExitCode 0
    if ($args[0] -eq 'release' -and $args[1] -eq 'download') {
        $patterns = @()
        $directory = '.'
        for ($index = 2; $index -lt $args.Count; $index++) {
            if ($args[$index] -eq '--pattern') { $patterns += [string]$args[$index + 1]; $index++ }
            elseif ($args[$index] -eq '--dir') { $directory = [string]$args[$index + 1]; $index++ }
        }
        if ($scenario.DownloadFail) { Write-ExitCode 1; return }
        foreach ($pattern in $patterns) {
            $source = Join-Path $scenario.Release $pattern
            if (-not (Test-Path -LiteralPath $source)) { Write-ExitCode 1; return }
            Copy-Item -LiteralPath $source -Destination (Join-Path $directory $pattern)
        }
        return
    }
    if ($args[0] -eq 'api') {
        $uri = [string]$args[1]
        $jqExpression = ''
        for ($index = 2; $index -lt $args.Count - 1; $index++) {
            if ($args[$index] -eq '--jq') { $jqExpression = [string]$args[$index + 1] }
        }
        if ($uri -like '*/git/ref/tags/*') {
            if ($scenario.TagFail) { Write-ExitCode 1; return }
            return (Get-StandInApiResponse -Body @{ ref = 'refs/tags/v0'; object = @{ type = $scenario.TagType; sha = $scenario.TagSha } } -JqExpression $jqExpression)
        }
        if ($uri -like '*/git/tags/*') {
            if ($scenario.AnnotatedFail) { Write-ExitCode 1; return }
            return (Get-StandInApiResponse -Body @{ tag = 'v0'; object = @{ type = 'commit'; sha = $scenario.AnnotatedSha } } -JqExpression $jqExpression)
        }
        if ($uri -like '*/compare/*') {
            if ($scenario.CompareFail) { Write-ExitCode 1; return }
            return (Get-StandInApiResponse -Body @{ status = $scenario.Compare; ahead_by = 0; behind_by = 0 } -JqExpression $jqExpression)
        }
    }
    if ($args[0] -eq 'attestation' -and $args[1] -eq 'verify') {
        if ($scenario.AttestFail) { Write-ExitCode 1 }
        return
    }
    Write-ExitCode 99
}

