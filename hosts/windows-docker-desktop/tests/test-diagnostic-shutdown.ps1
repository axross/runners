#Requires -Version 5.1
param([string]$HostDirectory)
$ErrorActionPreference = 'Stop'
. (Join-Path $HostDirectory 'diagnostic-export.ps1')

$script:DiagnosticExportMilliseconds = 180
$script:DiagnosticHostMilliseconds = 240
$script:shutdownEvents = New-Object System.Collections.Generic.List[string]
$script:removalTimes = New-Object System.Collections.Generic.List[long]
$script:stopClock = $null
$script:sharedClock = $null
$script:exportCalls = 0

# preserves the stop boundary while keeping this test independent of Docker/jobs.
function Invoke-KnownContainerStop {
    param($Workers, $Stopped)

    if ($null -eq $script:stopClock) { $script:stopClock = [Diagnostics.Stopwatch]::StartNew() }
    foreach ($worker in $Workers) { $null = $Stopped.Add($worker.Container) }
}
function Invoke-TestStopJob { param($Job, $ErrorAction) if ($null -eq $Job -or $ErrorAction -ne 'SilentlyContinue') { throw 'unexpected stop' }; Start-Sleep -Milliseconds 10 }

# a slow export consumes the remaining shared budget, never a new per-slot one.
function Invoke-BoundedDiagnosticExport {
    param($Name, $EntryName, $Directory, $RawRecords, $Clock)

    if ($EntryName -ne 'example-entry' -or $Directory -ne 'C:\example-evidence' -or $RawRecords) { throw 'unexpected export input' }
    $script:exportCalls++
    if ($null -eq $script:sharedClock) { $script:sharedClock = $Clock }
    if (-not [object]::ReferenceEquals($script:sharedClock, $Clock)) { throw 'clock restarted between slots' }
    $script:shutdownEvents.Add("export:$Name")
    $remaining = 180 - [int]$Clock.ElapsedMilliseconds
    Start-Sleep -Milliseconds ([Math]::Max(1, [Math]::Min(160, $remaining)))
    if ($Clock.ElapsedMilliseconds -ge 180) { throw 'diagnostic finalization timeout' }
}

# captures removal time independently from the finalizer's supplied stopwatch.
function Invoke-DiagnosticDocker {
    param($Arguments)

    if ($Arguments[0] -ne 'rm' -or $Arguments[1] -ne '-f') { throw 'unexpected Docker operation' }
    $script:shutdownEvents.Add("remove:$($Arguments[2])")
    $script:removalTimes.Add($script:stopClock.ElapsedMilliseconds)
    $fake = [pscustomobject]@{ HasExited = $true; ExitCode = 0 }
    $fake | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Milliseconds) return $Milliseconds -gt 0 }
    $fake | Add-Member -MemberType ScriptMethod -Name Dispose -Value {}
    return $fake
}
function Invoke-TestWaitJob { param($Job, $Timeout) if ($null -eq $Job -or $Timeout -ne 30) { throw 'unexpected wait' }; $script:shutdownEvents.Add('wait'); Start-Sleep -Milliseconds 310 }
function Invoke-TestRemoveJob { param($Job, [switch]$Force, $ErrorAction) if ($null -eq $Job -or -not $Force -or $ErrorAction -ne 'SilentlyContinue') { throw 'unexpected job removal' } }
function Invoke-TestEventSubscriber { return @() }
Set-Alias -Name Stop-Job -Value Invoke-TestStopJob -Scope Local
Set-Alias -Name Wait-Job -Value Invoke-TestWaitJob -Scope Local
Set-Alias -Name Remove-Job -Value Invoke-TestRemoveJob -Scope Local
Set-Alias -Name Get-EventSubscriber -Value Invoke-TestEventSubscriber -Scope Local

$workers = @(foreach ($index in 1..3) {
        [pscustomobject]@{ Container = "example-entry-$index-20240305060708009"; Job = [pscustomobject]@{ Id = $index }; Entry = [pscustomobject]@{
                Name = 'example-entry'; Diagnostics = ($index -ne 2); DiagnosticDirectory = 'C:\example-evidence'; DiagnosticRawRecords = $false
            } }
    })
$tokens = $null
$parseErrors = $null
$tree = [Management.Automation.Language.Parser]::ParseFile((Join-Path $HostDirectory 'supervisor.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw 'supervisor syntax unavailable' }
$main = @($tree.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.TryStatementAst] -and $null -ne $_.Finally })[-1]
if ($null -eq $main) { throw 'supervisor shutdown unavailable' }
$shutdown = [scriptblock]::Create("[CmdletBinding()]`nparam()`n" + ($main.Finally.Statements.Extent.Text -join "`n"))
& $shutdown -WarningVariable gaps
Assert-Case -Name 'multiple-slot shutdown counts stop, admission and earlier exports in one budget' `
    -Passed ($script:removalTimes.Count -eq 2 -and ($script:removalTimes | Measure-Object -Maximum).Maximum -lt 300 -and $script:exportCalls -eq 1 -and "$gaps".Contains('export skipped')) -Detail 'shutdown queue escaped finalization deadline'
Assert-Case -Name 'shutdown attempts every diagnostic removal before waiting for any worker' `
    -Passed (($script:shutdownEvents -join '|') -ceq 'export:example-entry-1-20240305060708009|remove:example-entry-1-20240305060708009|remove:example-entry-3-20240305060708009|wait|wait|wait') -Detail 'worker wait delayed a later removal or changed default cleanup'
