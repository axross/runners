#Requires -Version 5.1
<#
.SYNOPSIS
    keeps ephemeral, single-job GitHub Actions runner containers available for
    every target repository in a host configuration, on this Windows machine.

.DESCRIPTION
    for each target repository the configuration lists, this script runs that
    many slots. a slot loops forever: request a just-in-time (JIT) runner
    registration from GitHub with that repository's own token, start a
    throwaway container from the runner image with the registration passed
    through the container's environment, wait for it to finish the one job it
    takes, then repeat. see docs/operations/windows-runner-host.md for the
    procedure around it.

    written for Windows PowerShell 5.1 and PowerShell 7.

.PARAMETER ConfigPath
    path to the host configuration JSON file. see runner-host.example.json.

.PARAMETER ValidateOnly
    validates the configuration and prints the planned registrations, names,
    container name patterns, resource limits and volume names, then exits.
    exits 1 on an invalid configuration. calls neither Docker nor GitHub and
    reads no token file.
#>
param(
    [Parameter(Mandatory)][string]$ConfigPath,
    [switch]$ValidateOnly
)

$ErrorActionPreference = 'Stop'
$InformationPreference = 'Continue'

. (Join-Path $PSScriptRoot 'host-configuration.ps1')
. (Join-Path $PSScriptRoot 'docker-commands.ps1')
. (Join-Path $PSScriptRoot 'diagnostic-export.ps1')

try {
    $plan = Read-HostConfiguration -Path $ConfigPath
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}

if ($ValidateOnly) {
    Get-PlanSummary -Plan $plan
    exit 0
}

$RunCommand = '/home/runner/run.sh'
$JitConfigVariable = 'ACTIONS_RUNNER_INPUT_JITCONFIG'
$InitialBackoffSeconds = 5
$MaxBackoffSeconds = 300
# a slot job that ran this long before it died starts the next restart backoff
# over, so a long-lived worker that dies once is not punished like a crash loop.
$HealthyRunSeconds = 600

# returns the `--mount` arguments that attach the entry's volumes to a container.
function Get-MountArgument {
    param([Parameter(Mandatory)]$Entry)

    foreach ($volume in $Entry.Volumes) {
        '--mount'
        "type=volume,source=$($volume.Name),target=$($volume.MountPath)"
    }
}

# throws unless the entry's token file exists and holds text.
function Assert-TokenFile {
    param([Parameter(Mandatory)]$Entry)

    if (-not (Test-Path -LiteralPath $Entry.TokenPath -PathType Leaf)) {
        throw "$($Entry.Path): token file not found at $($Entry.TokenPath)."
    }
    if ([string]::IsNullOrWhiteSpace((Get-Content -LiteralPath $Entry.TokenPath -Raw))) {
        throw "$($Entry.Path): token file at $($Entry.TokenPath) is empty."
    }
}

# Docker Desktop and this script's scheduled task both start at sign-in with no
# ordering between them, so poll instead of failing.
function Wait-ForDocker {
    while ((Invoke-Docker -Arguments @('info')).ExitCode -ne 0) {
        Write-Warning 'Docker Desktop is not responding yet (still starting, or not running) - waiting.'
        Start-Sleep -Seconds 5
    }
}

# creates the entry's volumes and reasserts runner ownership of their mount
# points, because a fresh volume can be root-owned.
function Initialize-EntryVolume {
    param([Parameter(Mandatory)]$Entry)

    foreach ($volume in $Entry.Volumes) {
        $result = Invoke-Docker -Arguments @('volume', 'create', $volume.Name)
        if ($result.ExitCode -ne 0) {
            throw "Failed to create Docker volume '$($volume.Name)': $($result.Output -join ' ')"
        }
    }
    if (@($Entry.Volumes).Count -eq 0) {
        return
    }

    $arguments = @('run', '--rm', '--pull', 'never', '--user', 'root') + @(Get-MountArgument -Entry $Entry) +
        @($plan.ImageName, 'chown', 'runner:docker') + @($Entry.Volumes | ForEach-Object { $_.MountPath })
    $result = Invoke-Docker -Arguments $arguments
    if ($result.ExitCode -ne 0) {
        throw "Failed to set ownership of the cache volumes for $($Entry.Path): $($result.Output -join ' ')"
    }
}

