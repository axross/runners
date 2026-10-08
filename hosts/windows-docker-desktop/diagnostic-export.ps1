#Requires -Version 5.1
$ErrorActionPreference = 'Stop'
$script:DiagnosticStopMilliseconds = 12000
$script:DiagnosticExportMilliseconds = 18000
$script:DiagnosticHostMilliseconds = 24000
$script:DiagnosticTerminationMilliseconds = 1000

# evidence is private operator data, not a job mount or an automatic upload.
function Assert-PrivateDiagnosticDirectory {
    param([string]$Path)

    $full = [IO.Path]::GetFullPath($Path)
    $checkout = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd([IO.Path]::DirectorySeparatorChar)
    if ($full -eq $checkout -or $full.StartsWith($checkout + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'diagnostic directory must be outside the checkout'
    }
    $item = Get-Item -LiteralPath $full -Force
    if (-not $item.PSIsContainer) { throw 'diagnostic directory must be pre-created' }
    for ($ancestor = $item; $null -ne $ancestor; $ancestor = $ancestor.Parent) {
        if (($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'diagnostic directory has a reparse point' }
    }
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $acl = Get-Acl -LiteralPath $full
        if ($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid) { throw 'diagnostic directory owner differs' }
        foreach ($rule in $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
            if ($rule.AccessControlType -eq 'Allow' -and $rule.IdentityReference.Value -ne $sid) {
                throw 'diagnostic directory grants access to another account'
            }
        }
    } else {
        $mode = $item.UnixFileMode
        if ($null -eq $mode -or ([int]$mode -band 63) -ne 0) { throw 'diagnostic directory must be private (0700)' }
        $owner = @(& stat -c '%u' -- $full)
        $statExit = $LASTEXITCODE
        $current = @(& id -u)
        if ($statExit -ne 0 -or $LASTEXITCODE -ne 0 -or "$owner" -ne "$current") { throw 'diagnostic directory owner differs' }
    }
    return $full
}

# quotes only at the ProcessStartInfo boundary; callers still supply arrays.
function ConvertTo-NativeArgument {
    param([string]$Value)

    return '"' + ([regex]::Replace(([regex]::Replace($Value, '(\\*)"', '$1$1\"')), '(\\+)$', '$1$1')) + '"'
}

# redirects raw bytes instead of passing tar data through PowerShell's text stream.
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

# drains pipes without retaining daemon text, which can contain private host data.
function Invoke-BoundedDiagnosticStop {
    [CmdletBinding()]
    param([string[]]$Names, [Diagnostics.Stopwatch]$Clock)

    $process = $null
    try {
        $deadline = [Math]::Min($Clock.ElapsedMilliseconds + $script:DiagnosticStopMilliseconds, $script:DiagnosticExportMilliseconds)
        if ($Clock.ElapsedMilliseconds -ge $deadline) { throw 'diagnostic stop timeout' }
        $process = Invoke-DiagnosticDocker -Arguments (@('stop') + $Names)
        $null = $process.StandardOutput.BaseStream.CopyToAsync([IO.Stream]::Null)
        $null = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
        if (-not $process.WaitForExit([Math]::Max(1, $deadline - $Clock.ElapsedMilliseconds)) -or $process.ExitCode -ne 0) {
            throw 'diagnostic stop failed or timed out'
        }
    } catch {
        Write-Warning 'Diagnostic gap: stop unavailable or over budget; export and removal will still be attempted.'
    } finally {
        if ($null -ne $process) {
            try {
                if (-not $process.HasExited) {
                    $process.Kill()
                    if (-not $process.WaitForExit($script:DiagnosticTerminationMilliseconds)) { throw 'diagnostic stop client termination unavailable' }
                }
            } catch {
                Write-Warning 'Diagnostic gap: stop client termination unavailable.'
            } finally { $process.Dispose() }
        }
    }
}

# one monotonic deadline covers all bytes, including blocked or truncated copies.
function Read-DiagnosticByte {
    param([IO.Stream]$Stream, [int]$Count, [Diagnostics.Stopwatch]$Clock, [int]$DeadlineMilliseconds = $script:DiagnosticExportMilliseconds)

    $bytes = New-Object byte[] $Count
    $offset = 0
    while ($offset -lt $Count) {
        $remaining = $DeadlineMilliseconds - [int]$Clock.ElapsedMilliseconds
        if ($remaining -le 0) { throw 'diagnostic finalization timeout' }
        $task = $Stream.ReadAsync($bytes, $offset, $Count - $offset)
        if (-not $task.Wait($remaining)) { throw 'diagnostic finalization timeout' }
        $read = $task.Result
        if ($read -eq 0) { throw 'diagnostic copy incomplete' }
        $offset += $read
    }
    return ,$bytes
}

# accepts a deliberately small tar subset, never extracting paths from the job.
function Copy-DiagnosticTar {
    param([IO.Stream]$Stream, [string]$Bundle, [bool]$RawRecords, [Diagnostics.Stopwatch]$Clock)

    $payload = 4096L
    $rawPayload = 0L
    $seen = @{}
    for ($entry = 0; $entry -lt 132; $entry++) {
        $header = Read-DiagnosticByte -Stream $Stream -Count 512 -Clock $Clock
        if (@($header | Where-Object { $_ -ne 0 }).Count -eq 0) {
            if (-not $seen.ContainsKey('metrics.txt')) { throw 'diagnostic collector missing' }
            return
        }
        $name = [Text.Encoding]::ASCII.GetString($header, 0, 100).TrimEnd([char]0)
        $prefix = [Text.Encoding]::ASCII.GetString($header, 345, 155).TrimEnd([char]0)
        $sizeText = [Text.Encoding]::ASCII.GetString($header, 124, 12).Trim([char]0, [char]32)
        if ($sizeText -cnotmatch '\A[0-7]{1,11}\z' -or $prefix.Length -ne 0) { throw 'diagnostic tar header rejected' }
        $size = [Convert]::ToInt64($sizeText, 8)
        $checksumText = [Text.Encoding]::ASCII.GetString($header, 148, 8).Trim([char]0, [char]32)
        if ($checksumText -cnotmatch '\A[0-7]{1,7}\z') { throw 'diagnostic tar checksum rejected' }
        $checksum = 0L
        for ($byte = 0; $byte -lt 512; $byte++) {
            if ($byte -ge 148 -and $byte -lt 156) { $checksum += 32 } else { $checksum += $header[$byte] }
        }
        if ($checksum -ne [Convert]::ToInt64($checksumText, 8)) { throw 'diagnostic tar checksum rejected' }
        if ($header[156] -eq 53 -and $name -in @('./', 'runner-diagnostics/', '.')) {
            if ($size -ne 0) { throw 'diagnostic tar directory rejected' }
            continue
        }
        if ($header[156] -notin @(0, 48)) { throw 'diagnostic tar links or special records rejected' }
        if ($name.StartsWith('./')) { $name = $name.Substring(2) }
        if ($name.StartsWith('runner-diagnostics/')) { $name = $name.Substring(19) }
        $raw = $name -cmatch '\Araw-[0-9]{1,3}\.log\z'
        if ($name -cne 'metrics.txt' -and -not ($RawRecords -and $raw)) { throw 'diagnostic tar filename rejected' }
        if ($seen.ContainsKey($name)) { throw 'diagnostic tar duplicate rejected' }
        $seen[$name] = $true
        if ($raw) {
            $rawPayload += $size
            if ($rawPayload -gt 32MB) { throw 'diagnostic raw budget exceeded' }
        } elseif ($size -gt 8MB) { throw 'diagnostic metrics budget exceeded' }
        $payload += $size
        if ($payload -gt 40MB) { throw 'diagnostic bundle budget exceeded' }
        $null = Assert-PrivateDiagnosticDirectory -Path $Bundle
        $target = Join-Path $Bundle $name
        $file = [IO.File]::Open($target, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $left = $size
            while ($left -gt 0) {
                $count = [int][Math]::Min(65536, $left)
                $bytes = Read-DiagnosticByte -Stream $Stream -Count $count -Clock $Clock
                $file.Write($bytes, 0, $count)
                $left -= $count
            }
        } finally { $file.Dispose() }
        $padding = [int]((512 - ($size % 512)) % 512)
        if ($padding -gt 0) { $null = Read-DiagnosticByte -Stream $Stream -Count $padding -Clock $Clock }
    }
    throw 'diagnostic tar entry limit exceeded'
}

# inspect's format is a fixed allowlist, never the full configuration or logs.
function Get-DiagnosticIdentity {
    param([string]$Name, [Diagnostics.Stopwatch]$Clock)

    $format = '{{.Image}}|{{.HostConfig.NanoCpus}}|{{.HostConfig.Memory}}|{{.HostConfig.MemorySwap}}|{{.HostConfig.CpuQuota}}|{{.HostConfig.CpuPeriod}}|{{.HostConfig.CpusetCpus}}|{{.State.OOMKilled}}|{{.State.ExitCode}}'
    $process = Invoke-DiagnosticDocker -Arguments @('inspect', '--format', $format, $Name)
    try {
        $stream = $process.StandardOutput.BaseStream
        $buffer = New-Object byte[] 1024
        $offset = 0
        while ($offset -lt $buffer.Length) {
            $task = $stream.ReadAsync($buffer, $offset, $buffer.Length - $offset)
            $remaining = $script:DiagnosticExportMilliseconds - [int]$Clock.ElapsedMilliseconds
            if ($remaining -le 0 -or -not $task.Wait($remaining)) { throw 'diagnostic inspect timeout' }
            if ($task.Result -eq 0) { break }
            $offset += $task.Result
        }
        if ($offset -eq $buffer.Length) { throw 'diagnostic inspect output limit' }
        $text = [Text.Encoding]::ASCII.GetString($buffer, 0, $offset).Trim()
        if (-not $process.WaitForExit([Math]::Max(1, $script:DiagnosticExportMilliseconds - [int]$Clock.ElapsedMilliseconds)) -or $process.ExitCode -ne 0) { throw 'diagnostic inspect failed' }
        if ($text -cnotmatch '\Asha256:[0-9a-f]{64}\|[0-9]+\|[0-9]+\|-?[0-9]+\|-?[0-9]+\|[0-9]+\|[0-9,-]*\|(true|false)\|-?[0-9]+\z') { throw 'diagnostic inspect readback unavailable' }
        return "image|nano_cpus|memory_bytes|memory_swap_bytes|cpu_quota|cpu_period|allowed_cpus|container_oom_killed|container_exit_code`n$text`n"
    } finally {
        if (-not $process.HasExited) { $process.Kill() }
        $process.Dispose()
    }
}

# serializes bundle admission across an entry's slots, including partial bundles.
function Export-RunnerDiagnostic {
    [CmdletBinding()]
    param([string]$Name, [string]$EntryName, [string]$Directory, [bool]$RawRecords, [Diagnostics.Stopwatch]$Clock)

    $sink = Assert-PrivateDiagnosticDirectory -Path $Directory
    $hash = [Security.Cryptography.SHA256]::Create()
    try { $key = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes("$sink|$EntryName"))).Replace('-', '') } finally { $hash.Dispose() }
    $mutex = New-Object Threading.Mutex $false, "runners-diagnostics-$key"
    $locked = $false
    try {
        try { $locked = $mutex.WaitOne(500) } catch [Threading.AbandonedMutexException] { $locked = $true }
        if (-not $locked) { throw 'diagnostic sink busy' }
        $existing = @(Get-ChildItem -LiteralPath $sink -Force | Where-Object { $_.Name.StartsWith("$EntryName-bundle-") })
        if ($existing.Count -ge 10) { throw 'diagnostic sink full (ten bundles)' }
        $bundle = Join-Path $sink "$EntryName-bundle-$([Guid]::NewGuid().ToString('N'))"
        $null = New-Item -ItemType Directory -Path $bundle
        if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
            $acl = Get-Acl -LiteralPath $bundle
            $acl.SetOwner([Security.Principal.WindowsIdentity]::GetCurrent().User)
            Set-Acl -LiteralPath $bundle -AclObject $acl
        } else { [IO.File]::SetUnixFileMode($bundle, 448) }
        $null = Assert-PrivateDiagnosticDirectory -Path $bundle
    } finally {
        if ($locked) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
    $identity = Get-DiagnosticIdentity -Name $Name -Clock $Clock
    [IO.File]::WriteAllText((Join-Path $bundle 'identity.txt'), $identity, [Text.Encoding]::ASCII)
    $process = Invoke-DiagnosticDocker -Arguments @('cp', "${Name}:/tmp/runner-diagnostics/.", '-')
    try {
        Copy-DiagnosticTar -Stream $process.StandardOutput.BaseStream -Bundle $bundle -RawRecords $RawRecords -Clock $Clock
        if (-not $process.WaitForExit([Math]::Max(1, $script:DiagnosticExportMilliseconds - [int]$Clock.ElapsedMilliseconds)) -or $process.ExitCode -ne 0) { throw 'diagnostic copy failed' }
    } finally {
        if (-not $process.HasExited) { $process.Kill() }
        $process.Dispose()
    }
    $metrics = [IO.File]::ReadAllText((Join-Path $bundle 'metrics.txt'))
    if ($metrics -notmatch '(?m)^final=observed\r?$') {
        Write-Warning 'Diagnostic gap: final observation unavailable (abrupt exit or observation limit).'
    }
    [IO.File]::WriteAllText((Join-Path $bundle 'complete.txt'), 'export complete; evidence remains untrusted and private', [Text.Encoding]::ASCII)
}

