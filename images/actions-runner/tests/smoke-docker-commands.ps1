#Requires -Version 5.1
<#
.SYNOPSIS
    smoke-test helper, not a host script: runs the Docker command line client
    and builds a job container's arguments for the image smoke tests. the CLI's
    tests hold its launcher guard, probe reader and diagnostic variant equal to
    the CLI's own.

.DESCRIPTION
    Windows PowerShell 5.1 turns a native command's stderr output into a
    terminating error under $ErrorActionPreference = 'Stop' once it is
    redirected, so Invoke-Docker relaxes the preference and returns the exit
    code.
#>

. (Join-Path $PSScriptRoot 'smoke-diagnostic-process-lifetime.ps1')
$script:CpuProbeMilliseconds = 30000
$script:CpuProbeCleanupMilliseconds = 5000
$script:DockerClientTerminationMilliseconds = 1000
$script:CpuProbeOutputBytes = 65536

# runs Docker, returns its exit code and its output with stderr merged in.
function Invoke-Docker {
    param([Parameter(Mandatory)][string[]]$Arguments)

    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = @(& docker @Arguments 2>&1 | ForEach-Object { "$_" })
        return [pscustomobject]@{ ExitCode = $LASTEXITCODE; Output = $output }
    } finally {
        $ErrorActionPreference = $previous
    }
}

# quotes only at the ProcessStartInfo boundary; callers still supply arrays.
function ConvertTo-NativeArgument {
    param([string]$Value)

    return '"' + ([regex]::Replace(([regex]::Replace($Value, '(\\*)"', '$1$1\"')), '(\\+)$', '$1$1')) + '"'
}