# removes only containers named like this entry's own slots, so another
# supervisor's or another entry's containers are never touched.
function Clear-StaleContainer {
    param([Parameter(Mandatory)]$Entry)

    $result = Invoke-Docker -Arguments @('ps', '-a', '--filter', "name=$($Entry.Name)-", '--format', '{{.Names}}')
    if ($result.ExitCode -ne 0) {
        throw "Failed to list containers: $($result.Output -join ' ')"
    }
    $marked = Invoke-Docker -Arguments @('ps', '-a', '--filter', "name=$($Entry.Name)-", '--filter', 'label=runners.diagnostic-lifecycle=1', '--format', '{{.Names}}')
    if ($marked.ExitCode -ne 0) { throw 'Diagnostic lifecycle identification unavailable; stale cleanup refused.' }
    $ownName = Get-JobContainerNamePattern -Name $Entry.Name
    $diagnosticClock = [Diagnostics.Stopwatch]::StartNew()
    $retained = $false
    foreach ($name in $result.Output) {
        if ($name -cmatch $ownName) {
            if ($marked.Output -ccontains $name) {
                $null = Invoke-BoundedDiagnosticStop -Names @($name) -Clock $diagnosticClock
                if ($Entry.Diagnostics) {
                    Complete-DiagnosticContainer -Name $name -EntryName $Entry.Name -Directory $Entry.DiagnosticDirectory -RawRecords $Entry.DiagnosticRawRecords -Clock $diagnosticClock
                } else {
                    Write-Warning 'Diagnostic evidence retained after disabling; deletion refused. Explicit private recovery and cleanup are required before this entry can start.'
                    $retained = $true
                }
                continue
            }
            Write-Warning "Removing stale container '$name' left over from an earlier run."
            $removal = Invoke-Docker -Arguments @('rm', '-f', $name)
            if ($removal.ExitCode -ne 0) {
                Write-Warning "Failed to remove stale container '$name': $($removal.Output -join ' ')"
            }
        }
    }
    if ($retained) { throw 'Marked diagnostic containers remain after disabling; entry startup refused.' }
}

