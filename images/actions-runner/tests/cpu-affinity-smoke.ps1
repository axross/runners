#Requires -Version 7.0
param([Parameter(Mandatory)][string]$ImageName)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../../../hosts/windows-docker-desktop/docker-commands.ps1')
$prefix = 'cpu-smoke-' + [Guid]::NewGuid().ToString('N')
$names = New-Object System.Collections.Generic.List[string]
$probe = "$prefix-cpu-probe-1"
$variable = 'CPU_AFFINITY_TEST'
$previous = [Environment]::GetEnvironmentVariable($variable, 'Process')

# fixed numeric keys are the only observations published from the test runner.
function Read-CpuSmoke {
    param([string[]]$Lines)

    $result = @{}
    foreach ($line in $Lines) {
        if ($line -cmatch '^(process|child|cpuset)=([0-9,-]+)$' -or
            $line -cmatch '^(quota|period|nproc|getconf)=([0-9]+)$' -or
            $line -cmatch '^(layout)=(v1|v2)$') {
            if ($result.ContainsKey($Matches[1])) { throw 'duplicate CPU readback' }
            $result[$Matches[1]] = $Matches[2]
        }
    }
    foreach ($key in @('process', 'child', 'cpuset', 'quota', 'period', 'nproc', 'getconf', 'layout')) {
        if (-not $result.ContainsKey($key)) { throw "CPU readback incomplete: $key" }
    }
    return $result
}

# detached fixtures stay alive only long enough to inspect their actual cgroups.
function Invoke-CpuSmokeContainer {
    param([string]$Name, [string[]]$Arguments, [string]$ExpectedSet, [bool]$Diagnostic = $false)

    $names.Add($Name)
    $run = Invoke-Docker -Arguments (@($Arguments[0], '--detach', '--network', 'none') + $Arguments[1..($Arguments.Count - 1)])
    if ($run.ExitCode -ne 0) { throw 'actual CPU smoke launch failed' }
    $clock = [Diagnostics.Stopwatch]::StartNew()
    do {
        $logs = Invoke-Docker -Arguments @('logs', $Name)
        if ($logs.ExitCode -eq 0 -and ($logs.Output -join "`n") -match '(?m)^period=[0-9]+$') { break }
        Start-Sleep -Milliseconds 100
    } while ($clock.ElapsedMilliseconds -lt 10000)
    $readback = Read-CpuSmoke -Lines $logs.Output
    $inspect = Invoke-Docker -Arguments @('inspect', '--format', '{{.HostConfig.NanoCpus}}|{{.HostConfig.CpusetCpus}}', $Name)
    if ($inspect.ExitCode -ne 0 -or ($inspect.Output -join '') -cne "500000000|$ExpectedSet") { throw 'Docker requested quota/cpuset mismatch' }
    if ([long]$readback.quota * 2 -ne [long]$readback.period) { throw 'effective cgroup quota differs from 0.5' }
    if ($readback.process -cne $readback.child -or $readback.process -cne $readback.cpuset -or
        ($ExpectedSet -ne '' -and $readback.process -cne $ExpectedSet)) { throw 'process, child or effective cgroup affinity mismatch' }
    Write-Information "CPU smoke: requested=$ExpectedSet process=$($readback.process) child=$($readback.child) cgroup=$($readback.cpuset) quota=$($readback.quota)/$($readback.period) layout=$($readback.layout) nproc=$($readback.nproc) getconf=$($readback.getconf)" -InformationAction Continue
    $stop = Invoke-Docker -Arguments @('stop', '--time', '5', $Name)
    if ($stop.ExitCode -ne 0) { throw 'smoke fixture stop failed' }
    $left = Invoke-Docker -Arguments @('ps', '-a', '--filter', "name=^${Name}$", '--format', '{{.Names}}')
    if ($left.ExitCode -ne 0 -or (($left.Output -join '') -ne $(if ($Diagnostic) { $Name } else { '' }))) { throw 'ordinary/diagnostic removal contract changed' }
    return $readback
}

try {
    [Environment]::SetEnvironmentVariable($variable, 'wait', 'Process')
    $controlName = "$prefix-control"
    $control = Invoke-CpuSmokeContainer -Name $controlName -ExpectedSet '' -Arguments @('run', '--rm', '--pull', 'never', '--name', $controlName,
        '--cpus', '0.5', '-e', $variable, $ImageName, '/home/runner/run.sh')
    $allowed = New-Object System.Collections.Generic.List[int]
    foreach ($part in $control.process.Split(',')) {
        $bounds = $part.Split('-')
        $last = [int]$bounds[-1]
        for ($id = [int]$bounds[0]; $id -le $last; $id++) { $allowed.Add($id) }
    }
    if ($allowed.Count -lt 2 -or $allowed.Count -ge 64) { throw 'Hosted topology cannot cover proper subsets and capacity rejection; evidence incomplete.' }
    $sets = @()
    foreach ($slot in @(0, 1)) {
        $sets += Get-SlotCpuAffinity -ProbeName $probe -ImageName $ImageName -Count 1 -Offset $slot
    }
    if ($sets[0] -cne [string]$allowed[0] -or $sets[1] -cne [string]$allowed[1]) { throw 'automatic slot distribution differs from measured order' }
    foreach ($mode in @('ordinary', 'diagnostic', 'raw')) {
        $diagnostic = $mode -ne 'ordinary'
        $name = "$prefix-$mode"
        $set = $sets[[int]$diagnostic]
        $arguments = @(Get-JobContainerArgument -Name $name -Cpus '0.5' -CpusetCpus $set -MemoryGb 1 -JitConfigVariable $variable `
            -ImageName $ImageName -RunCommand '/home/runner/run.sh' -Diagnostics $diagnostic -DiagnosticRawRecords ($mode -eq 'raw'))
        $null = Invoke-CpuSmokeContainer -Name $name -Arguments $arguments -ExpectedSet $set -Diagnostic $diagnostic
    }
    $capacityRejected = $false
    try { $null = Get-SlotCpuAffinity -ProbeName $probe -ImageName $ImageName -Count ($allowed.Count + 1) -Offset 0 } catch { $capacityRejected = $true }
    if (-not $capacityRejected) { throw 'insufficient actual daemon capacity accepted' }

    foreach ($failure in @('mismatch', 'docker-rejection')) {
        $name = "$prefix-$failure"
        $names.Add($name)
        $set = $sets[0]
        if ($failure -eq 'docker-rejection') { $set = [string]($allowed[-1] + 1) }
        $arguments = @(Get-JobContainerArgument -Name $name -Cpus '0.5' -CpusetCpus $set -MemoryGb 1 -JitConfigVariable $variable `
            -ImageName $ImageName -RunCommand '/home/runner/run.sh')
        if ($failure -eq 'mismatch') { $arguments[-2] = $sets[1] }
        $result = Invoke-Docker -Arguments (@($arguments[0], '--network', 'none') + $arguments[1..($arguments.Count - 1)])
        if ($result.ExitCode -ne 125 -or ($result.Output -join "`n") -match '(?m)^process=') { throw "$failure started a synthetic runner" }
    }
    Write-Output 'Actual Docker quota-only, automatic slot affinity, ordinary/diagnostic/raw, capacity and refusal controls passed; processor APIs are observations, not worker-count guarantees.'
} finally {
    [Environment]::SetEnvironmentVariable($variable, $previous, 'Process')
    foreach ($name in $names) { $null = Invoke-Docker -Arguments @('rm', '-f', $name) }
    Invoke-CpuProbeRemoval -Name $probe
}
