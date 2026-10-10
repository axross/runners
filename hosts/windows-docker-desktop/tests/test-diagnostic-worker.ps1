#Requires -Version 5.1
param([string]$HostDirectory, [string]$Scratch)
$ErrorActionPreference = 'Stop'
. (Join-Path $HostDirectory 'host-configuration.ps1')

$tree = [Management.Automation.Language.Parser]::ParseFile((Join-Path $HostDirectory 'supervisor.ps1'), [ref]$null, [ref]$null)
$assignment = $tree.Find({ param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq '$WorkerScript' }, $true)
$block = $assignment.Right.Find({ param($node) $node -is [Management.Automation.Language.ScriptBlockExpressionAst] }, $true)
$workerScript = $block.ScriptBlock.GetScriptBlock()
$fixtureRoot = Join-Path $Scratch 'worker'
$null = New-Item -ItemType Directory -Path $fixtureRoot
$tokenPath = Join-Path $fixtureRoot 'token'
$marker = [Guid]::NewGuid().ToString('N')
[IO.File]::WriteAllText($tokenPath, $marker, [Text.Encoding]::ASCII)
$dockerFixture = @'
. '__DOCKER__'
function Get-SlotCpuAffinity {
    param($ProbeName, $ImageName, $Count, $Offset)
    $script:probes++
    if ($script:failCpu -and $script:probes -eq 1) { throw 'synthetic discovery failure' }
    $inventory = '2,5-6,11'
    if ($script:probes -gt (1 + [int]$script:failCpu)) { $inventory = '13-16' }
    return Select-CpuAffinity -AvailableCpus $inventory -Count $Count -Offset $Offset
}
function Invoke-RestMethod {
    param($Uri, $Method, $Headers, $Body, $ContentType)
    if ($script:probes -le [int]$script:failCpu) { throw 'registration before discovery' }
    $script:registrations++
    if ($script:failJit -and $script:registrations -eq 1) { throw 'synthetic JIT failure' }
    return [pscustomobject]@{ encoded_jit_config = $marker }
}
function Start-Sleep {
    param($Seconds)
    if (-not $Diagnostics -and $script:runs -ge 2) { throw 'fixture loop finished' }
    $script:delays.Add([int]$Seconds)
}
function Invoke-DockerLogged {
    param($Arguments)
    $script:runs++
    if ($Arguments -notcontains '--rm' -or $Arguments -contains '--label') { throw 'ordinary lifecycle changed' }
    $script:selections.Add($Arguments[[Array]::IndexOf($Arguments, '--cpuset-cpus') + 1])
    if ([Environment]::GetEnvironmentVariable('ACTIONS_RUNNER_INPUT_JITCONFIG', 'Process') -ne $marker) { throw 'JIT environment missing' }
    return $script:runnerExit
}
'@
$dockerFixture = $dockerFixture.Replace('__DOCKER__', (Join-Path $HostDirectory 'docker-commands.ps1').Replace("'", "''"))
[IO.File]::WriteAllText((Join-Path $fixtureRoot 'docker-commands.ps1'), $dockerFixture, [Text.Encoding]::ASCII)
$diagnosticFixture = @'
function Initialize-DiagnosticProcessContainment { }
function Invoke-DiagnosticContainerRun {
    param($Arguments)
    $script:runs++
    if ($Arguments -contains '--rm' -or $Arguments -notcontains 'runners.diagnostic-lifecycle=1') { throw 'diagnostic lifecycle missing' }
    $script:selections.Add($Arguments[[Array]::IndexOf($Arguments, '--cpuset-cpus') + 1])
    if ([Environment]::GetEnvironmentVariable('ACTIONS_RUNNER_INPUT_JITCONFIG', 'Process') -ne $marker) { throw 'JIT environment missing' }
    return $script:runnerExit
}
function Complete-DiagnosticContainer { throw 'worker attempted diagnostic finalization' }
'@
[IO.File]::WriteAllText((Join-Path $fixtureRoot 'diagnostic-export.ps1'), $diagnosticFixture, [Text.Encoding]::ASCII)
$entry = [pscustomobject]@{ Owner = 'example-owner'; Repository = 'example-repo'; Name = 'example-entry'; TokenPath = $tokenPath; Labels = [string[]]@('axpc'); Cpus = 1.5; CpuAffinityCount = 2; CpuAffinityOffset = 2L; MemoryGb = 8; Diagnostics = $true; DiagnosticDirectory = 'C:\example-evidence'; DiagnosticRawRecords = $false }
foreach ($code in @(0, 7)) {
    $script:runs = 0
    $script:registrations = 0
    $script:probes = 0
    $script:failCpu = $true
    $script:selections = New-Object System.Collections.Generic.List[string]
    $script:runnerExit = $code
    $script:failJit = $true
    $script:delays = New-Object System.Collections.Generic.List[int]
    $arguments = Get-SlotWorkerArgument -Entry $entry -Slot 1 -ScriptRoot $fixtureRoot -ImageName 'actions-runner:local' -RunCommand '/home/runner/run.sh' -JitConfigVariable 'ACTIONS_RUNNER_INPUT_JITCONFIG' -InitialBackoffSeconds 5 -MaxBackoffSeconds 300
    $before = [Diagnostics.Stopwatch]::GetTimestamp()
    $values = @($arguments.Values)
    $output = @(& $workerScript @values)
    $result = @($output | Where-Object { $_ -isnot [string] })
    Assert-Case -Name "enabled worker returns result $code after discovery/JIT retries, without export" -Passed ($script:runs -eq 1 -and $script:registrations -eq 2 -and $result.Count -eq 1 -and $result[0].RunnerExitCode -eq $code -and $result[0].BackoffSeconds -eq 20 -and $result[0].CompletedAt -ge $before -and ($script:delays -join '|') -eq '5|10' -and $output -notcontains 'SLOT_IDLE') -Detail 'worker looped, finalized or lost retry/result'
    Assert-Case -Name 'discovery failure creates no registration and recovery remeasures before launch' -Passed ($script:probes -eq 3 -and ($script:selections -join '|') -eq '15-16') -Detail 'failed inventory admitted or cached'
    Assert-Case -Name 'enabled worker clears JIT environment before publishing completion' -Passed ([string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable('ACTIONS_RUNNER_INPUT_JITCONFIG', 'Process')) -and -not ($output -join '|').Contains($marker)) -Detail 'JIT leaked into completion'
}
$entry.Diagnostics = $false
$script:runs = 0
$script:registrations = 0
$script:probes = 0
$script:failCpu = $false
$script:selections.Clear()
$script:failJit = $false
$script:runnerExit = 7
$script:delays.Clear()
$arguments = Get-SlotWorkerArgument -Entry $entry -Slot 1 -ScriptRoot $fixtureRoot -ImageName 'actions-runner:local' -RunCommand '/home/runner/run.sh' -JitConfigVariable 'ACTIONS_RUNNER_INPUT_JITCONFIG' -InitialBackoffSeconds 5 -MaxBackoffSeconds 300
$output = New-Object System.Collections.Generic.List[object]
$values = @($arguments.Values)
try { & $workerScript @values | ForEach-Object { $output.Add($_) } } catch { if ($_.Exception.Message -ne 'fixture loop finished') { throw } }
Assert-Case -Name 'ordinary worker retains registration, auto-removal, idle protocol and retry loop with current affinity' -Passed ($script:runs -eq 2 -and $script:registrations -eq 2 -and @($output | Where-Object { $_ -eq 'SLOT_IDLE' }).Count -eq 2 -and ($script:delays -join '|') -eq '5' -and ($script:selections -join '|') -eq '6,11|15-16' -and [string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable('ACTIONS_RUNNER_INPUT_JITCONFIG', 'Process'))) -Detail 'ordinary lifecycle/affinity changed'

