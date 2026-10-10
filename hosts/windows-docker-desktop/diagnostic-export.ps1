#Requires -Version 5.1
$ErrorActionPreference = 'Stop'
$script:DiagnosticStopMilliseconds = 12000
$script:DiagnosticExportMilliseconds = 18000
$script:DiagnosticHostMilliseconds = 24000
$script:DiagnosticTerminationMilliseconds = 1000
. (Join-Path $PSScriptRoot 'docker-commands.ps1')
. (Join-Path $PSScriptRoot 'diagnostic-process-lifetime.ps1')

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

# drains pipes without retaining daemon text, which can contain private host data.
function Invoke-BoundedDiagnosticStop {
    [CmdletBinding()]
    param([string[]]$Names, [Diagnostics.Stopwatch]$Clock)

    $process = $null
    $success = $false
    try {
        $deadline = [Math]::Min($Clock.ElapsedMilliseconds + $script:DiagnosticStopMilliseconds, $script:DiagnosticExportMilliseconds)
        if ($Clock.ElapsedMilliseconds -ge $deadline) { throw 'diagnostic stop timeout' }
        $process = Invoke-DiagnosticDocker -Arguments (@('stop') + $Names)
        $null = $process.StandardOutput.BaseStream.CopyToAsync([IO.Stream]::Null)
        $null = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
        if (-not $process.WaitForExit([Math]::Max(1, $deadline - $Clock.ElapsedMilliseconds)) -or $process.ExitCode -ne 0) {
            throw 'diagnostic stop failed or timed out'
        }
        $success = $true
    } catch {
        Write-Warning 'Diagnostic gap: stop unavailable or over budget; export and removal will still be attempted.'
    } finally {
        if ($null -ne $process) {
            try {
                if (-not $process.HasExited) {
                    $process.Kill()
                    $remaining = [Math]::Max(1, [Math]::Min($script:DiagnosticTerminationMilliseconds, $script:DiagnosticHostMilliseconds - $Clock.ElapsedMilliseconds))
                    if (-not $process.WaitForExit($remaining)) { throw 'diagnostic stop client termination unavailable' }
                }
            } catch {
                Write-Warning 'Diagnostic gap: stop client termination unavailable.'
            } finally { $process.Dispose() }
        }
    }
    return $success
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
    $info.RedirectStandardInput = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    $jobObject = $null
    $started = $false
    $assigned = $false
    try {
        $windows = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
        if ($windows -and -not ('Runners.DiagnosticJob' -as [type])) { throw 'diagnostic containment unavailable' }
        try { if ($windows) { $jobObject = New-Object Runners.DiagnosticJob } }
        catch { throw 'diagnostic containment unavailable' }
        $null = $process.Start()
        $started = $true
        $null = $process.StandardOutput.BaseStream.CopyToAsync([IO.Stream]::Null)
        $null = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
        try {
            if ($windows) { $jobObject.Assign($process.Handle); $assigned = $true }
            if ($Clock.ElapsedMilliseconds -ge $DeadlineMilliseconds) { throw 'admission deadline spent' }
            $process.StandardInput.WriteLine('diagnostic-admitted')
            $process.StandardInput.Close()
        } catch { throw 'diagnostic containment unavailable' }
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
        $terminationDeadline = [Math]::Min($Clock.ElapsedMilliseconds + $script:DiagnosticTerminationMilliseconds, $DeadlineMilliseconds + $script:DiagnosticHostMilliseconds - $script:DiagnosticExportMilliseconds)
        try {
            if ($started) { $process.StandardInput.Close() }
            if ($null -ne $jobObject) {
                $jobObject.Terminate()
                while ($jobObject.ActiveProcesses() -gt 0 -and $Clock.ElapsedMilliseconds -lt $terminationDeadline) { Start-Sleep -Milliseconds 10 }
                if ($jobObject.ActiveProcesses() -gt 0) { throw 'exporter tree still active' }
            }
            if (-not $assigned -and $started -and -not $process.HasExited) {
                if ($windows) { $process.Kill() } else { $process.Kill($true) }
            }
            if ($started -and -not $process.WaitForExit([Math]::Max(1, $terminationDeadline - $Clock.ElapsedMilliseconds))) { throw 'exporter still active' }
        } catch {
            Write-Warning 'Diagnostic gap: exporter tree termination failed or unknown.'
        } finally {
            if ($null -ne $jobObject -and -not $jobObject.Close()) { Write-Warning 'Diagnostic gap: exporter containment handle cleanup failed or unknown.' }
            if ($started) { $process.StandardInput.Close() }
            $process.Dispose()
        }
    }
}

