#Requires -Version 5.1
$ErrorActionPreference = 'Stop'
. (Join-Path $hostDirectory 'diagnostic-export.ps1')

# builds a small synthetic tar without external archivers or private fixtures.
function Get-DiagnosticTar {
    param([string]$Name = 'metrics.txt', [long]$Size = 15, [byte]$Type = 48, [int]$ActualSize = -1)

    $header = New-Object byte[] 512
    [Text.Encoding]::ASCII.GetBytes($Name).CopyTo($header, 0)
    [Text.Encoding]::ASCII.GetBytes([Convert]::ToString($Size, 8).PadLeft(11, '0')).CopyTo($header, 124)
    $header[156] = $Type
    for ($i = 148; $i -lt 156; $i++) { $header[$i] = 32 }
    $sum = ($header | Measure-Object -Sum).Sum
    [Text.Encoding]::ASCII.GetBytes([Convert]::ToString([long]$sum, 8).PadLeft(6, '0')).CopyTo($header, 148)
    $header[154] = 0
    $stream = New-Object IO.MemoryStream
    $stream.Write($header, 0, 512)
    if ($ActualSize -lt 0) { $ActualSize = [int]$Size }
    $buffer = New-Object byte[] ([Math]::Min($ActualSize, 65536))
    $left = $ActualSize
    while ($left -gt 0) { $count = [Math]::Min($left, $buffer.Length); $stream.Write($buffer, 0, $count); $left -= $count }
    if ($ActualSize -eq $Size) {
        $padding = New-Object byte[] ([int]((512 - ($Size % 512)) % 512) + 1024)
        $stream.Write($padding, 0, $padding.Length)
    }
    $stream.Position = 0
    return ,$stream
}

# gives fixtures the same private storage boundary as an operator's sink.
function Initialize-TestPrivateDirectory {
    param([string]$Path)

    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        $acl = Get-Acl -LiteralPath $Path
        $acl.SetAccessRuleProtection($true, $false)
        $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
        $acl.SetOwner($sid)
        $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule $sid, 'FullControl', 'ContainerInherit, ObjectInherit', 'None', 'Allow'))
        Set-Acl -LiteralPath $Path -AclObject $acl
    } else { [IO.File]::SetUnixFileMode($Path, 448) }
}