foreach ($name in @('Complete-SlotWorker', 'Complete-WorkerDiagnostic', 'Clear-StaleContainer', 'Invoke-KnownContainerStop')) {
    $definition = $tree.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    Set-Item -Path "Function:$name" -Value $definition.Body.GetScriptBlock()
}
$script:admissions = 0
function Complete-DiagnosticContainer {
    param($Name, $EntryName, $Directory, $RawRecords, $Clock, [long]$CompletedAt = 0)
    if ($Name -ne 'example-entry-1-20240305060708009' -or $EntryName -ne 'example-entry' -or $Directory -ne 'C:\example-evidence' -or $RawRecords -or -not $Clock.IsRunning -or $CompletedAt -lt 0) { throw 'invalid finalization fixture input' }
    $script:admissions++
    Write-Warning 'Diagnostic gap: synthetic export failure.'
}
function Receive-WorkerOutput { param($Worker) if ($null -eq $Worker) { throw 'missing worker' } }
function Invoke-TestRemoveJob { param($Job, [switch]$Force) if ($null -eq $Job -or -not $Force) { throw 'invalid job removal' } }
Set-Alias -Name Remove-Job -Value Invoke-TestRemoveJob -Scope Local
$clock = [Diagnostics.Stopwatch]::StartNew()
$settings = @{ InitialBackoffSeconds = 5; MaxBackoffSeconds = 300; HealthyRunSeconds = 600 }
foreach ($setting in $settings.GetEnumerator()) { Set-Variable -Name $setting.Key -Value $setting.Value }
$entry.Diagnostics = $true
foreach ($code in @(0, 7)) {
    $worker = [pscustomobject]@{ Key = 'example-entry slot 1'; Entry = $entry; Job = [pscustomobject]@{ State = 'Completed' }; Container = 'example-entry-1-20240305060708009'; Result = [pscustomobject]@{ RunnerExitCode = $code; CompletedAt = [Diagnostics.Stopwatch]::GetTimestamp(); BackoffSeconds = 10 }; Finalized = $false; RestartAt = 0; RunnerBackoff = 10; RestartBackoff = 20; StartedAt = 0 }
    $before = $script:admissions
    Complete-SlotWorker -Worker $worker
    Complete-WorkerDiagnostic -Worker $worker
    $expectedDelay = 0
    $expectedBackoff = 5
    if ($code -ne 0) { $expectedDelay = 10; $expectedBackoff = 20 }
    Assert-Case -Name "parent preserves result $code and restart backoff despite export failure and repeated shutdown finalization" -Passed ($worker.Result.RunnerExitCode -eq $code -and $worker.RunnerBackoff -eq $expectedBackoff -and $worker.RestartAt -ge $expectedDelay -and $worker.RestartAt -lt ($clock.Elapsed.TotalSeconds + $expectedDelay + 1) -and $script:admissions -eq ($before + 1) -and $null -eq $worker.Job -and $null -eq $worker.Container) -Detail 'result/backoff changed or completion admitted twice'
}