# drains removal output because daemon errors are not safe diagnostic evidence.
function Invoke-BoundedContainerRemoval {
    param([string]$Name, [Diagnostics.Stopwatch]$Clock, [int]$DeadlineMilliseconds = $script:DiagnosticHostMilliseconds)

    $process = $null
    $removed = $false
    try {
        $process = Invoke-DiagnosticDocker -Arguments @('rm', '-f', $Name)
        $null = $process.StandardOutput.BaseStream.CopyToAsync([IO.Stream]::Null)
        $null = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
        $removed = $process.WaitForExit([Math]::Max(1, $DeadlineMilliseconds - $Clock.ElapsedMilliseconds)) -and $process.ExitCode -eq 0
        if (-not $removed) { Write-Warning 'Container removal failed or over budget; startup recovery remains required.' }
    } catch { Write-Warning 'Container removal unavailable; startup recovery remains required.' }
    finally {
        if ($null -ne $process) {
            try {
                if (-not $process.HasExited) {
                    $process.Kill()
                    if (-not $process.WaitForExit([Math]::Max(1, [Math]::Min($script:DiagnosticTerminationMilliseconds, $DeadlineMilliseconds - $Clock.ElapsedMilliseconds)))) {
                        throw 'removal client still active'
                    }
                }
            } catch { Write-Warning 'Diagnostic gap: removal client termination failed or unknown.' }
            finally { $process.Dispose() }
        }
    }
    return $removed
}

# removal is attempted after every export outcome, without replacing runner status.
function Complete-DiagnosticContainer {
    [CmdletBinding()]
    param([string]$Name, [string]$EntryName, [string]$Directory, [bool]$RawRecords,
        [Diagnostics.Stopwatch]$Clock = [Diagnostics.Stopwatch]::StartNew(), [long]$CompletedAt = 0)

    $queuedMilliseconds = 0L
    if ($CompletedAt -gt 0) {
        $age = ([Diagnostics.Stopwatch]::GetTimestamp() - $CompletedAt) * 1000 / [Diagnostics.Stopwatch]::Frequency
        $queuedMilliseconds = [long][Math]::Min($script:DiagnosticHostMilliseconds, [Math]::Max(0, $age - $Clock.ElapsedMilliseconds))
    }
    $exportDeadline = [int]($script:DiagnosticExportMilliseconds - $queuedMilliseconds)
    $hostDeadline = [int]($script:DiagnosticHostMilliseconds - $queuedMilliseconds)
    try {
        if ($Clock.ElapsedMilliseconds -ge $exportDeadline) {
            Write-Warning 'Diagnostic gap: finalization budget spent while waiting; export skipped.'
        } else {
            Invoke-BoundedDiagnosticExport -Name $Name -EntryName $EntryName -Directory $Directory -RawRecords $RawRecords -Clock $Clock -DeadlineMilliseconds $exportDeadline
        }
    } catch {
        if ($_.Exception.Message -eq 'diagnostic sink full (ten bundles)') {
            Write-Warning 'Diagnostic gap: private sink full (ten bundles); prior evidence retained.'
        } elseif ($_.Exception.Message -eq 'diagnostic containment unavailable') {
            Write-Warning 'Diagnostic gap: exporter containment unavailable; export not admitted.'
        } else {
            Write-Warning 'Diagnostic gap: private export unavailable, incomplete, unsafe or over budget; prior evidence retained.'
        }
    } finally { $null = Invoke-BoundedContainerRemoval -Name $Name -Clock $Clock -DeadlineMilliseconds $hostDeadline }
}
