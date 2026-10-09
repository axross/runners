#Requires -Version 5.1
param([string]$HostDirectory, [string]$Scratch)
$ErrorActionPreference = 'Stop'
. (Join-Path $HostDirectory 'diagnostic-export.ps1')
Initialize-DiagnosticProcessContainment

$concurrentSink = Join-Path $Scratch 'concurrent'
$null = New-Item -ItemType Directory -Path $concurrentSink
Initialize-TestPrivateDirectory -Path $concurrentSink
for ($index = 1; $index -le 8; $index++) { $null = New-Item -ItemType Directory -Path (Join-Path $concurrentSink "example-entry-bundle-partial-$index") }
$fixture = Join-Path $Scratch 'export-fixture.ps1'
$source = @'
param([string]$Name, [string]$EntryName, [string]$Directory, [switch]$RawRecords)
$ErrorActionPreference = 'Stop'
. '__LIBRARY__'
if ([Console]::ReadLine() -cne 'diagnostic-admitted') { exit 5 }
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

# leaves exporter fixtures concurrent; waiting here would hide whole-copy admission locking.
function Invoke-TestDiagnosticExport {
    param([string]$Name)

    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = (Get-Process -Id $PID).Path
    $info.Arguments = (@('-NoProfile', '-NonInteractive', '-File', $fixture, '-Name', $Name, '-EntryName', 'example-entry', '-Directory', $concurrentSink) | ForEach-Object { ConvertTo-NativeArgument -Value $_ }) -join ' '
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.RedirectStandardInput = $true
    $child = [Diagnostics.Process]::Start($info)
    $child.StandardInput.WriteLine('diagnostic-admitted')
    $child.StandardInput.Close()
    return $child
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
    param($Name, $EntryName, $Directory, $RawRecords, $Clock, $DeadlineMilliseconds)

    if ($DeadlineMilliseconds -le 0) { throw 'missing export deadline' }
    try { & $boundedExport -Name $Name -EntryName $EntryName -Directory $Directory -RawRecords $RawRecords -Clock $Clock -DeadlineMilliseconds 2500 -ScriptPath $fixture }
    catch { $script:boundedTimeout = $_.Exception.Message -eq 'diagnostic finalization timeout'; throw }
}
function Invoke-DiagnosticDocker {
    param($Arguments)

    if (($Arguments -join ' ') -cne 'rm -f blocked-storage') { throw 'unexpected removal target' }
    $script:removalAttempted = $true
    $fake = [pscustomobject]@{ HasExited = $true; ExitCode = 0 }
    foreach ($pipe in @('StandardOutput', 'StandardError')) { $fake | Add-Member -NotePropertyName $pipe -NotePropertyValue (New-Object IO.StreamReader (New-Object IO.MemoryStream)) }
    $fake | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Milliseconds) return $Milliseconds -gt 0 -and $Milliseconds -le 24000 }
    $fake | Add-Member -MemberType ScriptMethod -Name Dispose -Value { $this.StandardOutput.Dispose(); $this.StandardError.Dispose() }
    return $fake
}
$clock = [Diagnostics.Stopwatch]::StartNew()
Complete-DiagnosticContainer -Name 'blocked-storage' -EntryName 'example-entry' -Directory $concurrentSink -RawRecords $false -WarningVariable gaps
Assert-Case -Name 'synchronous storage stall is bounded at process boundary' -Passed ($script:boundedTimeout -and $clock.ElapsedMilliseconds -lt 5000) -Detail 'storage I/O blocked finalization'
Assert-Case -Name 'storage timeout reports gap and attempts removal' -Passed ($script:removalAttempted -and "$gaps".Contains('over budget')) -Detail 'timeout skipped removal'
foreach ($pidFile in @('exporter.pid', 'descendant.pid')) {
    $childId = [int][IO.File]::ReadAllText((Join-Path $concurrentSink $pidFile))
    $remaining = Get-Process -Id $childId -ErrorAction SilentlyContinue
    Assert-Case -Name "storage timeout reaps $pidFile" -Passed ($null -eq $remaining -or $remaining.HasExited) -Detail 'exporter process tree survived'
    if ($null -ne $remaining) { $remaining.Dispose() }
}

$info = New-Object Diagnostics.ProcessStartInfo
$info.FileName = (Get-Process -Id $PID).Path
$info.Arguments = (@('-NoProfile', '-NonInteractive', '-File', $fixture, '-Name', 'blocked-storage', '-Directory', $Scratch) | ForEach-Object { ConvertTo-NativeArgument -Value $_ }) -join ' '
$info.UseShellExecute = $false
$info.RedirectStandardInput = $true
$notAdmitted = [Diagnostics.Process]::Start($info)
try {
    $notAdmitted.StandardInput.Close()
    Assert-Case -Name 'failed admission exits without storage writes or descendants' -Passed ($notAdmitted.WaitForExit(5000) -and $notAdmitted.ExitCode -eq 5 -and -not (Test-Path -LiteralPath (Join-Path $Scratch 'exporter.pid')) -and -not (Test-Path -LiteralPath (Join-Path $Scratch 'descendant.pid'))) -Detail 'exporter escaped ready gate'
} finally {
    if (-not $notAdmitted.HasExited) { $notAdmitted.Kill() }
    $null = $notAdmitted.WaitForExit(1000)
    $notAdmitted.Dispose()
}

