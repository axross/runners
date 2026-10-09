#Requires -Version 5.1
param([string]$HostDirectory)
$ErrorActionPreference = 'Stop'
. (Join-Path $HostDirectory 'diagnostic-export.ps1')
. (Join-Path $HostDirectory 'docker-commands.ps1')

$script:DiagnosticStopMilliseconds = 300
$script:DiagnosticExportMilliseconds = 1800
$script:DiagnosticHostMilliseconds = 2400
$script:stopTargets = New-Object System.Collections.Generic.List[string]
$script:stopClients = New-Object System.Collections.Generic.List[int]
$script:stopEvents = New-Object System.Collections.Generic.List[string]
$script:ordinaryCalls = 0
$script:stopExitCode = $null
$script:removalExitCode = 0
$script:privateMarker = [Guid]::NewGuid().ToString('N')
$script:stopBudgetClock = $null
$boundedStop = ${function:Invoke-BoundedDiagnosticStop}

# clock identity catches restarted budgets even when fast mocks hide elapsed time.
function Invoke-BoundedDiagnosticStop {
    [CmdletBinding()]
    param($Names, $Clock)

    if ($null -eq $script:stopBudgetClock) { $script:stopBudgetClock = $Clock }
    if (-not [object]::ReferenceEquals($script:stopBudgetClock, $Clock)) { throw 'stop clock restarted' }
    & $boundedStop -Names $Names -Clock $Clock
}

# Docker is replaced, but a real blocked child retains native wait/reap semantics.
function Invoke-DiagnosticDocker {
    param($Arguments)

    if ($Arguments[0] -eq 'stop') {
        $script:stopTargets.Add(($Arguments[1..($Arguments.Count - 1)] -join '|'))
        if ($null -eq $script:stopExitCode) {
            $info = New-Object Diagnostics.ProcessStartInfo
            $info.FileName = (Get-Process -Id $PID).Path
            $info.Arguments = '-NoProfile -NonInteractive -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes('Start-Sleep -Seconds 60'))
            $info.UseShellExecute = $false
            $info.RedirectStandardOutput = $true
            $info.RedirectStandardError = $true
            $process = [Diagnostics.Process]::Start($info)
            $script:stopClients.Add($process.Id)
            return $process
        }
    } elseif ($Arguments[0] -eq 'rm' -and $Arguments[1] -eq '-f') {
        $script:stopEvents.Add("remove:$($Arguments[2])")
    } else { throw 'unexpected diagnostic operation' }
    $code = $script:removalExitCode
    if ($Arguments[0] -eq 'stop') { $code = $script:stopExitCode }
    $fake = [pscustomobject]@{ HasExited = $true; ExitCode = $code }
    foreach ($streamName in @('StandardOutput', 'StandardError')) {
        $stream = New-Object IO.MemoryStream
        $bytes = [Text.Encoding]::ASCII.GetBytes($script:privateMarker)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Position = 0
        $fake | Add-Member -NotePropertyName $streamName -NotePropertyValue (New-Object IO.StreamReader $stream)
    }
    $fake | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Milliseconds) return $Milliseconds -gt 0 }
    $fake | Add-Member -MemberType ScriptMethod -Name Dispose -Value { $this.StandardOutput.Dispose(); $this.StandardError.Dispose() }
    return $fake
}

# an accidental synchronous call exceeds the independently chosen test deadline.
function Invoke-Docker {
    param($Arguments)

    if ($Arguments[0] -eq 'ps') {
        if ($Arguments -contains 'label=runners.diagnostic-lifecycle=1' -and -not $script:diagnosticScenario) { return [pscustomobject]@{ ExitCode = 0; Output = @() } }
        return [pscustomobject]@{ ExitCode = 0; Output = @('example-entry-1-20240305060708009', 'example-entry-other-1-20240305060708009') }
    }
    $script:ordinaryCalls++
    if ($script:diagnosticScenario) { Start-Sleep -Milliseconds 3100 }
    $script:stopEvents.Add('ordinary:' + ($Arguments -join ' '))
    return [pscustomobject]@{ ExitCode = 0; Output = @() }
}

# leaves storage out of stop fixtures without hiding a stop-to-export clock reset.
function Invoke-BoundedDiagnosticExport {
    param($Name, $EntryName, $Directory, $RawRecords, $Clock, $DeadlineMilliseconds)

    if ($EntryName -ne 'example-entry' -or $Directory -ne 'C:\example-evidence' -or $RawRecords -or $DeadlineMilliseconds -le 0) { throw 'unexpected export target' }
    if (-not [object]::ReferenceEquals($script:stopBudgetClock, $Clock)) { throw 'export clock restarted after stop' }
    if ($script:shutdownScenario -and -not $script:enabledStopped) { throw 'late diagnostic container has not started yet' }
    $script:stopEvents.Add("export:$Name")
}
function Receive-WorkerOutput { param($Worker) if ($null -eq $Worker) { throw 'missing worker' } }