$definition = $tree.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Receive-WorkerOutput' }, $true)
Set-Item -Path Function:Receive-WorkerOutput -Value $definition.Body.GetScriptBlock()
$completionScript = {
    Write-Output 'CURRENT_CONTAINER:example-entry-1-20240305060708009'
    Write-Information 'CURRENT_CONTAINER:example-untrusted-output'
    [pscustomobject]@{ RunnerExitCode = 0; CompletedAt = [Diagnostics.Stopwatch]::GetTimestamp(); BackoffSeconds = 10 }
}
$completionJob = Start-Job -ScriptBlock $completionScript
try {
    $null = Wait-Job -Job $completionJob -Timeout 5
    $worker = [pscustomobject]@{ Key = 'example-entry slot 1'; Entry = $entry; Job = $completionJob; Container = $null; Result = $null; Finalized = $false; RestartAt = 0; RunnerBackoff = 10; RestartBackoff = 20; StartedAt = 0 }
    $before = $script:admissions
    Receive-WorkerOutput -Worker $worker
    Complete-WorkerDiagnostic -Worker $worker
    Complete-SlotWorker -Worker $worker
    Assert-Case -Name 'real completed-job drain racing shutdown admits one export and ignores log protocol lookalikes' -Passed ($completionJob.State -eq 'Completed' -and $script:admissions -eq ($before + 1) -and $worker.Result.RunnerExitCode -eq 0 -and $null -eq $worker.Container -and $null -eq $worker.Job) -Detail 'completion drain lost result, trusted a log or exported twice'
} finally {
    if ($completionJob.State -eq 'Running') { Stop-Job -Job $completionJob }
    Microsoft.PowerShell.Core\Remove-Job -Job $completionJob -Force
}