# redirected streams keep raw tar bytes and bounded CPU readback out of logs.
function Invoke-DiagnosticDocker {
    param([string[]]$Arguments)

    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = (Get-Command docker -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    $info.Arguments = (@($Arguments | ForEach-Object { ConvertTo-NativeArgument -Value $_ }) -join ' ')
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    $null = $process.Start()
    return $process
}

# ranges are counted without expanding the daemon's entire CPU inventory.
function Select-CpuAffinity {
    param(
        [Parameter(Mandatory)][string]$AvailableCpus,
        [Parameter(Mandatory)][ValidateRange(1, 64)][int]$Count,
        [Parameter(Mandatory)][ValidateRange(0, [long]::MaxValue)][long]$Offset
    )

    if ($AvailableCpus.Length -gt $script:CpuProbeOutputBytes -or
        $AvailableCpus -cnotmatch '^(?:0|[1-9][0-9]*)(?:-(?:0|[1-9][0-9]*))?(?:,(?:0|[1-9][0-9]*)(?:-(?:0|[1-9][0-9]*))?)*\z') {
        throw 'CPU discovery: invalid allowed CPU list.'
    }
    $ranges = New-Object System.Collections.Generic.List[object]
    $availableCount = 0L
    $previousLast = -1L
    foreach ($range in $AvailableCpus.Split(',')) {
        $bounds = $range.Split('-')
        $first = 0
        $last = 0
        if (-not [int]::TryParse($bounds[0], [ref]$first)) { throw 'CPU discovery: unrepresentable CPU ID.' }
        $last = $first
        if ($bounds.Count -eq 2 -and -not [int]::TryParse($bounds[1], [ref]$last)) { throw 'CPU discovery: unrepresentable CPU ID.' }
        if ($first -le $previousLast -or $last -lt $first) { throw 'CPU discovery: allowed CPU list is not ordered and distinct.' }
        $ranges.Add([pscustomobject]@{ First = $first; Last = $last })
        $availableCount += [long]$last - $first + 1
        $previousLast = $last
    }
    if ($Count -gt $availableCount) { throw 'CPU discovery: affinity cardinality exceeds available CPUs.' }

    $selected = New-Object System.Collections.Generic.List[int]
    $start = $Offset % $availableCount
    for ($index = 0; $index -lt $Count; $index++) {
        $position = ($start + $index) % $availableCount
        foreach ($range in $ranges) {
            $length = [long]$range.Last - $range.First + 1
            if ($position -lt $length) {
                $selected.Add([int]($range.First + $position))
                break
            }
            $position -= $length
        }
    }
    $ordered = @($selected | Sort-Object)
    $parts = New-Object System.Collections.Generic.List[string]
    for ($index = 0; $index -lt $ordered.Count; $index++) {
        $first = $ordered[$index]
        $last = $first
        while ($index + 1 -lt $ordered.Count -and [long]$ordered[$index + 1] -eq [long]$last + 1) {
            $index++
            $last = $ordered[$index]
        }
        $text = $first.ToString([Globalization.CultureInfo]::InvariantCulture)
        if ($last -ne $first) { $text += '-' + $last.ToString([Globalization.CultureInfo]::InvariantCulture) }
        $parts.Add($text)
    }
    return $parts -join ','
}

# a shared deadline reserves client termination time; daemon stderr is discarded.
function Invoke-CpuProbeDocker {
    param([string[]]$Arguments, [Diagnostics.Stopwatch]$Clock, [long]$DeadlineMilliseconds)

    $process = $null
    $clientJob = $null
    try {
        $readDeadline = $DeadlineMilliseconds - $script:DockerClientTerminationMilliseconds
        if ($Clock.ElapsedMilliseconds -ge $readDeadline) { throw 'CPU discovery: deadline spent.' }
        if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
            Initialize-DiagnosticProcessContainment
            $clientJob = New-Object Runners.DiagnosticJob
        }
        $process = Invoke-DiagnosticDocker -Arguments $Arguments
        if ($null -ne $clientJob) { $clientJob.Assign($process.Handle) }
        $stderr = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
        $buffer = New-Object byte[] ($script:CpuProbeOutputBytes + 1)
        $length = 0
        while ($true) {
            $read = $process.StandardOutput.BaseStream.ReadAsync($buffer, $length, $buffer.Length - $length)
            $remaining = $readDeadline - $Clock.ElapsedMilliseconds
            if ($remaining -le 0 -or -not $read.Wait([int]$remaining)) { throw 'CPU discovery: Docker client timed out.' }
            $count = $read.GetAwaiter().GetResult()
            if ($count -eq 0) { break }
            $length += $count
            if ($length -gt $script:CpuProbeOutputBytes) { throw 'CPU discovery: Docker output exceeds its bound.' }
        }
        $remaining = $readDeadline - $Clock.ElapsedMilliseconds
        if ($remaining -le 0 -or -not $process.WaitForExit([int]$remaining)) { throw 'CPU discovery: Docker client timed out.' }
        $remaining = $readDeadline - $Clock.ElapsedMilliseconds
        if ($remaining -le 0 -or -not $stderr.Wait([int]$remaining)) { throw 'CPU discovery: Docker error pipe timed out.' }
        return [pscustomobject]@{
            ExitCode = $process.ExitCode
            Output = [Text.Encoding]::ASCII.GetString($buffer, 0, $length)
        }
    } catch {
        throw 'CPU discovery: Docker client unavailable, invalid or over budget.'
    } finally {
        $cleanupFailed = $false
        try {
            if ($null -ne $clientJob -and -not $clientJob.Close()) { throw 'CPU discovery: client containment cleanup failed.' }
        } catch { $cleanupFailed = $true }
        try {
            if ($null -ne $process -and -not $process.HasExited) {
                $process.Kill()
                $remaining = $DeadlineMilliseconds - $Clock.ElapsedMilliseconds
                if ($remaining -le 0 -or -not $process.WaitForExit([int]$remaining)) { throw 'CPU discovery: client termination unconfirmed.' }
            }
        } catch { $cleanupFailed = $true } finally { if ($null -ne $process) { $process.Dispose() } }
        if ($cleanupFailed) { throw 'CPU discovery: client cleanup unconfirmed.' }
    }
}

# immutable marker plus exact name exclude job containers and unmarked collisions.
function Invoke-CpuProbeRemoval {
    param(
        [Parameter(Mandatory)][ValidatePattern('^[a-z0-9][a-z0-9-]*-cpu-probe-[1-9][0-9]*\z')][string]$Name,
        [Diagnostics.Stopwatch]$Clock = [Diagnostics.Stopwatch]::StartNew(),
        [long]$DeadlineMilliseconds = $script:CpuProbeCleanupMilliseconds
    )

    $result = Invoke-CpuProbeDocker -Arguments @('ps', '-a', '--filter', "name=^${Name}$", '--filter',
        'label=runners.cpu-probe=1', '--format', '{{.Names}}') -Clock $Clock -DeadlineMilliseconds $DeadlineMilliseconds
    if ($result.ExitCode -ne 0) { throw 'CPU discovery: probe reconciliation unavailable.' }
    if ($result.Output.Length -eq 0) { return }
    if (($result.Output -replace '\r?\n\z', '') -cne $Name) { throw 'CPU discovery: unexpected probe identity.' }
    $result = Invoke-CpuProbeDocker -Arguments @('rm', '-f', $Name) -Clock $Clock -DeadlineMilliseconds $DeadlineMilliseconds
    if ($result.ExitCode -ne 0) { throw 'CPU discovery: probe removal unconfirmed.' }
}