# isolates all storage operations, including synchronous I/O, behind one deadline.
function Invoke-BoundedDiagnosticExport {
    param([string]$Name, [string]$EntryName, [string]$Directory, [bool]$RawRecords, [Diagnostics.Stopwatch]$Clock,
        [int]$DeadlineMilliseconds = $script:DiagnosticExportMilliseconds, [string]$ScriptPath = (Join-Path $PSScriptRoot 'export-runner-diagnostics.ps1'))

    if ($Clock.ElapsedMilliseconds -ge $DeadlineMilliseconds) { throw 'diagnostic finalization timeout' }
    $arguments = @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $ScriptPath, '-Name', $Name, '-EntryName', $EntryName, '-Directory', $Directory)
    if ($RawRecords) { $arguments += '-RawRecords' }
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = (Get-Process -Id $PID).Path
    $info.Arguments = (@($arguments | ForEach-Object { ConvertTo-NativeArgument -Value $_ }) -join ' ')
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    try {
        $null = $process.Start()
        if (-not $process.WaitForExit([Math]::Max(1, $DeadlineMilliseconds - [int]$Clock.ElapsedMilliseconds))) {
            throw 'diagnostic finalization timeout'
        }
        switch ($process.ExitCode) {
            0 { }
            3 { Write-Warning 'Diagnostic gap: final observation unavailable (abrupt exit or observation limit).' }
            4 { throw 'diagnostic sink full (ten bundles)' }
            default { throw 'diagnostic export failed' }
        }
    } finally {
        if (-not $process.HasExited) {
            if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
                $killInfo = New-Object Diagnostics.ProcessStartInfo
                $killInfo.FileName = Join-Path ([Environment]::SystemDirectory) 'taskkill.exe'
                $killInfo.Arguments = "/PID $($process.Id) /T /F"
                $killInfo.UseShellExecute = $false
                $killInfo.CreateNoWindow = $true
                $killInfo.RedirectStandardOutput = $true
                $killInfo.RedirectStandardError = $true
                $kill = [Diagnostics.Process]::Start($killInfo)
                try { if (-not $kill.WaitForExit($script:DiagnosticTerminationMilliseconds)) { $kill.Kill() } } finally { $kill.Dispose() }
                if (-not $process.HasExited) { $process.Kill() }
            } else { $process.Kill($true) }
            if (-not $process.WaitForExit($script:DiagnosticTerminationMilliseconds)) { Write-Warning 'Diagnostic gap: exporter termination unavailable.' }
        }
        $process.Dispose()
    }
}