$script:staleEvents = New-Object System.Collections.Generic.List[string]
function Invoke-Docker {
    param($Arguments)
    if ($Arguments[0] -eq 'ps') {
        $names = @('example-entry-1-20240305060708009', 'example-entry-2-20240305060708009', 'example-entry-other-1-20240305060708009')
        if ($Arguments -contains 'label=runners.diagnostic-lifecycle=1') { $names = @($names[0]) }
        return [pscustomobject]@{ ExitCode = 0; Output = $names }
    }
    $script:staleEvents.Add(($Arguments -join ' '))
    return [pscustomobject]@{ ExitCode = 0; Output = @() }
}
function Invoke-BoundedDiagnosticStop { param($Names, $Clock) if (-not $Clock.IsRunning) { throw 'missing stale clock' }; $script:staleEvents.Add('bounded-stop:' + ($Names -join '|')); return $false }
foreach ($enabled in @($false, $true)) {
    $entry.Diagnostics = $enabled
    $script:staleEvents.Clear()
    $before = $script:admissions
    $refused = $false
    try { Clear-StaleContainer -Entry $entry -WarningVariable gaps } catch { $refused = $_.Exception.Message.Contains('entry startup refused') }
    Assert-Case -Name "immutable marker governs stale cleanup with new diagnostics $enabled" -Passed ($refused -eq (-not $enabled) -and $script:staleEvents.Contains('bounded-stop:example-entry-1-20240305060708009') -and $script:staleEvents.Contains('rm -f example-entry-2-20240305060708009') -and -not $script:staleEvents.Contains('rm -f example-entry-1-20240305060708009') -and $script:staleEvents.Count -eq 2 -and $script:admissions -eq ($before + [int]$enabled) -and ($enabled -or "$gaps".Contains('deletion refused'))) -Detail 'rollback evidence deleted/exported or ordinary stale behavior changed'
}

. (Join-Path $HostDirectory 'diagnostic-export.ps1')
$pidFile = Join-Path $Scratch 'runner-client.pid'
$clientScript = {
    param($Root, $PidFile)
    $ErrorActionPreference = 'Stop'
    . (Join-Path $Root 'diagnostic-export.ps1')
    Initialize-DiagnosticProcessContainment
    $script:clientPidFile = $PidFile
    function Invoke-DiagnosticDocker {
        param($Arguments)
        if ($Arguments[0] -ne 'run') { throw 'unexpected runner operation' }
        $info = New-Object Diagnostics.ProcessStartInfo
        $info.FileName = (Get-Process -Id $PID).Path
        $info.Arguments = '-NoProfile -NonInteractive -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes('Start-Sleep 60'))
        $info.UseShellExecute = $false
        $info.RedirectStandardOutput = $true
        $info.RedirectStandardError = $true
        $process = [Diagnostics.Process]::Start($info)
        [IO.File]::WriteAllText($script:clientPidFile, [string]$process.Id)
        return $process
    }
    Invoke-DiagnosticContainerRun -Arguments @('run')
}
$job = Start-Job -ArgumentList $HostDirectory, $pidFile -ScriptBlock $clientScript
try {
    $readyClock = [Diagnostics.Stopwatch]::StartNew()
    while (-not (Test-Path -LiteralPath $pidFile) -and $job.State -eq 'Running' -and $readyClock.ElapsedMilliseconds -lt 5000) { Start-Sleep -Milliseconds 10 }
    Assert-Case -Name 'enabled cancellation fixture reaches real in-flight runner client' -Passed (Test-Path -LiteralPath $pidFile) -Detail 'fixture never started'
    $cancelClock = [Diagnostics.Stopwatch]::StartNew()
    Stop-Job -Job $job
    $childId = [int][IO.File]::ReadAllText($pidFile)
    $remaining = Get-Process -Id $childId -ErrorAction SilentlyContinue
    Assert-Case -Name 'enabled worker cancellation has no native Stop-Job wait' -Passed ($cancelClock.ElapsedMilliseconds -lt 3000) -Detail 'Stop-Job blocked'
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        $reaped = $null -eq $remaining -or $remaining.WaitForExit(1000)
        Assert-Case -Name 'Windows enabled worker death reaps its in-flight client' -Passed $reaped -Detail 'client escaped lifetime boundary'
    } else { Write-Output 'SKIP  Windows worker-death lifetime containment; Linux fixture client is explicitly reaped by the test' }
    if ($null -ne $remaining) { if (-not $remaining.HasExited) { $remaining.Kill() }; $remaining.Dispose() }
} finally {
    if ($job.State -eq 'Running') { Stop-Job -Job $job }
    Microsoft.PowerShell.Core\Remove-Job -Job $job -Force
}
