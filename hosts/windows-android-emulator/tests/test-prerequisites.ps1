#Requires -Version 5.1
<#
.SYNOPSIS
    exercises the public validation/refusal CLI without installed Android tools.
.DESCRIPTION
    Windows additionally exercises bounded metadata and actual worker-tree cleanup
    using disposable SDK files and a disposable copy of the CLI with fault workers.
    no emulator, ADB server, Node runtime, Docker resource, or host setting is used.
#>
$ErrorActionPreference = 'Stop'
$hostDirectory = Split-Path -Parent $PSScriptRoot
$inspection = Join-Path $hostDirectory 'inspect-prerequisites.ps1'
$powershell = (Get-Process -Id $PID).Path
$windows = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('emulator-prerequisites-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $scratch
$failures = New-Object System.Collections.Generic.List[string]
$marker = 'synthetic-private-marker'
$utf8 = New-Object Text.UTF8Encoding $false

function Assert-Case {
    param([string]$Name, [bool]$Passed)

    if ($Passed) { Write-Output "PASS  $Name" }
    else { Write-Output "FAIL  $Name"; $failures.Add($Name) }
}

# invokes a fresh public CLI and retains output for privacy assertions, not failure logging.
function Invoke-Inspection {
    param([string]$Path, [bool]$Validation = $true, [string]$Script = $inspection)

    $arguments = @('-NoProfile', '-NonInteractive', '-File', $Script, '-ConfigPath', $Path)
    if ($Validation) { $arguments += '-ValidateOnly' }
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $clock = [Diagnostics.Stopwatch]::StartNew()
    try {
        $output = @(& $powershell @arguments 2>&1 | ForEach-Object { "$_" })
        return [pscustomobject]@{ ExitCode = $LASTEXITCODE; Text = $output -join "`n"; Milliseconds = $clock.ElapsedMilliseconds }
    } finally { $ErrorActionPreference = $previous }
}

# writes only synthetic fixture data into the disposable test area.
function New-TestConfiguration {
    [CmdletBinding(SupportsShouldProcess)]
    param([object]$Sdk = 'C:\example-emulator\sdk', [object]$Avd = 'C:\example-emulator\avd', [string]$Text)

    $path = Join-Path $scratch 'configuration.json'
    if (-not $PSBoundParameters.ContainsKey('Text')) { $Text = ConvertTo-Json -InputObject @{ sdkRoot = $Sdk; avdRoot = $Avd } -Compress }
    if ($PSCmdlet.ShouldProcess($path, 'write synthetic configuration')) { [IO.File]::WriteAllText($path, $Text, $utf8) }
    return $path
}

try {
    $result = Invoke-Inspection -Path (Join-Path $hostDirectory 'emulator-host.example.json')
    Assert-Case 'example validates without exposing roots or claiming feasibility' ($result.ExitCode -eq 0 -and $result.Text.Contains('"feasibility":"unverified"') -and -not $result.Text.Contains('example-emulator'))
    $roots = @(
        @('C:\example-sdk', 'C:\example-sdk-child', $true),
        @('C:\example-sdk\child', 'C:\example-sdk', $false),
        @('C:\example-sdk', 'C:\example-sdk\child', $false),
        @('C:\Example-Sdk', 'c:\example-sdk\CHILD', $false),
        @('C:\Example-Sdk', 'c:\example-sdk', $false),
        @('C:\example-sdk\', 'D:\example-avd', $true),
        @('C:\', 'D:\example-avd', $false),
        @('C:example-sdk', 'D:\example-avd', $false),
        @('example-sdk', 'D:\example-avd', $false),
        @('\\example.invalid\share\sdk', 'D:\example-avd', $false),
        @('\\?\C:\example-sdk', 'D:\example-avd', $false),
        @('\\.\C:\example-sdk', 'D:\example-avd', $false),
        @('C:\example\..\sdk', 'D:\example-avd', $false),
        @('C:\example\.\sdk', 'D:\example-avd', $false),
        @('C:\example\sdk.', 'D:\example-avd', $false),
        @('C:\example\sdk ', 'D:\example-avd', $false),
        @('C:\example\\sdk', 'D:\example-avd', $false),
        @('C:\example\CON.txt', 'D:\example-avd', $false),
        @('C:\example\LPT1', 'D:\example-avd', $false),
        @('C:\example\sdk:stream', 'D:\example-avd', $false),
        @('C:\example\sdk*', 'D:\example-avd', $false),
        @('C:\%EXAMPLE_SDK%\sdk', 'D:\example-avd', $false),
        @('C:/example/sdk', 'D:\example-avd', $false),
        @('C:\example\CONIN$', 'D:\example-avd', $false),
        @('C:\example-sdk', 'D:\example\.\avd', $false),
        @(('C:\' + ('x' * 241)), 'D:\example-avd', $false)
    )
    for ($index = 0; $index -lt $roots.Count; $index++) {
        $case = $roots[$index]
        $result = Invoke-Inspection -Path (New-TestConfiguration -Sdk $case[0] -Avd $case[1])
        Assert-Case "root boundary case $index" (($result.ExitCode -eq 0) -eq $case[2] -and -not $result.Text.Contains($case[0]) -and $result.Text.Length -lt 1024)
    }
    $invalidConfigurations = @(
        '{', '{}', '[]', 'null',
        '{"sdkRoot":"C:\\example-sdk"}',
        '{"sdkRoot":12,"avdRoot":"D:\\example-avd"}',
        '{"sdkRoot":null,"avdRoot":"D:\\example-avd"}',
        '{"sdkRoot":[],"avdRoot":"D:\\example-avd"}',
        '{"sdkRoot":{},"avdRoot":"D:\\example-avd"}',
        '{"sdkRoot":"C:\\example-sdk","avdRoot":false}',
        '{"sdkRoot":"C:\\example-sdk","sdkRoot":"D:\\example-avd"}',
        '{"SdkRoot":"C:\\example-sdk","avdRoot":"D:\\example-avd"}',
        '{"sdkRoot":"C:\\example-sdk","avdRoot":"D:\\example-avd",}',
        '{/* synthetic-private-marker */"sdkRoot":"C:\\example-sdk","avdRoot":"D:\\example-avd"}',
        '{"synthetic-private-marker":"C:\\example-sdk","avdRoot":"D:\\example-avd"}',
        '{"sdkRoot":"C:\\example-sdk","avdRoot":"D:\\example-avd","synthetic-private-marker":true}',
        (' ' * 4097)
    )
    for ($index = 0; $index -lt $invalidConfigurations.Count; $index++) {
        $result = Invoke-Inspection -Path (New-TestConfiguration -Text $invalidConfigurations[$index])
        Assert-Case "configuration shape case $index" ($result.ExitCode -ne 0 -and $result.Text.Contains('sdkRoot') -and $result.Text.Contains('avdRoot') -and -not $result.Text.Contains($marker) -and $result.Text.Length -lt 1024)
    }
    $validJson = '{"avdRoot":"D:\\example-avd","sdk\u0052oot":"C:\\example-sdk"}'
    $result = Invoke-Inspection -Path (New-TestConfiguration -Text $validJson)
    Assert-Case 'field order and JSON character escapes preserve the schema' ($result.ExitCode -eq 0)
    foreach ($size in @(4096, 4097)) {
        $text = $validJson + (' ' * ($size - $utf8.GetByteCount($validJson)))
        $result = Invoke-Inspection -Path (New-TestConfiguration -Text $text)
        Assert-Case "configuration byte boundary $size" (($result.ExitCode -eq 0) -eq ($size -eq 4096))
    }
    $result = Invoke-Inspection -Path (New-TestConfiguration -Sdk ('C:\' + $marker + '\..\sdk'))
    Assert-Case 'invalid field values remain private' ($result.ExitCode -ne 0 -and -not $result.Text.Contains($marker))
    $result = Invoke-Inspection -Path (Join-Path $scratch ($marker + '.json'))
    Assert-Case 'missing configuration errors omit path and exception details' ($result.ExitCode -ne 0 -and -not $result.Text.Contains($marker) -and -not $result.Text.Contains($scratch))
    $path = New-TestConfiguration
    [IO.File]::WriteAllBytes($path, [byte[]]@(0xff, 0xfe, 0xff))
    $result = Invoke-Inspection -Path $path
    Assert-Case 'invalid UTF-8 is rejected' ($result.ExitCode -ne 0)

    if (-not $windows) {
        $result = Invoke-Inspection -Path (New-TestConfiguration) -Validation $false
        Assert-Case 'Linux refuses runtime inventory explicitly' ($result.ExitCode -ne 0 -and $result.Text.Contains('windows_only') -and $result.Text.Contains('unverified'))
        Write-Output 'SKIP  Windows metadata, checkout and Job Object cases require the hosted Windows route'
    } else {
        $checkout = [IO.Path]::GetFullPath((Join-Path $hostDirectory '../..'))
        foreach ($sdk in @($checkout, (Join-Path $checkout 'synthetic-sdk'), (Split-Path -Parent $checkout), $checkout.ToUpperInvariant())) {
            $result = Invoke-Inspection -Path (New-TestConfiguration -Sdk $sdk -Avd 'Z:\example-avd')
            Assert-Case 'checkout intersection is rejected without exposing its path' ($result.ExitCode -ne 0 -and -not $result.Text.Contains($checkout))
        }
        $sdk = Join-Path $scratch 'synthetic-sdk'
        $avd = Join-Path $scratch 'synthetic-avd'
        $bin = Join-Path $scratch 'synthetic-bin'
        foreach ($directory in @($sdk, $avd, $bin, (Join-Path $sdk 'emulator'), (Join-Path $sdk 'platform-tools'), (Join-Path $sdk 'cmdline-tools\latest'))) {
            $null = New-Item -ItemType Directory -Path $directory -Force
        }
        $sentinel = Join-Path $scratch 'executed.txt'
        $canary = Join-Path $scratch 'execution-canary.exe'
        $encodedSentinel = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($sentinel))
        $canarySource = 'public static class ExecutionCanary { public static void Main() { System.IO.File.WriteAllText(System.Text.Encoding.Unicode.GetString(System.Convert.FromBase64String("' + $encodedSentinel + '")), "synthetic-private-marker"); } }'
        $compilerCommand = "Add-Type -TypeDefinition '" + $canarySource.Replace("'", "''") + "' -OutputType ConsoleApplication -OutputAssembly '" + $canary.Replace("'", "''") + "'"
        $compiler = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
        & $compiler -NoProfile -NonInteractive -EncodedCommand ([Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($compilerCommand)))
        if ($LASTEXITCODE -ne 0) { throw 'execution canary compilation failed' }
        & $canary
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $sentinel)) { throw 'execution canary positive control failed' }
        Remove-Item -LiteralPath $sentinel
        foreach ($file in @((Join-Path $sdk 'emulator\emulator.exe'), (Join-Path $sdk 'platform-tools\adb.exe'), (Join-Path $bin 'node.exe'))) {
            Copy-Item -LiteralPath $canary -Destination $file
        }
        $trap = '@echo synthetic-private-marker > "' + $sentinel + '"'
        foreach ($file in @((Join-Path $sdk 'emulator\emulator.cmd'), (Join-Path $sdk 'platform-tools\adb.cmd'), (Join-Path $sdk 'cmdline-tools\latest\sdkmanager.bat'), (Join-Path $bin 'node.cmd'))) {
            [IO.File]::WriteAllText($file, $trap, $utf8)
        }
        [IO.File]::WriteAllText((Join-Path $sdk 'emulator\source.properties'), "Pkg.Revision=35.1.2`nPkg.Desc=$marker", $utf8)
        [IO.File]::WriteAllText((Join-Path $sdk 'platform-tools\source.properties'), "Pkg.Revision=35.0.1`nPkg.Desc=$marker", $utf8)
        [IO.File]::WriteAllText((Join-Path $sdk 'cmdline-tools\latest\source.properties'), "Pkg.Revision=$marker", $utf8)
        $config = New-TestConfiguration -Sdk $sdk -Avd $avd
        $result = Invoke-Inspection -Path $config
        Assert-Case 'ValidateOnly neither reads SDK metadata nor executes SDK fixtures' ($result.ExitCode -eq 0 -and -not $result.Text.Contains('35.1.2') -and -not $result.Text.Contains($scratch) -and -not $result.Text.Contains($marker) -and -not (Test-Path -LiteralPath $sentinel))
        $metadataLock = [IO.File]::Open((Join-Path $sdk 'emulator\source.properties'), [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
        try {
            $result = Invoke-Inspection -Path $config
            Assert-Case 'ValidateOnly works even when SDK metadata is exclusively locked' ($result.ExitCode -eq 0)
        } finally { $metadataLock.Dispose() }
        $oldPath = $env:PATH
        try {
            $env:PATH = $bin
            $result = Invoke-Inspection -Path $config -Validation $false
            $report = ConvertFrom-Json -InputObject $result.Text
            Assert-Case 'Windows reads allowlisted metadata, not SDK or Node programs' ($result.ExitCode -eq 0 -and $report.cleanup -eq 'confirmed' -and $report.feasibility -eq 'unverified' -and $report.facts.emulatorVersion -eq '35.1.2' -and $report.facts.platformToolsVersion -eq '35.0.1' -and $report.facts.commandLineToolsVersion -eq 'unknown' -and $report.facts.emulatorBinary -eq 'present' -and $report.facts.adbBinary -eq 'present' -and $report.facts.nodeBinary -eq 'present' -and $report.facts.nodeVersion -eq 'unknown' -and -not $result.Text.Contains($marker) -and -not $result.Text.Contains($scratch) -and -not (Test-Path -LiteralPath $sentinel) -and $result.Text.Length -lt 65536 -and $result.Milliseconds -lt 30000)
            $metadataPrefix = "Pkg.Revision=35.1.2`nPkg.Desc="
            foreach ($size in @(8192, 8193)) {
                [IO.File]::WriteAllText((Join-Path $sdk 'emulator\source.properties'), ($metadataPrefix + ('x' * ($size - $utf8.GetByteCount($metadataPrefix)))), $utf8)
                $result = Invoke-Inspection -Path $config -Validation $false
                $report = ConvertFrom-Json -InputObject $result.Text
                $expected = if ($size -eq 8192) { '35.1.2' } else { 'unknown' }
                Assert-Case "Windows metadata byte boundary $size" ($result.ExitCode -eq 0 -and $report.facts.emulatorVersion -eq $expected)
            }
            Copy-Item -LiteralPath $powershell -Destination (Join-Path $bin 'node.exe') -Force
            $nodeMetadata = [Diagnostics.FileVersionInfo]::GetVersionInfo($powershell)
            $expectedNode = @($nodeMetadata.FileMajorPart, $nodeMetadata.FileMinorPart, $nodeMetadata.FileBuildPart, $nodeMetadata.FilePrivatePart) -join '.'
            $result = Invoke-Inspection -Path $config -Validation $false
            $report = ConvertFrom-Json -InputObject $result.Text
            Assert-Case 'Windows Node version comes from numeric file metadata without execution' ($result.ExitCode -eq 0 -and $report.facts.nodeVersion -eq $expectedNode -and -not (Test-Path -LiteralPath $sentinel))
            Remove-Item -LiteralPath (Join-Path $sdk 'platform-tools\source.properties'), (Join-Path $bin 'node.exe')
            $result = Invoke-Inspection -Path $config -Validation $false
            $report = ConvertFrom-Json -InputObject $result.Text
            Assert-Case 'oversized metadata is unknown and absent metadata or Node is missing' ($result.ExitCode -eq 0 -and $report.facts.emulatorVersion -eq 'unknown' -and $report.facts.platformToolsVersion -eq 'missing' -and $report.facts.nodeBinary -eq 'missing' -and $report.facts.nodeVersion -eq 'missing')
        } finally { $env:PATH = $oldPath }

        $copy = Join-Path $scratch 'fault-host\hosts\windows-android-emulator'
        $lifetimeCopy = Join-Path $scratch 'fault-host\hosts\windows-docker-desktop'
        $null = New-Item -ItemType Directory -Path $copy, $lifetimeCopy -Force
        Copy-Item -LiteralPath $inspection -Destination $copy
        Copy-Item -LiteralPath (Join-Path $hostDirectory '../windows-docker-desktop/diagnostic-process-lifetime.ps1') -Destination $lifetimeCopy
        $faultWorker = @'
$ErrorActionPreference = 'Stop'
if ([Console]::In.ReadLine() -cne 'prerequisite-inventory-admitted') { exit 1 }
$configurationPath = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String([Console]::In.ReadLine()))
$directory = Split-Path -Parent $configurationPath
[IO.File]::WriteAllText((Join-Path $directory 'worker.pid'), [string]$PID)
$childSource = "[IO.File]::WriteAllText('" + (Join-Path $directory 'descendant.pid').Replace("'", "''") + "', [string]`$PID); Start-Sleep -Seconds 60"
$info = New-Object Diagnostics.ProcessStartInfo
$info.FileName = (Get-Process -Id $PID).Path
$info.Arguments = '-NoProfile -NonInteractive -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childSource))
$info.UseShellExecute = $false
$null = [Diagnostics.Process]::Start($info)
$clock = [Diagnostics.Stopwatch]::StartNew()
while (-not [IO.File]::Exists((Join-Path $directory 'descendant.pid')) -and $clock.ElapsedMilliseconds -lt 5000) { Start-Sleep -Milliseconds 10 }
switch ([IO.File]::ReadAllText($configurationPath)) {
    'overflow' { [Console]::Out.Write('synthetic-private-marker' + ('x' * 65537)); Start-Sleep -Seconds 60 }
    'invalid' { [Console]::Error.WriteLine('synthetic-private-marker'); [Console]::Out.WriteLine('synthetic-private-marker=private'); exit 0 }
    default { Start-Sleep -Seconds 60 }
}
'@
        [IO.File]::WriteAllText((Join-Path $copy 'prerequisite-inventory.ps1'), $faultWorker, $utf8)
        foreach ($fault in @('invalid', 'overflow', 'timeout')) {
            foreach ($file in @('worker.pid', 'descendant.pid')) { Remove-Item -LiteralPath (Join-Path $scratch $file) -ErrorAction SilentlyContinue }
            [IO.File]::WriteAllText($config, $fault, $utf8)
            $result = Invoke-Inspection -Path $config -Validation $false -Script (Join-Path $copy 'inspect-prerequisites.ps1')
            Assert-Case "Windows $fault fails closed with bounded private output" ($result.ExitCode -ne 0 -and $result.Text.Contains('inventory_unavailable') -and -not $result.Text.Contains($marker) -and $result.Text.Length -lt 65536 -and $result.Milliseconds -lt 32000)
            foreach ($file in @('worker.pid', 'descendant.pid')) {
                $pidFile = Join-Path $scratch $file
                $gone = $false
                if (Test-Path -LiteralPath $pidFile) {
                    $childId = [int][IO.File]::ReadAllText($pidFile)
                    $remaining = Get-Process -Id $childId -ErrorAction SilentlyContinue
                    $gone = $null -eq $remaining -or $remaining.HasExited
                    if ($null -ne $remaining) { $remaining.Dispose() }
                }
                Assert-Case "Windows $fault removes actual $file process" $gone
            }
        }
    }
} finally { Remove-Item -LiteralPath $scratch -Recurse -Force }
if ($failures.Count -gt 0) { Write-Output "$($failures.Count) prerequisite cases failed"; exit 1 }
Write-Output 'All prerequisite cases passed'
exit 0