if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
    & {
        function Invoke-TestNewObject {
            param([string]$TypeName, [object[]]$ArgumentList)
            if ($TypeName -ne 'Runners.DiagnosticJob') { return Microsoft.PowerShell.Utility\New-Object @PSBoundParameters }
            $fake = [pscustomobject]@{}
            $fake | Add-Member -MemberType ScriptMethod -Name Assign -Value { param($Handle) if ($Handle -eq [IntPtr]::Zero) { throw 'missing process' }; throw 'synthetic assignment failure' }
            $fake | Add-Member -MemberType ScriptMethod -Name Terminate -Value {}
            $fake | Add-Member -MemberType ScriptMethod -Name ActiveProcesses -Value { return 0 }
            $fake | Add-Member -MemberType ScriptMethod -Name Close -Value { return $true }
            return $fake
        }
        Set-Alias -Name New-Object -Value Invoke-TestNewObject -Scope Local
        $failedAdmission = $false
        try { & $boundedExport -Name 'blocked-storage' -EntryName 'example-entry' -Directory $Scratch -RawRecords $false -Clock ([Diagnostics.Stopwatch]::StartNew()) -DeadlineMilliseconds 3000 -ScriptPath $fixture }
        catch { $failedAdmission = $_.Exception.Message -eq 'diagnostic containment unavailable' }
        Assert-Case -Name 'Windows failed assignment refuses admission without storage work or native descendants' -Passed ($failedAdmission -and -not (Test-Path -LiteralPath (Join-Path $Scratch 'exporter.pid')) -and -not (Test-Path -LiteralPath (Join-Path $Scratch 'descendant.pid'))) -Detail 'failed containment released exporter'
    }
    $ownerScript = Join-Path $Scratch 'owner-fixture.ps1'
    $ownerSource = @'
param([string]$Library, [string]$Exporter, [string]$Directory)
$ErrorActionPreference = 'Stop'
. $Library
Initialize-DiagnosticProcessContainment
Invoke-BoundedDiagnosticExport -Name 'blocked-storage' -EntryName 'example-entry' -Directory $Directory -RawRecords $false -Clock ([Diagnostics.Stopwatch]::StartNew()) -ScriptPath $Exporter
'@
    [IO.File]::WriteAllText($ownerScript, $ownerSource, [Text.Encoding]::ASCII)
    $ownerSink = Join-Path $Scratch 'owner-death'
    $null = New-Item -ItemType Directory -Path $ownerSink
    $info.Arguments = (@('-NoProfile', '-NonInteractive', '-File', $ownerScript, '-Library', (Join-Path $HostDirectory 'diagnostic-export.ps1'), '-Exporter', $fixture, '-Directory', $ownerSink) | ForEach-Object { ConvertTo-NativeArgument -Value $_ }) -join ' '
    $owner = [Diagnostics.Process]::Start($info)
    $descendantIds = @()
    try {
        $readyClock = [Diagnostics.Stopwatch]::StartNew()
        while (-not (Test-Path -LiteralPath (Join-Path $ownerSink 'descendant.pid')) -and -not $owner.HasExited -and $readyClock.ElapsedMilliseconds -lt 10000) { Start-Sleep -Milliseconds 10 }
        Assert-Case -Name 'Windows owner fixture admits a real exporter descendant' -Passed (Test-Path -LiteralPath (Join-Path $ownerSink 'descendant.pid')) -Detail 'owner death fixture never admitted'
        foreach ($pidFile in @('exporter.pid', 'descendant.pid')) {
            if (Test-Path -LiteralPath (Join-Path $ownerSink $pidFile)) { $descendantIds += [int][IO.File]::ReadAllText((Join-Path $ownerSink $pidFile)) }
        }
        if (-not $owner.HasExited) { $owner.Kill() }
        $null = $owner.WaitForExit(1000)
        foreach ($childId in $descendantIds) {
            $child = Get-Process -Id $childId -ErrorAction SilentlyContinue
            $reaped = $null -eq $child -or $child.WaitForExit(2000)
            Assert-Case -Name 'Windows owner death kills exporter and descendant on last handle close' -Passed $reaped -Detail 'Job Object tree survived owner'
            if ($null -ne $child) { if (-not $child.HasExited) { $child.Kill() }; $child.Dispose() }
        }
    } finally {
        if (-not $owner.HasExited) { $owner.Kill() }
        $null = $owner.WaitForExit(1000)
        $owner.Dispose()
    }
} else { Write-Output 'SKIP  Job Object owner death requires the hosted Windows test route' }
