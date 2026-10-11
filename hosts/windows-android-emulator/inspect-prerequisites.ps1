#Requires -Version 5.1
param([string]$ConfigPath, [switch]$ValidateOnly)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$clock = [Diagnostics.Stopwatch]::StartNew()
$totalMilliseconds = 30000
$workMilliseconds = $totalMilliseconds - 5000
$maximumOutputBytes = 65536
$maximumConfigurationPathCharacters = 1024

# reports only fixed status fields, never exception text or a configuration value.
function Write-PrerequisiteFailure {
    param([string]$Status)

    [ordered]@{
        schemaVersion = 1
        mode = $(if ($ValidateOnly) { 'validation' } else { 'inventory' })
        status = $Status
        fields = @('sdkRoot', 'avdRoot')
        feasibility = 'unverified'
    } | ConvertTo-Json -Compress | Write-Output
}

if ([string]::IsNullOrWhiteSpace($ConfigPath) -or $args.Count -gt 0) {
    Write-PrerequisiteFailure -Status 'invalid_arguments'
    exit 1
}
if ($ValidateOnly) {
    try {
        . (Join-Path $PSScriptRoot 'prerequisite-configuration.ps1')
        $checkout = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
        $null = Read-PrerequisiteConfiguration -ConfigPath $ConfigPath -CheckoutRoot $checkout
        [ordered]@{ schemaVersion = 1; mode = 'validation'; status = 'valid'; feasibility = 'unverified' } |
            ConvertTo-Json -Compress | Write-Output
        exit 0
    } catch {
        Write-PrerequisiteFailure -Status 'invalid_configuration'
        exit 1
    }
}
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    Write-PrerequisiteFailure -Status 'windows_only'
    exit 1
}

