#Requires -Version 5.1
$ErrorActionPreference = 'Stop'
$cpuScratch = Join-Path ([IO.Path]::GetTempPath()) "runners-cpu-$([Guid]::NewGuid().ToString('N'))"
$null = New-Item -ItemType Directory -Path $cpuScratch
try {
    $config = Get-Content -LiteralPath $example -Raw | ConvertFrom-Json
    $config.repositories[0] | Add-Member -NotePropertyName cpus -NotePropertyValue 1.5 -Force
    $config.repositories[0].slots = 2
    $config.repositories[1] | Add-Member -NotePropertyName cpus -NotePropertyValue 1.0000000001 -Force
    $config.repositories[1].slots = 1
    $configPath = Join-Path $cpuScratch 'plan.json'
    $config | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $configPath -Encoding ASCII
    $cpuPlan = Read-HostConfiguration -Path $configPath
    $validation = Invoke-Validation -ConfigPath $configPath
    $summary = $validation.Text
    $counts = @([regex]::Matches($summary, 'affinity per slot: (\d+) CPUs') | ForEach-Object { $_.Groups[1].Value })
    $positions = @([regex]::Matches($summary, 'affinity position: (\d+)') | ForEach-Object { $_.Groups[1].Value })
    Assert-Case -Name 'offline plan separates quota, raw ceiling cardinality and cumulative entry positions' `
        -Passed ($validation.ExitCode -eq 0 -and ($counts -join '|') -ceq '2|2' -and ($positions -join '|') -ceq '0|4' -and
            $summary.Contains("cpus:              1`n") -and $summary.Contains("cpus:              1.5`n") -and
            $summary.Contains('cpus is CPU-time quota') -and $summary.Contains('IDs discovered at launch')) -Detail 'quota rounded before ceiling or entry positions lost'

    & {
        . (Join-Path $hostDirectory 'docker-commands.ps1')
        $script:inventory = '2,5-6,11,14-15'
        $script:probeFailure = ''
        $script:removed = New-Object System.Collections.Generic.List[string]
        function Invoke-CpuProbeDocker {
            param($Arguments, $Clock, $DeadlineMilliseconds)
            if (-not $Clock.IsRunning -or $Clock.ElapsedMilliseconds -ge $DeadlineMilliseconds) { throw 'probe deadline unavailable' }
            switch ($Arguments[0]) {
                'ps' {
                    if ($Arguments -notcontains 'label=runners.cpu-probe=1') { throw 'missing probe marker' }
                    if ($script:probeFailure -eq 'foreign') { return [pscustomobject]@{ ExitCode = 0; Output = "unrelated-job`n" } }
                    return [pscustomobject]@{ ExitCode = 0; Output = "example-entry-cpu-probe-1`n" }
                }
                'rm' {
                    $script:removed.Add($Arguments[-1])
                    $code = 0
                    if ($script:probeFailure -eq 'cleanup') { $code = 7 }
                    return [pscustomobject]@{ ExitCode = $code; Output = '' }
                }
                'run' {
                    if ($Arguments -contains '-e' -or $Arguments -contains '--mount' -or $Arguments -contains '--cpus' -or
                        $Arguments -contains '--cpuset-cpus' -or $Arguments -notcontains 'none' -or $Arguments -notcontains 'never' -or
                        $Arguments -notcontains 'actions-runner:synthetic') { throw 'probe changed registration, limits or isolation' }
                    $code = 0
                    if ($script:probeFailure -eq 'daemon') { $code = 7 }
                    return [pscustomobject]@{ ExitCode = $code; Output = $script:inventory + "`n" }
                }
                default { throw 'unexpected Docker operation' }
            }
        }
        $sets = @()
        foreach ($entry in $cpuPlan.Repositories) {
            for ($slot = 1; $slot -le $entry.Slots; $slot++) {
                $worker = Get-SlotWorkerArgument -Plan $cpuPlan -Entry $entry -Slot $slot -ScriptRoot $hostDirectory `
                    -RunCommand '/home/runner/run.sh' -JitConfigVariable 'ACTIONS_RUNNER_INPUT_JITCONFIG' -InitialBackoffSeconds 5 -MaxBackoffSeconds 300
                $sets += Get-SlotCpuAffinity -ProbeName 'example-entry-cpu-probe-1' -ImageName 'actions-runner:synthetic' `
                    -Count ([int][Math]::Ceiling($worker.Cpus)) -Offset $worker.CpuAffinityOffset
            }
        }
        foreach ($offset in @(6, 5)) {
            $sets += Get-SlotCpuAffinity -ProbeName 'example-entry-cpu-probe-1' -ImageName 'actions-runner:synthetic' -Count 2 -Offset $offset
        }
        Assert-Case -Name 'measured sparse inventory distributes slots, exactly fills capacity and wraps overcommit' `
            -Passed (($sets -join '|') -ceq '2,5|6,11|14-15|2,5|2,15') -Detail ($sets -join '|')
        foreach ($inventory in @('', '0,0', '4-2', '1, 3', '1;exit', '2,1', '01', '2147483648', '2', "2,5`nextra")) {
            $script:inventory = $inventory
            $rejected = $false
            try { $null = Get-SlotCpuAffinity -ProbeName 'example-entry-cpu-probe-1' -ImageName 'actions-runner:synthetic' -Count 2 -Offset 0 } catch { $rejected = $true }
            Assert-Case -Name "invalid or insufficient probe inventory rejected: $($inventory.Replace("`n", '<newline>'))" -Passed $rejected -Detail 'invalid inventory admitted'
        }
        $script:inventory = '2,5-6,11'
        foreach ($failure in @('daemon', 'cleanup', 'foreign')) {
            $script:probeFailure = $failure
            $script:removed.Clear()
            $rejected = $false
            try { $null = Get-SlotCpuAffinity -ProbeName 'example-entry-cpu-probe-1' -ImageName 'actions-runner:synthetic' -Count 2 -Offset 0 } catch { $rejected = $true }
            Assert-Case -Name "$failure failure never returns unrestricted affinity or removes a foreign container" `
                -Passed ($rejected -and @($script:removed | Where-Object { $_ -cne 'example-entry-cpu-probe-1' }).Count -eq 0 -and
                    ($failure -ne 'foreign' -or $script:removed.Count -eq 0)) -Detail 'unsafe failure result'
        }
    }

    & {
        . (Join-Path $hostDirectory 'docker-commands.ps1')
        $script:clientMode = 'timeout'
        function Invoke-DiagnosticDocker {
            param($Arguments)
            $code = 'exit 0'
            if ($Arguments[0] -eq 'run') {
                $code = "[IO.File]::WriteAllText('$($script:clientPid.Replace("'", "''"))', [string]`$PID); [Console]::Error.Write('SYNTHETIC_PRIVATE_MARKER'); "
                if ($script:clientMode -eq 'timeout') { $code += 'Start-Sleep -Seconds 60' }
                else { $code += "[Console]::Out.Write('x' * 65537)" }
            }
            $info = New-Object Diagnostics.ProcessStartInfo
            $info.FileName = (Get-Process -Id $PID).Path
            $info.Arguments = '-NoProfile -NonInteractive -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($code))
            $info.UseShellExecute = $false
            $info.RedirectStandardOutput = $true
            $info.RedirectStandardError = $true
            return [Diagnostics.Process]::Start($info)
        }
        foreach ($mode in @('timeout', 'overflow')) {
            $script:clientMode = $mode
            $script:clientPid = Join-Path $cpuScratch "client-$mode.pid"
            $clock = [Diagnostics.Stopwatch]::StartNew()
            $errorText = ''
            try { $null = Get-SlotCpuAffinity -ProbeName 'example-entry-cpu-probe-1' -ImageName 'actions-runner:synthetic' -Count 2 -Offset 0 } catch { $errorText = $_.Exception.Message }
            $client = Get-Process -Id ([int][IO.File]::ReadAllText($script:clientPid)) -ErrorAction SilentlyContinue
            Assert-Case -Name "$mode has bounded wait, reaps actual client and discards private stderr" `
                -Passed ($errorText.Length -gt 0 -and -not $errorText.Contains('SYNTHETIC_PRIVATE_MARKER') -and
                    $clock.ElapsedMilliseconds -lt 35500 -and $null -eq $client) -Detail "elapsed $($clock.ElapsedMilliseconds) ms; client reaped: $($null -eq $client)"
            if ($null -ne $client) { $client.Kill(); $client.Dispose() }
        }
    }
} finally { Remove-Item -LiteralPath $cpuScratch -Recurse -Force }
