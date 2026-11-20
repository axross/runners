. (Join-Path (Split-Path -Parent $PSScriptRoot) 'tests/install-block-helpers.ps1')

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
            string reported = settings["version"];
            string atInstallPath;
            if (settings.TryGetValue("versionAtInstallPath", out atInstallPath) && self.IndexOf("Programs" + Path.DirectorySeparatorChar + "marshal", StringComparison.OrdinalIgnoreCase) >= 0) {
                reported = atInstallPath;
            }
            Console.Out.WriteLine("marshal " + reported);
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
# every call in <path>.calls when STANDIN_RECORD_CALLS is 1. when VersionAtInstallPath
# is given, --version prints that version instead whenever the file runs from a
# Programs\marshal directory, as a copy of the file at the install path does. on Windows it is a
# small console program compiled with the .NET Framework's csc, because the
# blocks run marshal.exe by path; elsewhere it is a shell script.
function Write-StandInMarshal {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Version,
        [string]$StopOutput = 'No instance is running.',
        [int]$StopExit = 1,
        [int]$VersionExit = 0,
        [string]$VersionAtInstallPath = ''
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
  --version)
    reported="$(get version)"
    case "$0" in
      */Programs/marshal/*) [ -z "$(get versionAtInstallPath)" ] || reported="$(get versionAtInstallPath)" ;;
    esac
    echo "marshal $reported"; exit "$(get versionExit)" ;;
  stop) echo "$(get stopOutput)" >&2; exit "$(get stopExit)" ;;
esac
exit 2
'@
        [IO.File]::WriteAllText($Path, ($shellScript -replace "`r`n", "`n"))
        & chmod 0755 $Path
        if ($LASTEXITCODE -ne 0) { throw "chmod exited $LASTEXITCODE" }
    }
    $settings = @("version=$Version", "stopOutput=$StopOutput", "stopExit=$StopExit", "versionExit=$VersionExit")
    if ($VersionAtInstallPath) { $settings += "versionAtInstallPath=$VersionAtInstallPath" }
    $settingsText = "`n#STANDIN#`n" + ($settings -join "`n") + "`n"
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