function Get-SlotCpuAffinity {
    param(
        [Parameter(Mandatory)][string]$ProbeName,
        [Parameter(Mandatory)][string]$ImageName,
        [Parameter(Mandatory)][int]$Count,
        [Parameter(Mandatory)][long]$Offset
    )

    $clock = [Diagnostics.Stopwatch]::StartNew()
    $reader = 'while read -r key value; do if [ "$key" = Cpus_allowed_list: ]; then printf "%s\n" "$value"; exit; fi; done < /proc/self/status; exit 1'
    try {
        Invoke-CpuProbeRemoval -Name $ProbeName -Clock $clock -DeadlineMilliseconds $script:CpuProbeCleanupMilliseconds
        $arguments = @('run', '--rm', '--pull', 'never', '--network', 'none', '--name', $ProbeName,
            '--label', 'runners.cpu-probe=1', $ImageName, 'bash', '-c', $reader)
        $result = Invoke-CpuProbeDocker -Arguments $arguments -Clock $clock -DeadlineMilliseconds $script:CpuProbeMilliseconds
        if ($result.ExitCode -ne 0) { throw 'CPU discovery: allowed CPU probe failed.' }
        $available = $result.Output -replace '\r?\n\z', ''
        $selection = Select-CpuAffinity -AvailableCpus $available -Count $Count -Offset $Offset
    } finally {
        $cleanupDeadline = [Math]::Min($clock.ElapsedMilliseconds, $script:CpuProbeMilliseconds) + $script:CpuProbeCleanupMilliseconds
        Invoke-CpuProbeRemoval -Name $ProbeName -Clock $clock -DeadlineMilliseconds $cleanupDeadline
    }
    return $selection
}

# returns the `docker run` arguments of a job container. memory and swap are both
# -MemoryGb, so there is no swap beyond the limit. `-e NAME` without a value makes
# the Docker client read the registration from its own environment, never a command
# line.
function Get-JobContainerArgument {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Cpus,
        [Parameter(Mandatory)][ValidatePattern('^[0-9]+(?:-[0-9]+)?(?:,[0-9]+(?:-[0-9]+)?)*\z')][string]$CpusetCpus,
        [Parameter(Mandatory)][int]$MemoryGb,
        [string[]]$Mounts = @(),
        [Parameter(Mandatory)][string]$JitConfigVariable,
        [Parameter(Mandatory)][string]$ImageName,
        [Parameter(Mandatory)][string]$RunCommand,
        [bool]$Diagnostics = $false,
        [bool]$DiagnosticRawRecords = $false
    )

    $guard = @'
set -eu
expected=$1
shift
allowed=
if [ ! -r /proc/self/status ]; then
  printf 'CPU affinity readback unavailable.\n' >&2
  exit 125
fi
while read -r key value; do
  if [ "$key" = Cpus_allowed_list: ]; then allowed=$value; break; fi
done < /proc/self/status
if [ "$allowed" != "$expected" ]; then
  printf 'CPU affinity readback mismatch; runner startup refused.\n' >&2
  exit 125
fi
printf 'CPU affinity verified: %s\n' "$allowed"
'@
    $guard = $guard.Replace("`r`n", "`n")

    if ($Diagnostics) {
        $mode = ''
        if ($DiagnosticRawRecords) { $mode = ' raw-run' }
        $launcher = $guard + "`n" + 'if [ -x /usr/local/bin/runner-diagnostics ]; then exec /usr/local/bin/runner-diagnostics' + $mode +
            '; else printf "Diagnostic gap: collector missing.\n" >&2; exec /home/runner/run.sh; fi'
        return @('run', '--pull', 'never', '--name', $Name,
            '--label', 'runners.diagnostic-lifecycle=1',
            '--cpus', $Cpus, '--cpuset-cpus', $CpusetCpus, '--memory', "${MemoryGb}g", '--memory-swap', "${MemoryGb}g") + @($Mounts) +
            @('-e', $JitConfigVariable, $ImageName, 'bash', '-c', $launcher, '--', $CpusetCpus, $RunCommand)
    }
    $launcher = $guard + "`n" + 'exec "$@"'
    return @('run', '--rm', '--pull', 'never', '--name', $Name,
        '--cpus', $Cpus, '--cpuset-cpus', $CpusetCpus, '--memory', "${MemoryGb}g", '--memory-swap', "${MemoryGb}g") + @($Mounts) +
        @('-e', $JitConfigVariable, $ImageName, 'bash', '-c', $launcher, '--', $CpusetCpus, $RunCommand)
}
