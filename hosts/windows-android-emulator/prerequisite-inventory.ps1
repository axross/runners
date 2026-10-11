#Requires -Version 5.1
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$WarningPreference = 'SilentlyContinue'

# fixed admission precedes configuration I/O and every Windows or SDK observation.
try {
    $inputReader = New-Object IO.StreamReader ([Console]::OpenStandardInput())
    try {
        if ($inputReader.ReadLine() -cne 'prerequisite-inventory-admitted') { exit 1 }
        $request = $inputReader.ReadLine()
    } finally { $inputReader.Dispose() }
    if ($null -eq $request -or $request.Length -gt 4096) { exit 1 }
    $configurationPath = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($request))
    if ($configurationPath.Length -gt 1024) { exit 1 }
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { exit 1 }
    . (Join-Path $PSScriptRoot 'prerequisite-configuration.ps1')
    $checkout = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
    try { $configuration = Read-PrerequisiteConfiguration -ConfigPath $configurationPath -CheckoutRoot $checkout }
    catch { exit 2 }
} catch { exit 1 }

# refuses network drives and reparse traversal; inaccessible paths remain unknown, not missing.
function Get-PrerequisitePathStatus {
    param([string]$Path, [bool]$Directory)

    try {
        $null = ConvertTo-PrerequisiteRoot -Value $Path -Field 'sdkRoot'
        $drive = New-Object IO.DriveInfo $Path.Substring(0, 3)
        if ($drive.DriveType -ne [IO.DriveType]::Fixed) { return 'unknown' }
        $current = $Path.Substring(0, 3)
        foreach ($component in $Path.Substring(3).Split('\')) {
            $current = [IO.Path]::Combine($current, $component)
            $attributes = [IO.File]::GetAttributes($current)
            if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { return 'unknown' }
        }
        if ((($attributes -band [IO.FileAttributes]::Directory) -ne 0) -ne $Directory) { return 'missing' }
        return 'present'
    } catch [IO.FileNotFoundException] { return 'missing' }
    catch [IO.DirectoryNotFoundException] { return 'missing' }
    catch { return 'unknown' }
}

# accepts only one numeric Pkg.Revision in a fixed, byte-bounded source.properties file.
function Get-PrerequisiteSdkVersion {
    param([string]$Path)

    $maximumMetadataBytes = 8192
    $presence = Get-PrerequisitePathStatus -Path $Path -Directory $false
    if ($presence -ne 'present') { return $presence }
    try {
        $text = Read-BoundedPrerequisiteText -Path $Path -MaximumBytes $maximumMetadataBytes
        $revisions = @($text.Split("`n") | Where-Object { $_ -match '\A[ \t]*Pkg\.Revision[ \t]*=' })
        if ($revisions.Count -ne 1 -or $revisions[0] -cnotmatch '\A[ \t]*Pkg\.Revision[ \t]*=[ \t]*([0-9]{1,6}(?:\.[0-9]{1,6}){1,3})[ \t\r]*\z') {
            return 'unknown'
        }
        return $Matches[1]
    } catch { return 'unknown' }
}

# preserves absent properties and inconsistent CPU reports instead of coercing them to false.
function ConvertTo-PrerequisiteBoolean {
    param([object[]]$Instances, [string]$Property)

    if ($Instances.Count -eq 0) { return 'unknown' }
    $trueCount = 0
    foreach ($instance in $Instances) {
        $value = $instance.$Property
        if ($value -isnot [bool]) { return 'unknown' }
        if ($value) { $trueCount++ }
    }
    if ($trueCount -eq $Instances.Count) { return 'true' }
    if ($trueCount -eq 0) { return 'false' }
    return 'mixed'
}

try {
    $facts = [ordered]@{
        windowsVersion = 'unknown'; windowsSku = 'unknown'
        powershellVersion = $PSVersionTable.PSVersion.ToString()
        osArchitecture = 'unknown'; processArchitecture = 'unknown'
        hypervisorPresent = 'unknown'; virtualizationFirmwareEnabled = 'unknown'
        vmMonitorModeExtensions = 'unknown'; secondLevelAddressTranslation = 'unknown'
    }
    try {
        $operatingSystem = Get-CimInstance -Query 'SELECT Version, OperatingSystemSKU FROM Win32_OperatingSystem' -OperationTimeoutSec 3
        if ($operatingSystem.Version -match '\A[0-9]{1,6}(?:\.[0-9]{1,6}){1,3}\z') { $facts.windowsVersion = $operatingSystem.Version }
        if ($null -ne $operatingSystem.OperatingSystemSKU -and [string]$operatingSystem.OperatingSystemSKU -match '\A[0-9]{1,3}\z') {
            $facts.windowsSku = [string]$operatingSystem.OperatingSystemSKU
        }
    } catch {
        $facts.windowsVersion = 'unknown'
        $facts.windowsSku = 'unknown'
    }
    try {
        $computer = Get-CimInstance -Query 'SELECT HypervisorPresent FROM Win32_ComputerSystem' -OperationTimeoutSec 3
        $facts.hypervisorPresent = ConvertTo-PrerequisiteBoolean -Instances @($computer) -Property 'HypervisorPresent'
    } catch { $facts.hypervisorPresent = 'unknown' }
    try {
        $processors = @(Get-CimInstance -Query 'SELECT VirtualizationFirmwareEnabled, VMMonitorModeExtensions, SecondLevelAddressTranslationExtensions FROM Win32_Processor' -OperationTimeoutSec 3)
        $facts.virtualizationFirmwareEnabled = ConvertTo-PrerequisiteBoolean -Instances $processors -Property 'VirtualizationFirmwareEnabled'
        $facts.vmMonitorModeExtensions = ConvertTo-PrerequisiteBoolean -Instances $processors -Property 'VMMonitorModeExtensions'
        $facts.secondLevelAddressTranslation = ConvertTo-PrerequisiteBoolean -Instances $processors -Property 'SecondLevelAddressTranslationExtensions'
    } catch {
        $facts.virtualizationFirmwareEnabled = 'unknown'
        $facts.vmMonitorModeExtensions = 'unknown'
        $facts.secondLevelAddressTranslation = 'unknown'
    }
    try {
        $osArchitecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
        $processArchitecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString().ToLowerInvariant()
        if ($osArchitecture -cin @('x64', 'x86', 'arm64', 'arm')) { $facts.osArchitecture = $osArchitecture }
        if ($processArchitecture -cin @('x64', 'x86', 'arm64', 'arm')) { $facts.processArchitecture = $processArchitecture }
    } catch {
        $facts.osArchitecture = 'unknown'
        $facts.processArchitecture = 'unknown'
    }
    $facts.sdkRootDirectory = Get-PrerequisitePathStatus -Path $configuration.sdkRoot -Directory $true
    $facts.avdRootDirectory = Get-PrerequisitePathStatus -Path $configuration.avdRoot -Directory $true
    $facts.emulatorVersion = Get-PrerequisiteSdkVersion -Path ([IO.Path]::Combine($configuration.sdkRoot, 'emulator\source.properties'))
    $facts.platformToolsVersion = Get-PrerequisiteSdkVersion -Path ([IO.Path]::Combine($configuration.sdkRoot, 'platform-tools\source.properties'))
    $facts.commandLineToolsVersion = Get-PrerequisiteSdkVersion -Path ([IO.Path]::Combine($configuration.sdkRoot, 'cmdline-tools\latest\source.properties'))
    $facts.emulatorBinary = Get-PrerequisitePathStatus -Path ([IO.Path]::Combine($configuration.sdkRoot, 'emulator\emulator.exe')) -Directory $false
    $facts.adbBinary = Get-PrerequisitePathStatus -Path ([IO.Path]::Combine($configuration.sdkRoot, 'platform-tools\adb.exe')) -Directory $false
    $facts.nodeBinary = 'unknown'
    $facts.nodeVersion = 'unknown'
    $maximumNodeFileBytes = 268435456
    try {
        $node = @(Get-Command -Name node.exe -CommandType Application -ErrorAction Stop)[0]
        $facts.nodeBinary = Get-PrerequisitePathStatus -Path $node.Source -Directory $false
        if ($facts.nodeBinary -eq 'present') {
            $file = New-Object IO.FileInfo $node.Source
            if ($file.Length -le $maximumNodeFileBytes) {
                $metadata = [Diagnostics.FileVersionInfo]::GetVersionInfo($node.Source)
                $versionParts = @($metadata.FileMajorPart, $metadata.FileMinorPart, $metadata.FileBuildPart, $metadata.FilePrivatePart)
                if ($metadata.FileMajorPart -gt 0 -and @($versionParts | Where-Object { $_ -lt 0 -or $_ -gt 999999 }).Count -eq 0) {
                    $facts.nodeVersion = $versionParts -join '.'
                }
            }
        } elseif ($facts.nodeBinary -eq 'missing') { $facts.nodeVersion = 'missing' }
    } catch [System.Management.Automation.CommandNotFoundException] {
        $facts.nodeBinary = 'missing'
        $facts.nodeVersion = 'missing'
    } catch { $facts.nodeVersion = 'unknown' }
    foreach ($key in $facts.Keys) { [Console]::Out.WriteLine($key + '=' + $facts[$key]) }
    exit 0
} catch { exit 1 }