# the script block of one slot's background job. it is self-contained because
# Start-Job runs in a separate process that shares no functions with this one,
# and job logs must not enter the parent's lifecycle protocol.
$WorkerScript = {
    param(
        [string]$ScriptRoot,
        [string]$Owner,
        [string]$Repository,
        [string]$TokenPath,
        [int]$Slot,
        [string]$ImageName,
        [string]$EntryName,
        [string[]]$Labels,
        [double]$Cpus,
        [long]$CpuAffinityOffset,
        [int]$MemoryGb,
        [string[]]$Mounts,
        [string]$RunCommand,
        [string]$JitConfigVariable,
        [int]$InitialBackoffSeconds,
        [int]$MaxBackoffSeconds,
        [bool]$Diagnostics,
        [bool]$DiagnosticRawRecords,
        [int]$BackoffSeconds
    )

    $ErrorActionPreference = 'Stop'
    . (Join-Path $ScriptRoot 'host-configuration.ps1')
    . (Join-Path $ScriptRoot 'docker-commands.ps1')
    . (Join-Path $ScriptRoot 'diagnostic-export.ps1')
    if ($Diagnostics) {
        try { Initialize-DiagnosticProcessContainment }
        catch { Write-Warning 'Diagnostic gap: runner client lifetime support unavailable.' }
    }

    $label = "$EntryName slot ${Slot}"

    # re-read on every registration, so a rotated token file applies to the
    # next job without a restart.
    function Get-RunnerToken {
        param([string]$Path)

        if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
            throw "token file not found at $Path"
        }
        $token = (Get-Content -LiteralPath $Path -Raw).Trim()
        if ([string]::IsNullOrWhiteSpace($token)) {
            throw "token file at $Path is empty"
        }
        return $token
    }

    # runner_group_id 1 is the default group; the endpoint requires the field
    # even though a repository-level runner has no group to choose.
    function Request-JitConfig {
        param(
            [string]$Owner,
            [string]$Repository,
            [string]$TokenPath,
            [string]$Name,
            [string[]]$Labels
        )

        $body = @{ name = $Name; runner_group_id = 1; labels = $Labels } | ConvertTo-Json -Compress
        $headers = @{
            Authorization          = "Bearer $(Get-RunnerToken -Path $TokenPath)"
            Accept                 = 'application/vnd.github+json'
            'X-GitHub-Api-Version' = '2022-11-28'
        }
        $uri = "https://api.github.com/repos/$Owner/$Repository/actions/runners/generate-jitconfig"
        try {
            $response = Invoke-RestMethod -Uri $uri -Method Post -Headers $headers -Body $body -ContentType 'application/json'
        } catch {
            # GitHub's error body says why a request was refused. it never holds
            # the request headers, but the token is still scrubbed from the text
            # and the text is kept to one short line before it reaches a log.
            $detail = "$($_.Exception.Message) $($_.ErrorDetails.Message)"
            $detail = ($detail.Replace($headers.Authorization.Substring('Bearer '.Length), '<token>') -replace '\s+', ' ').Trim()
            if ($detail.Length -gt 500) {
                $detail = $detail.Substring(0, 500) + '...'
            }
            throw $detail
        }
        if ([string]::IsNullOrWhiteSpace($response.encoded_jit_config)) {
            throw 'GitHub returned no JIT configuration'
        }
        return $response.encoded_jit_config
    }

    $backoffSeconds = $BackoffSeconds
    while ($true) {
        try {
            $cpuset = Get-SlotCpuAffinity -ProbeName "$EntryName-cpu-probe-$Slot" -ImageName $ImageName `
                -Count ([int][Math]::Ceiling($Cpus)) -Offset $CpuAffinityOffset
        } catch {
            Write-Warning "${label}: CPU discovery or probe cleanup failed; no registration - retrying in ${backoffSeconds}s."
            Start-Sleep -Seconds $backoffSeconds
            $backoffSeconds = [Math]::Min($backoffSeconds * 2, $MaxBackoffSeconds)
            continue
        }

        try {
            $name = Get-JobContainerName -EntryName $EntryName -Slot $Slot
            $jit = [pscustomobject]@{
                Name             = $name
                EncodedJitConfig = Request-JitConfig -Owner $Owner -Repository $Repository -TokenPath $TokenPath -Name $name -Labels $Labels
            }
        } catch {
            Write-Warning "${label}: failed to request a JIT registration ($($_.Exception.Message)) - retrying in ${backoffSeconds}s."
            Start-Sleep -Seconds $backoffSeconds
            $backoffSeconds = [Math]::Min($backoffSeconds * 2, $MaxBackoffSeconds)
            continue
        }

        Write-Output "CURRENT_CONTAINER:$($jit.Name)"
        Write-Information "${label}: starting container $($jit.Name)."

        # the variable holds the registration only in this job's process
        # environment, where the Docker client reads it from.
        $arguments = Get-JobContainerArgument -Name $jit.Name -Cpus (Format-CpuCount -Cpus $Cpus) -CpusetCpus $cpuset -MemoryGb $MemoryGb -Mounts $Mounts `
            -JitConfigVariable $JitConfigVariable -ImageName $ImageName -RunCommand $RunCommand `
            -Diagnostics $Diagnostics -DiagnosticRawRecords $DiagnosticRawRecords
        [Environment]::SetEnvironmentVariable($JitConfigVariable, $jit.EncodedJitConfig, 'Process')
        try {
            if ($Diagnostics) {
                $exitCode = Invoke-DiagnosticContainerRun -Arguments $arguments
            } else {
                $exitCode = Invoke-DockerLogged -Arguments $arguments
            }
        } finally {
            [Environment]::SetEnvironmentVariable($JitConfigVariable, $null, 'Process')
        }
        $jit = $null

        if ($Diagnostics) {
            return [pscustomobject]@{
                RunnerExitCode = $exitCode
                CompletedAt = [Diagnostics.Stopwatch]::GetTimestamp()
                BackoffSeconds = $backoffSeconds
            }
        }
        Write-Output 'SLOT_IDLE'
        if ($exitCode -ne 0) {
            Write-Warning "${label}: container exited with code $exitCode - retrying in ${backoffSeconds}s."
            Start-Sleep -Seconds $backoffSeconds
            $backoffSeconds = [Math]::Min($backoffSeconds * 2, $MaxBackoffSeconds)
        } else {
            Write-Information "${label}: container exited successfully (GitHub owns the job result) - starting a replacement."
            $backoffSeconds = $InitialBackoffSeconds
        }
    }
}

# starts the background job that runs one slot's worker loop and returns it.
function Invoke-SlotWorkerJob {
    param([Parameter(Mandatory)]$Entry, [Parameter(Mandatory)][int]$Slot, [int]$BackoffSeconds = $InitialBackoffSeconds)

    $workerArguments = Get-SlotWorkerArgument -Plan $plan -Entry $Entry -Slot $Slot -ScriptRoot $PSScriptRoot `
        -Mounts ([string[]]@(Get-MountArgument -Entry $Entry)) -RunCommand $RunCommand -JitConfigVariable $JitConfigVariable `
        -InitialBackoffSeconds $InitialBackoffSeconds -MaxBackoffSeconds $MaxBackoffSeconds -BackoffSeconds $BackoffSeconds
    Start-Job -ScriptBlock $WorkerScript -ArgumentList @($workerArguments.Values)
}

# drains a worker's output. the container it is running, read only to know what
# to stop on shutdown, is tracked from its protocol lines.
function Receive-WorkerOutput {
    param([Parameter(Mandatory)]$Worker)

    # a worker that failed re-raises its error here, so contain it and let the
    # caller's state check restart the worker.
    try {
        Receive-Job -Job $Worker.Job -ErrorAction Stop | ForEach-Object {
            if ($_ -is [string] -and $_ -cmatch '^CURRENT_CONTAINER:(.+)$') {
                $Worker.Container = $Matches[1]
            } elseif ($_ -is [string] -and $_ -ceq 'SLOT_IDLE') {
                $Worker.Container = $null
            } elseif ($Worker.Entry.Diagnostics -and $null -ne $_.PSObject.Properties['RunnerExitCode']) {
                $Worker.Result = $_
            }
        }
    } catch {
        Write-Warning "$($Worker.Key): error draining job output ($($_.Exception.Message))."
    }
}

# stops the containers the workers last reported, once each, in a single Docker
# call so that they share one grace period. the drain first catches a container
# started since the last pass.
function Invoke-KnownContainerStop {
    param([Parameter(Mandatory)]$Workers, [Parameter(Mandatory)]$Stopped, [Diagnostics.Stopwatch]$Clock)

    foreach ($worker in $Workers) {
        if ($null -ne $worker.Job) {
            Receive-WorkerOutput -Worker $worker
        }
    }
    $pending = @($Workers | Where-Object { $null -ne $_.Container -and -not $Stopped.Contains($_.Container) })
    $names = @($pending | ForEach-Object { $_.Container })
    if ($names.Count -eq 0) {
        return
    }
    Write-Information "Stopping container(s): $($names -join ', ')."
    if (@($Workers | Where-Object { $_.Entry.Diagnostics }).Count -gt 0) {
        $success = Invoke-BoundedDiagnosticStop -Names $names -Clock $Clock
        foreach ($worker in $pending) {
            if ($success) { $null = $Stopped.Add($worker.Container) }
            elseif (-not $worker.Entry.Diagnostics) {
                Write-Warning 'Mixed shutdown stop failed; force-removing an unconfirmed ordinary container.'
                if (Invoke-BoundedContainerRemoval -Name $worker.Container -Clock $Clock) { $null = $Stopped.Add($worker.Container) }
            }
        }
        return
    } else {
        $stop = Invoke-Docker -Arguments (@('stop') + $names)
        if ($stop.ExitCode -ne 0) {
            Write-Warning "Failed to stop container(s) $($names -join ', '): $($stop.Output -join ' ')"
        }
    }
    foreach ($name in $names) {
        $null = $Stopped.Add($name)
    }
}

# admission is recorded before export, so shutdown cannot admit a completion twice.
function Complete-WorkerDiagnostic {
    [CmdletBinding()]
    param($Worker, [Diagnostics.Stopwatch]$Clock = [Diagnostics.Stopwatch]::StartNew())

    if (-not $Worker.Entry.Diagnostics -or $null -eq $Worker.Container -or $Worker.Finalized) { return }
    $Worker.Finalized = $true
    $completedAt = 0L
    if ($null -ne $Worker.Result) { $completedAt = [long]$Worker.Result.CompletedAt }
    try {
        Complete-DiagnosticContainer -Name $Worker.Container -EntryName $Worker.Entry.Name -Directory $Worker.Entry.DiagnosticDirectory -RawRecords $Worker.Entry.DiagnosticRawRecords -Clock $Clock -CompletedAt $completedAt
    } finally { $Worker.Container = $null }
}

# worker failure backoff is distinct from a container's success/nonzero retry.
function Complete-SlotWorker {
    param($Worker)

    Receive-WorkerOutput -Worker $Worker
    $state = $Worker.Job.State
    if ($Worker.Entry.Diagnostics -and $null -ne $Worker.Container) {
        $diagnosticClock = [Diagnostics.Stopwatch]::StartNew()
        if ($null -eq $Worker.Result) { $null = Invoke-BoundedDiagnosticStop -Names @($Worker.Container) -Clock $diagnosticClock }
        Complete-WorkerDiagnostic -Worker $Worker -Clock $diagnosticClock
    }
    Remove-Job -Job $Worker.Job -Force
    $Worker.Job = $null
    $Worker.Container = $null
    $now = $clock.Elapsed.TotalSeconds
    if ($Worker.Entry.Diagnostics -and $null -ne $Worker.Result) {
        $Worker.RestartBackoff = $InitialBackoffSeconds
        if ($Worker.Result.RunnerExitCode -eq 0) {
            $Worker.RestartAt = $now
            $Worker.RunnerBackoff = $InitialBackoffSeconds
            Write-Information "$($Worker.Key): container exited successfully (GitHub owns the job result) - starting a replacement."
        } else {
            $delay = [int]$Worker.Result.BackoffSeconds
            $Worker.RestartAt = $now + $delay
            $Worker.RunnerBackoff = [Math]::Min($delay * 2, $MaxBackoffSeconds)
            Write-Warning "$($Worker.Key): container exited with code $($Worker.Result.RunnerExitCode) - retrying in ${delay}s."
        }
    } else {
        if (($now - $Worker.StartedAt) -ge $HealthyRunSeconds) { $Worker.RestartBackoff = $InitialBackoffSeconds }
        $Worker.RestartAt = $now + $Worker.RestartBackoff
        Write-Warning "$($Worker.Key): job ended unexpectedly (state: $state) - restarting it in $($Worker.RestartBackoff)s."
        $Worker.RestartBackoff = [Math]::Min($Worker.RestartBackoff * 2, $MaxBackoffSeconds)
    }
}

Write-Information "Runner supervisor starting: $(@($plan.Repositories).Count) entries, image $($plan.ImageName)."

# one entry that cannot start (a missing token file, a volume that cannot be
# created, a stale container that cannot be listed) is skipped with a warning
# naming it, so it does not take the other repositories down. the warning holds
# the entry and the reason, never a token.
function Write-EntrySkipped {
    param([Parameter(Mandatory)]$Entry, [Parameter(Mandatory)][string]$Reason)

    Write-Warning "$($Entry.Path) ($($Entry.Owner)/$($Entry.Repository)) is not started and gets no slots: $Reason"
}

$startable = New-Object System.Collections.Generic.List[object]
foreach ($entry in $plan.Repositories) {
    try {
        Assert-TokenFile -Entry $entry
        $startable.Add($entry)
    } catch {
        Write-EntrySkipped -Entry $entry -Reason $_.Exception.Message
    }
}
if ($startable.Count -gt 0) {
    if (@($startable | Where-Object { $_.Diagnostics }).Count -gt 0) {
        try { Initialize-DiagnosticProcessContainment }
        catch { Write-Warning 'Diagnostic gap: exporter containment support unavailable; exports will not be admitted.' }
    }
    Wait-ForDocker
}
$prepared = New-Object System.Collections.Generic.List[object]
foreach ($entry in $startable) {
    try {
        Clear-StaleContainer -Entry $entry
        Initialize-EntryVolume -Entry $entry
        $prepared.Add($entry)
    } catch {
        Write-EntrySkipped -Entry $entry -Reason $_.Exception.Message
    }
}
if ($prepared.Count -eq 0) {
    [Console]::Error.WriteLine('No repository could be started, so the supervisor is exiting.')
    exit 1
}

# restart timing uses a monotonic clock, so a change of the system time cannot
# shorten or stretch a backoff.
$clock = [System.Diagnostics.Stopwatch]::StartNew()

$workers = New-Object System.Collections.Generic.List[object]
foreach ($entry in $prepared) {
    for ($slot = 1; $slot -le $entry.Slots; $slot++) {
        $workers.Add([pscustomobject]@{
                Key            = "$($entry.Name) slot $slot"
                Entry          = $entry
                Slot           = $slot
                Job            = Invoke-SlotWorkerJob -Entry $entry -Slot $slot
                StartedAt      = $clock.Elapsed.TotalSeconds
                RestartAt      = 0
                RestartBackoff = $InitialBackoffSeconds
                RunnerBackoff  = $InitialBackoffSeconds
                Container      = $null
                Result         = $null
                Finalized      = $false
            })
    }
}

# Ctrl+C is the one shutdown path this script observes. the action runs in this
# runspace, so the flag is visible to the loop below.
$script:stopRequested = $false
$null = Register-ObjectEvent -InputObject ([Console]) -EventName CancelKeyPress -Action {
    $script:stopRequested = $true
    $EventArgs.Cancel = $true
}

try {
    while (-not $script:stopRequested) {
        foreach ($worker in $workers) {
            if ($null -ne $worker.Job) {
                Receive-WorkerOutput -Worker $worker
            }
            if ($null -ne $worker.Job -and $worker.Job.State -in @('Failed', 'Stopped', 'Completed')) {
                Complete-SlotWorker -Worker $worker
            }
            if ($null -eq $worker.Job -and $clock.Elapsed.TotalSeconds -ge $worker.RestartAt) {
                $worker.Result = $null
                $worker.Finalized = $false
                $worker.Job = Invoke-SlotWorkerJob -Entry $worker.Entry -Slot $worker.Slot -BackoffSeconds $worker.RunnerBackoff
                $worker.StartedAt = $clock.Elapsed.TotalSeconds
            }
        }
        Start-Sleep -Seconds 2
    }
} finally {
    Write-Information 'Shutdown requested - stopping every slot''s current container.'
    # finalize before Stop-Job: an ordinary worker can still be in a native wait.
    $stopped = New-Object System.Collections.Generic.HashSet[string]
    $shutdownClock = [Diagnostics.Stopwatch]::StartNew()
    Invoke-KnownContainerStop -Workers $workers -Stopped $stopped -Clock $shutdownClock
    $mixedShutdown = @($workers | Where-Object { $_.Entry.Diagnostics }).Count -gt 0
    foreach ($worker in $workers) {
        if ($null -ne $worker.Job -and ($worker.Entry.Diagnostics -or -not $mixedShutdown)) {
            Stop-Job -Job $worker.Job -ErrorAction SilentlyContinue
        }
    }
    # a container a slot started between the first pass and the job stopping.
    Invoke-KnownContainerStop -Workers $workers -Stopped $stopped -Clock $shutdownClock
    foreach ($worker in $workers) { Complete-WorkerDiagnostic -Worker $worker -Clock $shutdownClock }
    if ($mixedShutdown) {
        foreach ($worker in $workers) {
            if ($null -ne $worker.Job -and -not $worker.Entry.Diagnostics) { Stop-Job -Job $worker.Job -ErrorAction SilentlyContinue }
        }
        Invoke-KnownContainerStop -Workers $workers -Stopped $stopped -Clock $shutdownClock
    }
    foreach ($worker in $workers) {
        if ($null -ne $worker.Job) {
            Wait-Job -Job $worker.Job -Timeout 30 | Out-Null
            Remove-Job -Job $worker.Job -Force -ErrorAction SilentlyContinue
        }
        try { Invoke-CpuProbeRemoval -Name "$($worker.Entry.Name)-cpu-probe-$($worker.Slot)" }
        catch { Write-Warning "$($worker.Key): probe removal unconfirmed; next launch must reconcile it." }
    }
    Get-EventSubscriber | Unregister-Event -ErrorAction SilentlyContinue
    Write-Information 'Runner supervisor stopped.'
}