# removal is attempted after every export outcome, without replacing runner status.
function Complete-DiagnosticContainer {
    param([string]$Name, [string]$EntryName, [string]$Directory, [bool]$RawRecords,
        [Diagnostics.Stopwatch]$Clock = [Diagnostics.Stopwatch]::StartNew())

    try {
        if ($Clock.ElapsedMilliseconds -ge $script:DiagnosticExportMilliseconds) {
            Write-Warning 'Diagnostic gap: finalization budget spent while waiting; export skipped.'
        } else {
            Invoke-BoundedDiagnosticExport -Name $Name -EntryName $EntryName -Directory $Directory -RawRecords $RawRecords -Clock $Clock
        }
    } catch {
        if ($_.Exception.Message -eq 'diagnostic sink full (ten bundles)') {
            Write-Warning 'Diagnostic gap: private sink full (ten bundles); prior evidence retained.'
        } else {
            Write-Warning 'Diagnostic gap: private export unavailable, incomplete, unsafe or over budget; prior evidence retained.'
        }
    } finally {
        $process = $null
        try {
            $process = Invoke-DiagnosticDocker -Arguments @('rm', '-f', $Name)
            if (-not $process.WaitForExit([Math]::Max(1, $script:DiagnosticHostMilliseconds - [int]$clock.ElapsedMilliseconds)) -or $process.ExitCode -ne 0) {
                Write-Warning 'Diagnostic gap: container removal failed; entry-specific startup recovery will retry.'
            }
        } catch {
            Write-Warning 'Diagnostic gap: container removal unavailable; entry-specific startup recovery will retry.'
        } finally {
            if ($null -ne $process) {
                if (-not $process.HasExited) { $process.Kill() }
                $process.Dispose()
            }
        }
    }
}

# holds the container status apart from diagnostics and from GitHub's job result.
function Invoke-DiagnosticRunner {
    [CmdletBinding()]
    param([string[]]$Arguments, [string]$Name, [string]$EntryName, [string]$Directory, [bool]$RawRecords)

    try {
        $runnerExit = Invoke-DockerLogged -Arguments $Arguments
    } finally {
        try { Complete-DiagnosticContainer -Name $Name -EntryName $EntryName -Directory $Directory -RawRecords $RawRecords }
        catch { Write-Warning 'Diagnostic gap: finalization failed; entry-specific startup recovery remains required.' }
    }
    return $runnerExit
}
