#Requires -Version 5.1
<#
.SYNOPSIS
    runs the Docker command line client and reports its exit code, and builds
    the job container's arguments, its name and the pattern that recognizes it.
    dot-sourced by the supervisor and by each slot's background job, so it
    depends on nothing from host-configuration.ps1.

.DESCRIPTION
    Windows PowerShell 5.1 turns a native command's stderr output into a
    terminating error under $ErrorActionPreference = 'Stop' once it is
    redirected. Invoke-Docker relaxes the preference and returns the exit code;
    runner logging uses explicitly quoted process arguments and separate pipes.
#>

. (Join-Path $PSScriptRoot 'diagnostic-process-lifetime.ps1')
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

# runs Docker and writes each output line to the information stream as it
# arrives, so a job's log never reaches the output stream the supervisor parses.
function Invoke-DockerLogged {
    param([Parameter(Mandatory)][string[]]$Arguments)

    return Invoke-DiagnosticContainerRun -Arguments $Arguments
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

# explicit native quoting preserves bash's arguments on Windows PowerShell 5.1.
function Invoke-DiagnosticContainerRun {
    param([string[]]$Arguments)

    $clientJob = $null
    $process = $null
    try {
        try {
            if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
                Initialize-DiagnosticProcessContainment
                $clientJob = New-Object Runners.DiagnosticJob
            }
        } catch { Write-Warning 'Diagnostic gap: runner client lifetime containment unavailable.' }
        $process = Invoke-DiagnosticDocker -Arguments $Arguments
        try { if ($null -ne $clientJob) { $clientJob.Assign($process.Handle) } }
        catch { Write-Warning 'Diagnostic gap: runner client lifetime containment unavailable.' }
        $stdout = $process.StandardOutput.ReadLineAsync()
        $stderr = $process.StandardError.ReadLineAsync()
        while (-not $process.HasExited -or $null -ne $stdout -or $null -ne $stderr) {
            foreach ($pipe in @('stdout', 'stderr')) {
                $read = Get-Variable -Name $pipe -ValueOnly
                if ($null -ne $read -and $read.IsCompleted) {
                    $line = $read.GetAwaiter().GetResult()
                    $next = $null
                    if ($null -ne $line) {
                        Write-Information $line
                        if ($pipe -eq 'stdout') { $next = $process.StandardOutput.ReadLineAsync() }
                        else { $next = $process.StandardError.ReadLineAsync() }
                    }
                    Set-Variable -Name $pipe -Value $next
                }
            }
            Start-Sleep -Milliseconds 20
        }
        return $process.ExitCode
    } finally {
        try {
            if ($null -ne $clientJob -and -not $clientJob.Close()) { throw 'runner client containment cleanup failed' }
            if ($null -ne $process -and -not $process.HasExited) {
                $process.Kill()
                if (-not $process.WaitForExit($script:DockerClientTerminationMilliseconds)) { throw 'runner client termination failed' }
            }
        } catch { Write-Warning 'Diagnostic gap: runner client termination unavailable.' }
        finally { if ($null -ne $process) { $process.Dispose() } }
    }
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

# returns the arguments of the one `docker run` that starts a job container.
# -Cpus is the text Format-CpuCount returns. memory and swap are both
# -MemoryGb, so the container gets no swap beyond its memory limit. `-e NAME`
# without a value makes the Docker client read the registration from its own
# environment, so it is never on a command line.
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

# returns the name of a slot's next job container, which is also its GitHub
# runner name: the entry's name, the 1-based slot index and a 17-digit
# timestamp. the invariant culture keeps the digits ASCII under any calendar.
function Get-JobContainerName {
    param(
        [Parameter(Mandatory)][string]$EntryName,
        [Parameter(Mandatory)][int]$Slot,
        [datetime]$Now = (Get-Date)
    )

    return "$EntryName-$Slot-$($Now.ToString('yyyyMMddHHmmssfff', [System.Globalization.CultureInfo]::InvariantCulture))"
}

# returns the pattern that matches only the names Get-JobContainerName gives
# this entry's containers, so a container of another entry whose name starts
# with this one is not matched. \z rather than $ keeps a trailing newline out.
function Get-JobContainerNamePattern {
    param([Parameter(Mandatory)][string]$Name)

    return '^' + [regex]::Escape($Name) + '-\d+-\d{17}\z'
}