# verifies admission before and after the selected byte and path boundaries.
function Test-DiagnosticTarCase {
    param([string]$Case, [string]$Name, [long]$Size, [byte]$Type = 48, [int]$ActualSize = -1, [bool]$Raw = $false, [bool]$Accepted = $false)

    $bundle = Join-Path $scratch ([Guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $bundle
    Initialize-TestPrivateDirectory -Path $bundle
    $stream = Get-DiagnosticTar -Name $Name -Size $Size -Type $Type -ActualSize $ActualSize
    $okay = $true
    try { Copy-DiagnosticTar -Stream $stream -Bundle $bundle -RawRecords $Raw -Clock ([Diagnostics.Stopwatch]::StartNew()) } catch { $okay = $false }
    finally { $stream.Dispose() }
    Assert-Case -Name $Case -Passed ($okay -eq $Accepted) -Detail 'unexpected tar admission result'
    $lengths = @(Get-ChildItem -LiteralPath $bundle -File | ForEach-Object { $_.Length })
    Assert-Case -Name "$Case stays within payload budget" -Passed (($lengths | Measure-Object -Sum).Sum -le 40MB) -Detail 'oversized export'
}

$scratch = Join-Path ([IO.Path]::GetTempPath()) "runners-diagnostics-$([Guid]::NewGuid().ToString('N'))"
$null = New-Item -ItemType Directory -Path $scratch
try {
    $config = Get-Content -LiteralPath $example -Raw | ConvertFrom-Json
    $config.repositories[0] | Add-Member -NotePropertyName diagnostics -NotePropertyValue $true
    $config.repositories[0] | Add-Member -NotePropertyName diagnosticDirectory -NotePropertyValue 'C:\example-evidence'
    $configPath = Join-Path $scratch 'config.json'
    $config | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $configPath -Encoding ASCII
    $diagnosticPlan = Read-HostConfiguration -Path $configPath
    Assert-Case -Name 'diagnostics enabled on one entry only, raw off by default' `
        -Passed ($diagnosticPlan.Repositories[0].Diagnostics -and -not $diagnosticPlan.Repositories[0].DiagnosticRawRecords -and -not $diagnosticPlan.Repositories[1].Diagnostics) -Detail 'opt-in leaked to another entry'
    foreach ($case in @(
        @('diagnostics', 'true', 'diagnostics'),
        @('diagnosticRawRecords', 'true', 'diagnosticRawRecords'),
        @('diagnosticDirectory', '..\outside', 'diagnosticDirectory'),
        @('diagnosticDirectory', 'C:\example\..\outside', 'diagnosticDirectory'),
        @('diagnosticDirectory', $null, 'diagnosticDirectory')
    )) {
        $candidate = $config | ConvertTo-Json -Depth 10 | ConvertFrom-Json
        if ($case[0] -eq 'diagnosticRawRecords') { $candidate.repositories[0] | Add-Member -NotePropertyName diagnosticRawRecords -NotePropertyValue $case[1] }
        else { $candidate.repositories[0].($case[0]) = $case[1] }
        $candidate | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $configPath -Encoding ASCII
        $rejected = $false
        try { $null = Read-HostConfiguration -Path $configPath } catch { $rejected = $_.Exception.Message.Contains($case[2]) }
        Assert-Case -Name "rejects invalid diagnostic $($case[0])" -Passed $rejected -Detail 'invalid diagnostic configuration accepted'
    }
    $config.repositories[0].diagnostics = $false
    $config.repositories[0] | Add-Member -NotePropertyName diagnosticRawRecords -NotePropertyValue $true
    $config | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $configPath -Encoding ASCII
    $rejected = $false
    try { $null = Read-HostConfiguration -Path $configPath } catch { $rejected = $_.Exception.Message.Contains('diagnosticRawRecords: requires diagnostics') }
    Assert-Case -Name 'raw-only configuration rejected' -Passed $rejected -Detail 'raw admitted without diagnostics'

    $base = @{ Name = 'example-entry-1-20240305060708009'; Cpus = '1.5'; MemoryGb = 16; Mounts = $mountArguments; JitConfigVariable = 'ACTIONS_RUNNER_INPUT_JITCONFIG'; ImageName = 'actions-runner:local'; RunCommand = '/home/runner/run.sh' }
    $ordinary = @(Get-JobContainerArgument @base)
    $diagnostic = @(Get-JobContainerArgument @base -Diagnostics $true)
    $raw = @(Get-JobContainerArgument @base -Diagnostics $true -DiagnosticRawRecords $true)
    Assert-Case -Name 'default command and auto-removal unchanged' -Passed ($ordinary[1] -ceq '--rm' -and $ordinary[-1] -ceq '/home/runner/run.sh') -Detail 'default lifecycle changed'
    Assert-Case -Name 'diagnostic lifecycle omits auto-removal and preserves resource and mount arguments' `
        -Passed ($diagnostic -notcontains '--rm' -and ($ordinary[2..($ordinary.Count - 2)] -join '|') -ceq ($diagnostic[1..($diagnostic.Count - 4)] -join '|')) -Detail 'diagnostics changed limits, mounts or registration'
    Assert-Case -Name 'raw launcher needs second opt-in and missing collector has a fallback' `
        -Passed (-not $diagnostic[-1].Contains(' raw-run') -and $raw[-1].Contains(' raw-run') -and $diagnostic[-1].Contains('exec /home/runner/run.sh')) -Detail 'wrong diagnostic launcher'

    Test-DiagnosticTarCase -Case 'accepts regular metrics' -Name 'metrics.txt' -Size 15 -Accepted $true
    Test-DiagnosticTarCase -Case 'accepts Docker directory-relative metrics' -Name './metrics.txt' -Size 0 -Accepted $true
    Test-DiagnosticTarCase -Case 'accepts 8 MiB metrics boundary' -Name 'metrics.txt' -Size 8MB -Accepted $true
    Test-DiagnosticTarCase -Case 'rejects oversized metrics before copy' -Name 'metrics.txt' -Size (8MB + 1) -ActualSize 0
    Test-DiagnosticTarCase -Case 'rejects raw without opt-in' -Name 'raw-1.log' -Size 5
    Test-DiagnosticTarCase -Case 'rejects raw over 32 MiB before copy' -Name 'raw-1.log' -Size (32MB + 1) -ActualSize 0 -Raw $true
    Test-DiagnosticTarCase -Case 'rejects traversal' -Name '../metrics.txt' -Size 5
    Test-DiagnosticTarCase -Case 'rejects absolute path' -Name '/metrics.txt' -Size 5
    Test-DiagnosticTarCase -Case 'rejects arbitrary file' -Name 'configuration.txt' -Size 5
    Test-DiagnosticTarCase -Case 'rejects symlink' -Name 'metrics.txt' -Size 0 -Type 50
    Test-DiagnosticTarCase -Case 'rejects hardlink' -Name 'metrics.txt' -Size 0 -Type 49
    Test-DiagnosticTarCase -Case 'rejects truncated copy but retains bounded partial' -Name 'metrics.txt' -Size 100 -ActualSize 5
    foreach ($over in @(0, 1)) {
        $bundle = Join-Path $scratch "budget-$over"
        $null = New-Item -ItemType Directory -Path $bundle
        Initialize-TestPrivateDirectory -Path $bundle
        $stream = Get-DiagnosticTar -Name 'raw-1.log' -Size 32MB
        $stream.SetLength($stream.Length - 1024)
        $stream.Position = $stream.Length
        $second = Get-DiagnosticTar -Name 'metrics.txt' -Size (8MB - 4096 + $over)
        $second.CopyTo($stream)
        $second.Dispose()
        $stream.Position = 0
        $admitted = $true
        try { Copy-DiagnosticTar -Stream $stream -Bundle $bundle -RawRecords $true -Clock ([Diagnostics.Stopwatch]::StartNew()) } catch { $admitted = $false }
        finally { $stream.Dispose() }
        Assert-Case -Name "40 MiB budget with metadata reservation plus $over byte" -Passed ($admitted -eq ($over -eq 0)) -Detail 'combined budget boundary ignored'
    }
    $timeout = $false
    $stream = New-Object IO.MemoryStream
    try { $null = Read-DiagnosticByte -Stream $stream -Count 1 -Clock ([Diagnostics.Stopwatch]::StartNew()) -DeadlineMilliseconds 0 } catch { $timeout = $_.Exception.Message.Contains('timeout') }
    finally { $stream.Dispose() }
    Assert-Case -Name 'expired finalization deadline prevents reads' -Passed $timeout -Detail 'deadline ignored'
    $privateRejected = $false
    try { $null = Assert-PrivateDiagnosticDirectory -Path (Join-Path $scratch 'missing') } catch { $privateRejected = $true }
    Assert-Case -Name 'missing sink is not created' -Passed ($privateRejected -and -not (Test-Path -LiteralPath (Join-Path $scratch 'missing'))) -Detail 'sink created silently'
    $checkoutRejected = $false
    try { $null = Assert-PrivateDiagnosticDirectory -Path $hostDirectory } catch { $checkoutRejected = $true }
    Assert-Case -Name 'checkout cannot be evidence sink' -Passed $checkoutRejected -Detail 'checkout sink admitted'

    $sink = Join-Path $scratch 'sink'
    $null = New-Item -ItemType Directory -Path $sink
    Initialize-TestPrivateDirectory -Path $sink
    $null = Assert-PrivateDiagnosticDirectory -Path $sink
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        [IO.File]::SetUnixFileMode($sink, 493)
        $insecure = $false
        try { $null = Assert-PrivateDiagnosticDirectory -Path $sink } catch { $insecure = $true }
        Assert-Case -Name 'insecure sink rejected without writes' -Passed $insecure -Detail 'publicly readable sink admitted'
        [IO.File]::SetUnixFileMode($sink, 448)
        $link = Join-Path $scratch 'sink-link'
        $null = New-Item -ItemType SymbolicLink -Path $link -Target $sink
        $linked = $false
        try { $null = Assert-PrivateDiagnosticDirectory -Path $link } catch { $linked = $true }
        Assert-Case -Name 'symlink sink rejected' -Passed $linked -Detail 'symlink sink admitted'

        $previousPath = $env:PATH
        $previousReadback = $env:RUNNER_TEST_READBACK
        $docker = Join-Path $scratch 'docker'
        [IO.File]::WriteAllText($docker, "#!/bin/sh`nprintf '%s\n' `"`$RUNNER_TEST_READBACK`"`n", [Text.Encoding]::ASCII)
        [IO.File]::SetUnixFileMode($docker, 448)
        try {
            $env:PATH = $scratch + [IO.Path]::PathSeparator + $previousPath
            $env:RUNNER_TEST_READBACK = 'sha256:' + ('0' * 64) + '|1500000000|900|900|150000|100000|1,3-4|false|7'
            $identity = Get-DiagnosticIdentity -Name 'example-entry-1-20240305060708009' -Clock ([Diagnostics.Stopwatch]::StartNew())
            Assert-Case -Name 'real subprocess readback records immutable image and only selected resources' -Passed ($identity.Contains($env:RUNNER_TEST_READBACK)) -Detail 'allowlisted readback lost'
            [IO.File]::WriteAllText($docker, "#!/bin/sh`nprintf 'sha256:'`nsleep 0.03`nprintf '%s\n' `"`${RUNNER_TEST_READBACK#sha256:}`"`n", [Text.Encoding]::ASCII)
            $identity = Get-DiagnosticIdentity -Name 'example-entry-1-20240305060708009' -Clock ([Diagnostics.Stopwatch]::StartNew())
            Assert-Case -Name 'fragmented inspect output is read completely within its bound' -Passed ($identity.Contains($env:RUNNER_TEST_READBACK)) -Detail 'fragmented readback lost'
            $env:RUNNER_TEST_READBACK = [Guid]::NewGuid().ToString('N')
            $sensitiveRejected = $false
            try { $null = Get-DiagnosticIdentity -Name 'example-entry-1-20240305060708009' -Clock ([Diagnostics.Stopwatch]::StartNew()) } catch { $sensitiveRejected = -not $_.Exception.Message.Contains($env:RUNNER_TEST_READBACK) }
            Assert-Case -Name 'unfiltered inspect values rejected without echoing private marker' -Passed $sensitiveRejected -Detail 'unfiltered output escaped'
            [IO.File]::WriteAllText($docker, "#!/bin/sh`nexec sleep 60`n", [Text.Encoding]::ASCII)
            $process = Invoke-DiagnosticDocker -Arguments @('cp')
            $clock = [Diagnostics.Stopwatch]::StartNew()
            $timedOut = $false
            try { $null = Read-DiagnosticByte -Stream $process.StandardOutput.BaseStream -Count 512 -Clock $clock -DeadlineMilliseconds 50 }
            catch { $timedOut = $_.Exception.Message.Contains('timeout') }
            finally { if (-not $process.HasExited) { $process.Kill() }; $process.WaitForExit(); $process.Dispose() }
            Assert-Case -Name 'blocked byte stream is time-bounded and child reaped' -Passed ($timedOut -and $clock.ElapsedMilliseconds -lt 2000) -Detail 'blocked export exceeded timeout'
        } finally {
            $env:PATH = $previousPath
            $env:RUNNER_TEST_READBACK = $previousReadback
        }
    }
    function Get-DiagnosticIdentity {
        param($Name, $Clock)
        if ($Name -cne 'example-entry-1-20240305060708009' -or -not $Clock.IsRunning) { throw 'unexpected inspect input' }
        return 'allowlisted synthetic identity'
    }
    function Invoke-DiagnosticDocker {
        param($Arguments)
        if ($Arguments[0] -cne 'cp') { throw 'unexpected copy operation' }
        $fake = [pscustomobject]@{ HasExited = $true; ExitCode = 0; StandardOutput = [pscustomobject]@{ BaseStream = (Get-DiagnosticTar -Name 'metrics.txt' -Size 0) } }
        $fake | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Milliseconds) return $Milliseconds -le 18000 }
        $fake | Add-Member -MemberType ScriptMethod -Name Dispose -Value { $this.StandardOutput.BaseStream.Dispose() }
        return $fake
    }
    for ($index = 1; $index -le 9; $index++) { $null = New-Item -ItemType Directory -Path (Join-Path $sink "example-entry-bundle-partial-$index") }
    Export-RunnerDiagnostic -Name 'example-entry-1-20240305060708009' -EntryName 'example-entry' -Directory $sink -RawRecords $false -Clock ([Diagnostics.Stopwatch]::StartNew())
    $full = $false
    try { Export-RunnerDiagnostic -Name 'example-entry-1-20240305060708009' -EntryName 'example-entry' -Directory $sink -RawRecords $false -Clock ([Diagnostics.Stopwatch]::StartNew()) } catch { $full = $_.Exception.Message.Contains('ten bundles') }
    Assert-Case -Name 'tenth bundle allowed, eleventh rejected, partials count and remain' `
        -Passed ($full -and @(Get-ChildItem -LiteralPath $sink -Force).Count -eq 10 -and @(Get-ChildItem -LiteralPath $sink -Filter '*partial*').Count -eq 9) -Detail 'quota admission or retention failed'

    $script:completionOrder = New-Object System.Collections.Generic.List[string]
    function Export-RunnerDiagnostic {
        param($Name, $EntryName, $Directory, $RawRecords, $Clock)
        if ($Name -cne 'example-entry-1-20240305060708009' -or $EntryName -cne 'example-entry' -or $Directory -cne $scratch -or $RawRecords -or -not $Clock.IsRunning) { throw 'unexpected export input' }
        $script:completionOrder.Add('export')
        throw 'diagnostic sink full (ten bundles)'
    }
    function Invoke-DiagnosticDocker {
        param($Arguments)
        $script:completionOrder.Add(($Arguments -join ' '))
        $fake = [pscustomobject]@{ HasExited = $true; ExitCode = 0 }
        $fake | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Milliseconds) return $Milliseconds -le 30000 }
        $fake | Add-Member -MemberType ScriptMethod -Name Dispose -Value {}
        return $fake
    }
    function Invoke-DockerLogged {
        param($Arguments)
        if ($Arguments[0] -cne 'run') { throw 'unexpected runner operation' }
        $script:completionOrder.Add('run')
        return $script:testRunnerExit
    }
    foreach ($runnerExit in @(0, 7)) {
        $script:testRunnerExit = $runnerExit
        $script:completionOrder.Clear()
        $actualExit = Invoke-DiagnosticRunner -Arguments @('run') -Name 'example-entry-1-20240305060708009' -EntryName 'example-entry' -Directory $scratch -RawRecords $false -WarningVariable gap
        Assert-Case -Name "export failure preserves runner result $runnerExit and attempts removal after export" `
            -Passed ($actualExit -eq $runnerExit -and ($script:completionOrder -join '|') -ceq 'run|export|rm -f example-entry-1-20240305060708009' -and "$gap".Contains('sink full')) -Detail 'runner result changed or removal missing'
    }
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
