#Requires -Version 5.1
param([string]$Name, [string]$Sink, [switch]$RawRecords, [string]$Marker, [switch]$FinalUnavailable)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../diagnostic-export.ps1')
Invoke-BoundedDiagnosticExport -Name $Name -EntryName 'example-entry' -Directory $Sink -RawRecords $RawRecords.IsPresent -Clock ([Diagnostics.Stopwatch]::StartNew())
$bundle = @(Get-ChildItem -LiteralPath $Sink -Directory)
if ($bundle.Count -ne 1) { throw 'unexpected bundle count' }
$metrics = [IO.File]::ReadAllText((Join-Path $bundle[0].FullName 'metrics.txt'))
if ($metrics.Contains($Marker)) { throw 'private marker reached metrics' }
if (-not $FinalUnavailable -and $metrics -notmatch '(?m)^final=observed\r?$') { throw 'final sample missing' }
$raw = @(Get-ChildItem -LiteralPath $bundle[0].FullName -Filter 'raw-*.log')
$expected = 0
if ($RawRecords) { $expected = 2 }
if ($raw.Count -ne $expected) { throw 'raw opt-in or symlink exclusion failed' }
if (-not (Test-Path -LiteralPath (Join-Path $bundle[0].FullName 'complete.txt'))) { throw 'export did not complete' }
Write-Output 'Private export from actual image completed before removal.'
