#Requires -Version 5.1
param([string]$HostDirectory, [string]$Scratch)
$ErrorActionPreference = 'Stop'

$concurrentSink = Join-Path $Scratch 'concurrent'
$null = New-Item -ItemType Directory -Path $concurrentSink
Initialize-TestPrivateDirectory -Path $concurrentSink
for ($index = 1; $index -le 8; $index++) { $null = New-Item -ItemType Directory -Path (Join-Path $concurrentSink "example-entry-bundle-partial-$index") }
$fixture = Join-Path $Scratch 'export-fixture.ps1'
$source = @'
param([string]$Name, [string]$EntryName, [string]$Directory, [switch]$RawRecords)
$ErrorActionPreference = 'Stop'
. '__LIBRARY__'
function Get-DiagnosticTar { __TAR__ }
if ($Name -eq 'blocked-storage') {
    function Assert-PrivateDiagnosticDirectory {
        param($Path)
        [IO.File]::WriteAllText((Join-Path $Directory 'exporter.pid'), "$PID")
        $command = "[IO.File]::WriteAllText('" + (Join-Path $Directory 'descendant.pid').Replace("'", "''") + "', [string]`$PID); Start-Sleep 60"
        $info = New-Object Diagnostics.ProcessStartInfo
        $info.FileName = (Get-Process -Id $PID).Path
        $info.Arguments = '-NoProfile -NonInteractive -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
        $info.UseShellExecute = $false
        $null = [Diagnostics.Process]::Start($info)
        Start-Sleep 60
        return $Path
    }
}
function Get-DiagnosticIdentity {
    param($Name, $Clock)
    if ($Name -eq 'slow') {
        [IO.File]::WriteAllText((Join-Path $Directory 'reserved.txt'), 'reserved')
        Start-Sleep 3
    }
    return 'synthetic allowlisted identity'
}
function Invoke-DiagnosticDocker {
    param($Arguments)
    $fake = [pscustomobject]@{ HasExited = $true; ExitCode = 0; StandardOutput = [pscustomobject]@{ BaseStream = (Get-DiagnosticTar -Name 'metrics.txt' -Size 0) } }
    $fake | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Milliseconds) return $Milliseconds -gt 0 }
    $fake | Add-Member -MemberType ScriptMethod -Name Dispose -Value { $this.StandardOutput.BaseStream.Dispose() }
    return $fake
}
try {
    Export-RunnerDiagnostic -Name $Name -EntryName $EntryName -Directory $Directory -RawRecords $RawRecords.IsPresent -Clock ([Diagnostics.Stopwatch]::StartNew())
    exit 0
} catch {
    if ($_.Exception.Message -eq 'diagnostic sink full (ten bundles)') { exit 4 }
    exit 1
}
'@
$source = $source.Replace('__LIBRARY__', (Join-Path $HostDirectory 'diagnostic-export.ps1').Replace("'", "''")).Replace('__TAR__', ${function:Get-DiagnosticTar}.ToString())
[IO.File]::WriteAllText($fixture, $source, [Text.Encoding]::ASCII)

function Invoke-TestDiagnosticExport {
    param([string]$Name)

    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = (Get-Process -Id $PID).Path
    $info.Arguments = (@('-NoProfile', '-NonInteractive', '-File', $fixture, '-Name', $Name, '-EntryName', 'example-entry', '-Directory', $concurrentSink) | ForEach-Object { ConvertTo-NativeArgument -Value $_ }) -join ' '
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    return [Diagnostics.Process]::Start($info)
}

$slow = Invoke-TestDiagnosticExport -Name 'slow'
$fast = $null
$full = $null
try {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not (Test-Path -LiteralPath (Join-Path $concurrentSink 'reserved.txt')) -and $clock.ElapsedMilliseconds -lt 5000) { Start-Sleep -Milliseconds 10 }
    Assert-Case -Name 'slow export reaches identity after reserving its bundle' -Passed (-not $slow.HasExited -and $clock.ElapsedMilliseconds -lt 5000) -Detail 'slow fixture failed to reserve'
    $fast = Invoke-TestDiagnosticExport -Name 'fast'
    Assert-Case -Name 'second slot exports while first slot is still copying' -Passed ($fast.WaitForExit(2000) -and $fast.ExitCode -eq 0 -and -not $slow.HasExited) -Detail 'whole export remained serialized or second slot failed'
    $full = Invoke-TestDiagnosticExport -Name 'full'
    Assert-Case -Name 'concurrent reservations enforce ten-bundle quota including partials' -Passed ($full.WaitForExit(2000) -and $full.ExitCode -eq 4 -and @(Get-ChildItem -LiteralPath $concurrentSink -Filter '*-bundle-*').Count -eq 10) -Detail 'concurrent quota or retention failed'
    Assert-Case -Name 'first concurrent export finishes without losing its reservation' -Passed ($slow.WaitForExit(5000) -and $slow.ExitCode -eq 0) -Detail 'first slot failed'
} finally {
    foreach ($process in @($slow, $fast, $full)) {
        if ($null -ne $process) {
            if (-not $process.HasExited) { $process.Kill() }
            $null = $process.WaitForExit(1000)
            $process.Dispose()
        }
    }
}

$boundedExport = ${function:Invoke-BoundedDiagnosticExport}
$script:boundedTimeout = $false
$script:removalAttempted = $false
function Invoke-BoundedDiagnosticExport {
    param($Name, $EntryName, $Directory, $RawRecords, $Clock)

    try { & $boundedExport @PSBoundParameters -DeadlineMilliseconds 2500 -ScriptPath $fixture }
    catch { $script:boundedTimeout = $_.Exception.Message -eq 'diagnostic finalization timeout'; throw }
}
function Invoke-DiagnosticDocker {
    param($Arguments)

    if (($Arguments -join ' ') -cne 'rm -f blocked-storage') { throw 'unexpected removal target' }
    $script:removalAttempted = $true
    $fake = [pscustomobject]@{ HasExited = $true; ExitCode = 0 }
    $fake | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Milliseconds) return $Milliseconds -gt 0 -and $Milliseconds -le 24000 }
    $fake | Add-Member -MemberType ScriptMethod -Name Dispose -Value {}
    return $fake
}
function Invoke-DockerLogged {
    param($Arguments)

    if ($Arguments[0] -cne 'run') { throw 'unexpected runner operation' }
    return 7
}
$clock = [Diagnostics.Stopwatch]::StartNew()
$exitCode = Invoke-DiagnosticRunner -Arguments @('run') -Name 'blocked-storage' -EntryName 'example-entry' -Directory $concurrentSink -RawRecords $false -WarningVariable gaps
Assert-Case -Name 'synchronous storage stall is bounded at process boundary' -Passed ($script:boundedTimeout -and $clock.ElapsedMilliseconds -lt 5000) -Detail 'storage I/O blocked finalization'
Assert-Case -Name 'storage timeout preserves runner result, reports gap and attempts removal' -Passed ($exitCode -eq 7 -and $script:removalAttempted -and "$gaps".Contains('over budget')) -Detail 'timeout changed runner result or skipped removal'
foreach ($pidFile in @('exporter.pid', 'descendant.pid')) {
    $childId = [int][IO.File]::ReadAllText((Join-Path $concurrentSink $pidFile))
    $remaining = Get-Process -Id $childId -ErrorAction SilentlyContinue
    Assert-Case -Name "storage timeout reaps $pidFile" -Passed ($null -eq $remaining -or $remaining.HasExited) -Detail 'exporter process tree survived'
    if ($null -ne $remaining) { $remaining.Dispose() }
}