$process = $null
$job = $null
$started = $false
$assigned = $false
$cleanupConfirmed = $false
$failure = 'inventory_unavailable'
$facts = $null
try {
    . (Join-Path $PSScriptRoot '../windows-docker-desktop/diagnostic-process-lifetime.ps1')
    Initialize-DiagnosticProcessContainment
    $job = New-Object Runners.DiagnosticJob
    if ($clock.ElapsedMilliseconds -ge $workMilliseconds) { throw 'work deadline' }
    if ($ConfigPath.Length -gt $maximumConfigurationPathCharacters) { throw 'configuration path budget' }
    $info = New-Object Diagnostics.ProcessStartInfo
    $current = [Diagnostics.Process]::GetCurrentProcess()
    try { $info.FileName = $current.MainModule.FileName } finally { $current.Dispose() }
    $arguments = @('-NoProfile', '-NonInteractive', '-File', (Join-Path $PSScriptRoot 'prerequisite-inventory.ps1'))
    $info.Arguments = (@($arguments | ForEach-Object {
                '"' + ([regex]::Replace(([regex]::Replace($_, '(\\*)"', '$1$1\"')), '(\\+)$', '$1$1')) + '"'
            }) -join ' ')
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    $null = $process.Start()
    $started = $true
    $stderr = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
    $job.Assign($process.Handle)
    $assigned = $true
    if ($clock.ElapsedMilliseconds -ge $workMilliseconds) { throw 'admission deadline' }
    $process.StandardInput.WriteLine('prerequisite-inventory-admitted')
    $process.StandardInput.WriteLine([Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($ConfigPath)))
    $process.StandardInput.Close()
    $buffer = New-Object byte[] ($maximumOutputBytes + 1)
    $length = 0
    while ($true) {
        $read = $process.StandardOutput.BaseStream.ReadAsync($buffer, $length, $buffer.Length - $length)
        $remaining = $workMilliseconds - $clock.ElapsedMilliseconds
        if ($remaining -le 0 -or -not $read.Wait([int]$remaining)) { throw 'read deadline' }
        $count = $read.GetAwaiter().GetResult()
        if ($count -eq 0) { break }
        $length += $count
        if ($length -gt $maximumOutputBytes) { throw 'output budget' }
    }
    $remaining = $workMilliseconds - $clock.ElapsedMilliseconds
    if ($remaining -le 0 -or -not $process.WaitForExit([int]$remaining)) { throw 'exit deadline' }
    if ($process.ExitCode -eq 2) { $failure = 'invalid_configuration'; throw 'configuration rejected' }
    if ($process.ExitCode -ne 0) { throw 'worker failed' }
    $remaining = $workMilliseconds - $clock.ElapsedMilliseconds
    if ($remaining -le 0 -or -not $stderr.Wait([int]$remaining)) { throw 'stderr deadline' }

    $version = '(?:unknown|missing|[0-9]{1,6}(?:\.[0-9]{1,6}){1,3})'
    $presence = '(?:present|missing|unknown)'
    $boolean = '(?:true|false|mixed|unknown)'
    $schema = [ordered]@{
        windowsVersion = $version
        windowsSku = '(?:unknown|[0-9]{1,3})'
        powershellVersion = $version
        osArchitecture = '(?:x64|x86|arm64|arm|unknown)'
        processArchitecture = '(?:x64|x86|arm64|arm|unknown)'
        hypervisorPresent = $boolean
        virtualizationFirmwareEnabled = $boolean
        vmMonitorModeExtensions = $boolean
        secondLevelAddressTranslation = $boolean
        sdkRootDirectory = $presence
        avdRootDirectory = $presence
        emulatorVersion = $version
        platformToolsVersion = $version
        commandLineToolsVersion = $version
        emulatorBinary = $presence
        adbBinary = $presence
        nodeBinary = $presence
        nodeVersion = $version
    }
    $facts = [ordered]@{}
    foreach ($line in ([Text.Encoding]::ASCII.GetString($buffer, 0, $length).TrimEnd("`r", "`n").Split("`n"))) {
        $parts = $line.TrimEnd("`r").Split('=')
        if ($parts.Count -ne 2 -or -not $schema.Contains($parts[0]) -or $facts.Contains($parts[0]) -or
            $parts[1] -cnotmatch ('\A' + $schema[$parts[0]] + '\z')) { throw 'invalid worker facts' }
        $facts[$parts[0]] = $parts[1]
    }
    if ($facts.Count -ne $schema.Count) { throw 'incomplete worker facts' }
} catch {
    $facts = $null
} finally {
    $cleanupConfirmed = $true
    try { if ($started) { $process.StandardInput.Close() } }
    catch { $cleanupConfirmed = $false }
    try {
        if ($null -ne $job) {
            $job.Terminate()
            while ($job.ActiveProcesses() -gt 0 -and $clock.ElapsedMilliseconds -lt $totalMilliseconds) {
                Start-Sleep -Milliseconds 10
            }
            if ($job.ActiveProcesses() -ne 0) { throw 'worker tree termination unconfirmed' }
        }
    } catch { $cleanupConfirmed = $false }
    try {
        if ($started) {
            if (-not $assigned -and -not $process.HasExited) { $process.Kill() }
            $remaining = $totalMilliseconds - $clock.ElapsedMilliseconds
            if ($remaining -le 0 -or -not $process.WaitForExit([int]$remaining)) { throw 'worker termination unconfirmed' }
        }
    } catch {
        $cleanupConfirmed = $false
    } finally {
        if ($null -ne $job) {
            try { if (-not $job.Close()) { $cleanupConfirmed = $false } }
            catch { $cleanupConfirmed = $false }
        }
        try { if ($null -ne $process) { $process.Dispose() } }
        catch { $cleanupConfirmed = $false }
    }
}
if (-not $cleanupConfirmed) {
    Write-PrerequisiteFailure -Status 'cleanup_unconfirmed'
    exit 1
}
if ($null -eq $facts -or $clock.ElapsedMilliseconds -ge $totalMilliseconds) {
    Write-PrerequisiteFailure -Status $failure
    exit 1
}
[ordered]@{ schemaVersion = 1; mode = 'inventory'; status = 'collected'; feasibility = 'unverified'; cleanup = 'confirmed'; facts = $facts } |
    ConvertTo-Json -Depth 3 -Compress | Write-Output
exit 0
