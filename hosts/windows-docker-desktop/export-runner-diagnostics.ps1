#Requires -Version 5.1
param([string]$Name, [string]$EntryName, [string]$Directory, [switch]$RawRecords)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'diagnostic-export.ps1')

$admission = New-Object IO.StreamReader ([Console]::OpenStandardInput())
try { if ($admission.ReadLine() -cne 'diagnostic-admitted') { exit 5 } }
finally { $admission.Dispose() }

try {
    Export-RunnerDiagnostic -Name $Name -EntryName $EntryName -Directory $Directory -RawRecords $RawRecords.IsPresent -Clock ([Diagnostics.Stopwatch]::StartNew()) -WarningVariable gaps
    if ($gaps.Count -gt 0) { exit 3 }
    exit 0
} catch {
    if ($_.Exception.Message -eq 'diagnostic sink full (ten bundles)') { exit 4 }
    exit 1
}