# models the replacement race after the diagnostic names have already been stopped.
function Invoke-TestStopJob {
    param($Job, $ErrorAction)

    if ($null -eq $Job -or $ErrorAction -ne 'SilentlyContinue') { throw 'unexpected job stop' }
    if ($Job.Id -eq 1) { $script:enabledStopped = $true }
    if ($Job.Id -eq 2) { $workers[1].Container = 'example-other-2-20240305060708009' }
}
function Invoke-TestWaitJob { param($Job, $Timeout) if ($null -eq $Job -or $Timeout -ne 30) { throw 'unexpected job wait' }; $script:stopEvents.Add('wait') }
function Invoke-TestRemoveJob { param($Job, [switch]$Force, $ErrorAction) if ($null -eq $Job -or -not $Force -or ($null -ne $ErrorAction -and $ErrorAction -ne 'SilentlyContinue')) { throw 'unexpected job removal' } }
function Invoke-TestEventSubscriber { return @() }
Set-Alias -Name Stop-Job -Value Invoke-TestStopJob -Scope Local
Set-Alias -Name Wait-Job -Value Invoke-TestWaitJob -Scope Local
Set-Alias -Name Remove-Job -Value Invoke-TestRemoveJob -Scope Local
Set-Alias -Name Get-EventSubscriber -Value Invoke-TestEventSubscriber -Scope Local

$tokens = $null
$parseErrors = $null
$tree = [Management.Automation.Language.Parser]::ParseFile((Join-Path $HostDirectory 'supervisor.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw 'supervisor syntax unavailable' }
foreach ($name in @('Clear-StaleContainer', 'Invoke-KnownContainerStop', 'Complete-WorkerDiagnostic', 'Complete-SlotWorker')) {
    $definition = $tree.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if ($null -eq $definition) { throw 'supervisor cleanup function unavailable' }
    Set-Item -Path "Function:$name" -Value $definition.Body.GetScriptBlock()
}
$main = @($tree.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.TryStatementAst] -and $null -ne $_.Finally })[-1]
$shutdown = [scriptblock]::Create("[CmdletBinding()]`nparam()`n" + ($main.Finally.Statements.Extent.Text -join "`n"))
$failure = $tree.Find({ param($node) $node -is [Management.Automation.Language.IfStatementAst] -and $node.Clauses[0].Item1.Extent.Text.Contains('$worker.Job.State -in') }, $true)
if ($null -eq $failure) { throw 'worker recovery unavailable' }
$recover = [scriptblock]::Create("[CmdletBinding()]`nparam()`n" + ($failure.Clauses[0].Item2.Statements.Extent.Text -join "`n"))
$clock = [Diagnostics.Stopwatch]::StartNew()
$recoverySettings = @{ HealthyRunSeconds = 600; InitialBackoffSeconds = 5; MaxBackoffSeconds = 300 }
foreach ($setting in $recoverySettings.GetEnumerator()) { Set-Variable -Name $setting.Key -Value $setting.Value }
$entry = [pscustomobject]@{ Name = 'example-entry'; Diagnostics = $true; DiagnosticDirectory = 'C:\example-evidence'; DiagnosticRawRecords = $false }

foreach ($path in @('stale', 'recovery', 'shutdown')) {
    $script:diagnosticScenario = $true
    $script:shutdownScenario = $path -eq 'shutdown'
    $script:enabledStopped = $false
    $script:stopBudgetClock = $null
    $script:stopTargets.Clear()
    $script:stopClients.Clear()
    $script:stopEvents.Clear()
    $script:ordinaryCalls = 0
    $worker = [pscustomobject]@{ Key = 'example-entry slot 1'; Container = 'example-entry-1-20240305060708009'; Entry = $entry; Job = [pscustomobject]@{ Id = 1; State = 'Failed' }; StartedAt = 0; RestartAt = 0; RestartBackoff = 5; Result = $null; Finalized = $false }
    $workers = @($worker, [pscustomobject]@{ Container = 'example-other-1-20240305060708009'; Entry = [pscustomobject]@{ Diagnostics = $false }; Job = [pscustomobject]@{ Id = 2 } })
    $elapsed = [Diagnostics.Stopwatch]::StartNew()
    switch ($path) {
        'stale' { Clear-StaleContainer -Entry $entry -WarningVariable gaps }
        'recovery' { & $recover -WarningVariable gaps }
        'shutdown' { & $shutdown -WarningVariable gaps }
    }
    Assert-Case -Name "$path stuck stop still exports and removes within shared finalization budget" `
        -Passed ($elapsed.ElapsedMilliseconds -lt 3000 -and $script:ordinaryCalls -eq 0 -and ($script:stopEvents -join '|').Contains('export:example-entry-1-20240305060708009|remove:example-entry-1-20240305060708009') -and "$gaps".Contains('stop unavailable or over budget')) -Detail 'stuck stop bypassed deadline, export or removal'
    $expectedStops = 1
    if ($path -eq 'shutdown') { $expectedStops = 3 }
    Assert-Case -Name "$path exercises real stop subprocesses and reaps timed-out clients" `
        -Passed ($script:stopClients.Count -eq $expectedStops -and @($script:stopClients | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue }).Count -eq 0) -Detail 'stop fixture was not exercised or client survived'
    if ($path -eq 'shutdown') {
        Assert-Case -Name 'shutdown retries late diagnostic and ordinary generations within the same clock' `
            -Passed ($script:stopTargets[0] -ceq 'example-entry-1-20240305060708009|example-other-1-20240305060708009' -and $script:stopTargets[1] -ceq 'example-entry-1-20240305060708009' -and $script:stopTargets[2] -ceq 'example-other-2-20240305060708009' -and ($script:stopEvents -join '|').EndsWith('|wait|wait')) -Detail 'late container escaped bounded stop or delayed removal'
        Assert-Case -Name 'failed mixed stop force-removes both unconfirmed ordinary generations' -Passed ($script:stopEvents.Contains('remove:example-other-1-20240305060708009') -and $script:stopEvents.Contains('remove:example-other-2-20240305060708009') -and "$gaps".Contains('Mixed shutdown stop failed')) -Detail 'ordinary container incorrectly declared stopped'
    }
    if ($path -eq 'recovery') {
        Assert-Case -Name 'failed worker recovery retains restart and backoff after stop timeout' -Passed ($null -eq $worker.Job -and $null -eq $worker.Container -and $worker.RestartAt -gt 0 -and $worker.RestartBackoff -eq 10) -Detail 'recovery state changed'
    }
}

$script:diagnosticScenario = $false
$script:stopEvents.Clear()
$entry.Diagnostics = $false
Clear-StaleContainer -Entry $entry
$worker = [pscustomobject]@{ Container = 'example-entry-1-20240305060708009'; Entry = $entry; Job = $null }
$stopped = New-Object System.Collections.Generic.HashSet[string]
Invoke-KnownContainerStop -Workers @($worker) -Stopped $stopped -Clock ([Diagnostics.Stopwatch]::StartNew())
Assert-Case -Name 'non-diagnostic stale cleanup and shutdown keep original Docker commands' `
    -Passed (($script:stopEvents -join '|') -ceq 'ordinary:rm -f example-entry-1-20240305060708009|ordinary:stop example-entry-1-20240305060708009') -Detail 'default path gained diagnostic finalization'

foreach ($removalCode in @(0, 7)) {
    $script:stopBudgetClock = $null
    $script:stopExitCode = 7
    $script:removalExitCode = $removalCode
    $script:stopEvents.Clear()
    $stopped = New-Object System.Collections.Generic.HashSet[string]
    $mixed = @($worker, [pscustomobject]@{ Container = $null; Job = $null; Entry = [pscustomobject]@{ Diagnostics = $true } })
    Invoke-KnownContainerStop -Workers $mixed -Stopped $stopped -Clock ([Diagnostics.Stopwatch]::StartNew()) -WarningVariable removalGaps
    Assert-Case -Name "mixed stop failure confirms ordinary cleanup only after removal exit $removalCode" -Passed ($stopped.Contains($worker.Container) -eq ($removalCode -eq 0) -and $script:stopEvents.Contains('remove:example-entry-1-20240305060708009') -and "$removalGaps".Contains('Mixed shutdown stop failed') -and ("$removalGaps".Contains('Container removal failed') -eq ($removalCode -ne 0))) -Detail 'failed removal declared success or attempt unreported'
}
$script:removalExitCode = 0
foreach ($code in @(0, 7)) {
    $script:stopBudgetClock = $null
    $script:stopExitCode = $code
    $stopSucceeded = Invoke-BoundedDiagnosticStop -Names @('example-entry-1-20240305060708009') -Clock ([Diagnostics.Stopwatch]::StartNew()) -WarningVariable gaps
    Assert-Case -Name "stop exit $code is checked without emitting client stdout or stderr" `
        -Passed ($stopSucceeded -eq ($code -eq 0) -and -not "$gaps".Contains($script:privateMarker) -and ("$gaps".Contains('stop unavailable or over budget') -eq ($code -ne 0))) -Detail 'stop exit ignored or private output emitted'
}
$script:DiagnosticExportMilliseconds = 0
$script:stopBudgetClock = $null
$before = $script:stopTargets.Count
$null = Invoke-BoundedDiagnosticStop -Names @('example-entry-1-20240305060708009') -Clock ([Diagnostics.Stopwatch]::StartNew()) -WarningVariable gaps
Assert-Case -Name 'spent shared deadline skips stop subprocess with an explicit gap' -Passed ($script:stopTargets.Count -eq $before -and "$gaps".Contains('over budget')) -Detail 'spent clock started another stop'
