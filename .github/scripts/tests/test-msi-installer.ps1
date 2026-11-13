#Requires -Version 5.1
<#
.SYNOPSIS
    installs, upgrades, refuses a downgrade, and removes Marshal's MSI with the
    real msiexec, and reads the release MSI's tables.

.DESCRIPTION
    runs on the build leg of release-build.yaml, after build-msi.ps1 has built the
    release MSI. the release MSI is installed without AUTOLAUNCHAPP, so the app it
    holds is never started. the other cases build throwaway MSIs at fixed test
    versions from a stand-in program, compiled here, that writes its arguments,
    its path and its user to the file its --record argument names and exits, so a
    launch is seen and judged by what the stand-in wrote. copies of it that differ
    in trailing bytes tell versions apart by the installed file's SHA-256. the one
    program that keeps running from the install path is a copy of PING.EXE packed
    as marshal-app.exe, for the upgrade-while-running case. each outcome the CLI
    document states is printed on a line that starts with MEASURED. a failed check
    throws, so nothing is skipped silently. see docs/operations/cli.md.
#>
param(
    [Parameter(Mandatory = $true)][string]$App,
    [Parameter(Mandatory = $true)][string]$Notices,
    [Parameter(Mandatory = $true)][string]$ReleaseMsi,
    [Parameter(Mandatory = $true)][string]$InstallerVersion,
    [Parameter(Mandatory = $true)][string]$WixDirectory,
    [Parameter(Mandatory = $true)][string]$LogDirectory
)

$ErrorActionPreference = 'Stop'

# the upgrade code in marshal.wxs never changes; a change there fails here. the
# command line's MSI has its own, which this one must never name.
$UpgradeCode = '{F53C5F78-BA20-4941-9091-988AFDFD656B}'
$CommandLineUpgradeCode = '{512F67C6-97ED-4562-84F3-2584FA3BA29C}'
$CommandLineComponent = '{EE33286A-2CA3-4A0B-A273-FC580EFD27CB}'
$ProductName = 'Marshal'
$DowngradeMessage = 'A newer version of Marshal is already installed.'
$InstallDirectory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
$InstalledApp = Join-Path $InstallDirectory 'marshal-app.exe'
$InstalledNotices = Join-Path $InstallDirectory 'THIRD-PARTY-NOTICES.txt'
$Shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Marshal.lnk'
$RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run'
# what an uninstall keeps: the logs, the configuration and the web view's data.
$KeptMarkers = @(
    (Join-Path $env:LOCALAPPDATA 'marshal\logs\installer-test.marker'),
    (Join-Path $env:LOCALAPPDATA 'marshal\installer-test.marker'),
    (Join-Path $env:LOCALAPPDATA 'app.axross.marshal\installer-test.marker')
)
$HkcuUninstallRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
$HklmUninstallRoot = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
$MsiContextName = @{ 1 = 'user-managed'; 2 = 'user-unmanaged'; 4 = 'machine' }
$BuildScript = Join-Path (Split-Path -Parent $PSScriptRoot) 'build-msi.ps1'

function Write-Measured {
    param([Parameter(Mandatory = $true)][string]$Text)
    Write-Output "MEASURED: $Text"
}

function Assert-That {
    param([Parameter(Mandatory = $true)][bool]$Condition, [Parameter(Mandatory = $true)][string]$Message)
    if (-not $Condition) { throw "FAILED: $Message" }
    Write-Output "ok   $Message"
}

function Get-Sha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

# runs msiexec with the arguments and a verbose log named for the step, and waits
# for it. 1618 means another installation is running, so it waits and tries again.
function Invoke-Msi {
    param([Parameter(Mandatory = $true)][string[]]$Arguments, [Parameter(Mandatory = $true)][string]$LogName)
    $log = Join-Path $LogDirectory "$LogName.log"
    $argumentList = @($Arguments) + @('/qn', '/norestart', '/l*v', "`"$log`"")
    for ($attempt = 1; $attempt -le 6; $attempt++) {
        if (Test-Path -LiteralPath $log) { Remove-Item -LiteralPath $log -Force }
        $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList $argumentList -Wait -PassThru -NoNewWindow
        $null = $process.Handle
        if ($process.ExitCode -ne 1618) { break }
        Write-Information "msiexec exited 1618 for $LogName; waiting for the other installation (attempt $attempt)." -InformationAction Continue
        Start-Sleep -Seconds 10
    }
    return [pscustomobject]@{ ExitCode = $process.ExitCode; Log = $log }
}

function Install-Msi {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$LogName, [string[]]$Property = @())
    return Invoke-Msi -Arguments (@('/i', "`"$Path`"") + $Property) -LogName $LogName
}

function Uninstall-Msi {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$LogName, [string[]]$Property = @())
    return Invoke-Msi -Arguments (@('/x', "`"$Path`"") + $Property) -LogName $LogName
}

function Invoke-ComMethod {
    param([Parameter(Mandatory = $true)]$Target, [Parameter(Mandatory = $true)][string]$Name, [string]$Kind = 'InvokeMethod', $Arguments = $null)
    return $Target.GetType().InvokeMember($Name, $Kind, $null, $Target, $Arguments)
}

function Open-MsiDatabase {
    param([Parameter(Mandatory = $true)][string]$Path)
    $installer = New-Object -ComObject WindowsInstaller.Installer
    return Invoke-ComMethod -Target $installer -Name 'OpenDatabase' -Arguments @($Path, 0)
}

# returns every row of the query as an array of its columns' text.
function Get-MsiRowSet {
    param([Parameter(Mandatory = $true)]$Database, [Parameter(Mandatory = $true)][string]$Query)
    $view = Invoke-ComMethod -Target $Database -Name 'OpenView' -Arguments @($Query)
    $null = Invoke-ComMethod -Target $view -Name 'Execute'
    $rows = New-Object System.Collections.Generic.List[object]
    while ($true) {
        $record = Invoke-ComMethod -Target $view -Name 'Fetch'
        if ($null -eq $record) { break }
        $count = Invoke-ComMethod -Target $record -Name 'FieldCount' -Kind 'GetProperty'
        $fields = New-Object System.Collections.Generic.List[string]
        for ($column = 1; $column -le $count; $column++) {
            $fields.Add([string](Invoke-ComMethod -Target $record -Name 'StringData' -Kind 'GetProperty' -Arguments @($column)))
        }
        $rows.Add($fields.ToArray())
    }
    $null = Invoke-ComMethod -Target $view -Name 'Close'
    return , $rows.ToArray()
}

function Get-MsiTableName {
    param([Parameter(Mandatory = $true)]$Database)
    $names = @()
    foreach ($row in (Get-MsiRowSet -Database $Database -Query 'SELECT `Name` FROM `_Tables`')) { $names += $row[0] }
    return , $names
}

function Get-MsiProperty {
    param([Parameter(Mandatory = $true)]$Database, [Parameter(Mandatory = $true)][string]$Name)
    $rows = Get-MsiRowSet -Database $Database -Query ('SELECT `Value` FROM `Property` WHERE `Property` = ''{0}''' -f $Name)
    if ($rows.Count -ne 1) { throw "The Property table holds $($rows.Count) rows for $Name, not 1." }
    return $rows[0][0]
}

# the Windows Installer keeps a product's registration in its own store, which its
# API reads in every install context; a per-user MSI product has no HKCU Uninstall
# key to look for. the context values are msiInstallContextUserManaged 1,
# UserUnmanaged 2 and Machine 4, and ProductsEx takes (product code, user SID,
# contexts); empty strings mean every product of the current user, and 7 means all
# three contexts. the names compare with case, because the command line's product
# is named marshal.
function Get-InstalledProduct {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $listed = Invoke-ComMethod -Target $installer -Name 'ProductsEx' -Kind 'GetProperty' -Arguments @('', '', 7)
    $products = @()
    foreach ($item in $listed) {
        try {
            $name = [string](Invoke-ComMethod -Target $item -Name 'InstallProperty' -Kind 'GetProperty' -Arguments @('ProductName'))
        } catch {
            # a product that cannot report a name is not Marshal.
            continue
        }
        if ($name -cne $ProductName) { continue }
        $context = [int](Invoke-ComMethod -Target $item -Name 'Context' -Kind 'GetProperty')
        $contextName = 'unknown'
        if ($MsiContextName.ContainsKey($context)) { $contextName = $MsiContextName[$context] }
        $products += [pscustomobject]@{
            ProductCode = [string](Invoke-ComMethod -Target $item -Name 'ProductCode' -Kind 'GetProperty')
            Name = $name
            Version = [string](Invoke-ComMethod -Target $item -Name 'InstallProperty' -Kind 'GetProperty' -Arguments @('VersionString'))
            Publisher = [string](Invoke-ComMethod -Target $item -Name 'InstallProperty' -Kind 'GetProperty' -Arguments @('Publisher'))
            Context = $context
            ContextName = $contextName
        }
    }
    return , $products
}

# returns the key names of the Uninstall entries under the root whose DisplayName is
# Marshal; a root that does not exist holds none.
function Get-UninstallEntryName {
    param([Parameter(Mandatory = $true)][string]$Root)
    if (-not (Test-Path -LiteralPath $Root)) { return , @() }
    $names = @(Get-ChildItem -LiteralPath $Root | Where-Object { $_.GetValue('DisplayName') -ceq $ProductName } | ForEach-Object { $_.PSChildName })
    return , $names
}

# one line with the count and the key names of the Marshal entries under both roots.
function Format-UninstallEntryMeasure {
    $hkcu = Get-UninstallEntryName -Root $HkcuUninstallRoot
    $hklm = Get-UninstallEntryName -Root $HklmUninstallRoot
    return "Uninstall entries named Marshal under HKCU: $($hkcu.Count)$(if ($hkcu.Count -gt 0) { ' (' + ($hkcu -join ', ') + ')' }); under HKLM: $($hklm.Count)$(if ($hklm.Count -gt 0) { ' (' + ($hklm -join ', ') + ')' })"
}

function Get-RelatedProductCode {
    param([string]$Code = $UpgradeCode)
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $codes = @()
    foreach ($found in (Invoke-ComMethod -Target $installer -Name 'RelatedProducts' -Kind 'GetProperty' -Arguments @($Code))) { $codes += $found }
    return , $codes
}

function Get-UserPath {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    try {
        return [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    } finally {
        $key.Close()
    }
}

# the data of the HKCU Run value, or $null when it is absent.
function Get-RunValue {
    param([Parameter(Mandatory = $true)][string]$Name)
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($RunKey)
    if ($null -eq $key) { return $null }
    try { return $key.GetValue($Name, $null) } finally { $key.Close() }
}

function Write-RunValue {
    param([Parameter(Mandatory = $true)][string]$Name, [Parameter(Mandatory = $true)][string]$Data)
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($RunKey)
    try { $key.SetValue($Name, $Data, [Microsoft.Win32.RegistryValueKind]::String) } finally { $key.Close() }
}

function Clear-RunValue {
    param([Parameter(Mandatory = $true)][string]$Name)
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($RunKey, $true)
    if ($null -eq $key) { return }
    try { $key.DeleteValue($Name, $false) } finally { $key.Close() }
}

function Get-ShortcutTarget {
    $shell = New-Object -ComObject WScript.Shell
    return $shell.CreateShortcut($Shortcut).TargetPath
}

function Assert-Installed {
    param([Parameter(Mandatory = $true)][string]$Hash, [Parameter(Mandatory = $true)][string]$Version, [Parameter(Mandatory = $true)][string]$Label)
    Assert-That (Test-Path -LiteralPath $InstalledApp -PathType Leaf) "$Label - marshal-app.exe is at $InstalledApp"
    Assert-That ((Get-Sha256 $InstalledApp) -eq $Hash) "$Label - the installed app has the expected SHA-256"
    Assert-That ((Get-Sha256 $InstalledNotices) -eq $NoticesHash) "$Label - THIRD-PARTY-NOTICES.txt is the app's notices for Windows"
    Assert-That (Test-Path -LiteralPath $Shortcut -PathType Leaf) "$Label - the Start menu entry Marshal is at $Shortcut"
    Assert-That ((Get-ShortcutTarget) -ieq $InstalledApp) "$Label - the Start menu entry starts $InstalledApp"
    Assert-That ((Get-UserPath) -ceq $PathBefore) "$Label - the user PATH is unchanged"
    $products = Get-InstalledProduct
    Assert-That ($products.Count -eq 1) "$Label - one product is listed as Marshal"
    Assert-That ($products[0].Version -eq $Version) "$Label - the listed version is $Version"
    Assert-That ((Get-RelatedProductCode).Count -eq 1) "$Label - one product holds the upgrade code"
}

function Assert-NothingInstalled {
    param([Parameter(Mandatory = $true)][string]$Label)
    Assert-That (-not (Test-Path -LiteralPath $InstalledApp)) "$Label - marshal-app.exe is gone"
    Assert-That (-not (Test-Path -LiteralPath $InstalledNotices)) "$Label - THIRD-PARTY-NOTICES.txt is gone"
    Assert-That (-not (Test-Path -LiteralPath $InstallDirectory)) "$Label - the install directory is gone"
    Assert-That (-not (Test-Path -LiteralPath $Shortcut)) "$Label - the Start menu entry is gone"
    Assert-That ((Get-UserPath) -ceq $PathBefore) "$Label - the user PATH is unchanged"
    Assert-That ((Get-InstalledProduct).Count -eq 0) "$Label - no product is listed as Marshal"
    Assert-That ((Get-RelatedProductCode).Count -eq 0) "$Label - no product holds the upgrade code"
}

function Assert-KeptMarker {
    param([Parameter(Mandatory = $true)][string]$Label)
    foreach ($marker in $KeptMarkers) { Assert-That (Test-Path -LiteralPath $marker) "$Label - the uninstall left $marker" }
}

# compiles the stand-in that records its launch, with the .NET Framework's csc.
function Build-LaunchRecorder {
    param([Parameter(Mandatory = $true)][string]$Path)
    $source = @'
using System;
using System.IO;
using System.Reflection;
using System.Security.Principal;
public static class LaunchRecorder {
    public static int Main(string[] args) {
        int at = Array.IndexOf(args, "--record");
        if (at < 0 || at + 1 >= args.Length) { return 2; }
        WindowsIdentity identity = WindowsIdentity.GetCurrent();
        bool elevated = new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator);
        string text = "path=" + Assembly.GetExecutingAssembly().Location + "\n"
            + "user=" + identity.Name + "\n"
            + "elevated=" + elevated + "\n"
            + "args=" + string.Join("|", args) + "\n";
        File.WriteAllText(args[at + 1], text);
        return 0;
    }
}
'@
    $sourcePath = "$Path.cs"
    Set-Content -LiteralPath $sourcePath -Value $source -Encoding UTF8
    $compiler = Join-Path $env:windir 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    if (-not (Test-Path -LiteralPath $compiler)) { throw "$compiler does not exist." }
    & $compiler /nologo /target:winexe "/out:$Path" $sourcePath | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $Path)) { throw 'csc could not build the launch recorder.' }
}

function Copy-Fixture {
    param([Parameter(Mandatory = $true)][string]$Name, [Parameter(Mandatory = $true)][string]$Source, [string]$Tail = '')
    $path = Join-Path $FixtureDirectory "$Name.exe"
    Copy-Item -LiteralPath $Source -Destination $path
    if ($Tail -ne '') {
        $bytes = [Text.Encoding]::ASCII.GetBytes($Tail)
        $stream = [IO.File]::Open($path, [IO.FileMode]::Append)
        try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
    }
    return $path
}

function Build-FixtureMsi {
    param([Parameter(Mandatory = $true)][string]$Name, [Parameter(Mandatory = $true)][string]$FixturePath, [Parameter(Mandatory = $true)][string]$Version, [Parameter(Mandatory = $true)][string]$Wix)
    $output = Join-Path $MsiDirectory "$Name.msi"
    & $BuildScript -App $FixturePath -Notices $Notices -Version $Version -Output $output -WixDirectory $Wix | Out-Null
    return $output
}

function Assert-ExitCode {
    param([Parameter(Mandatory = $true)]$Result, [Parameter(Mandatory = $true)][string]$Label)
    Assert-That ($Result.ExitCode -eq 0) "$Label - msiexec exited 0 (log: $($Result.Log))"
}

# the properties a launch is asked for with, in the form W4's update runs msiexec
# with, and the file the recorder writes.
function Get-LaunchProperty {
    param([Parameter(Mandatory = $true)][string]$Record, [switch]$WithoutAutoLaunch)
    if ($Record.Contains(' ')) { throw "The record path $Record holds a space, which LAUNCHAPPARGS would split." }
    $arguments = "LAUNCHAPPARGS=`"--record $Record --after-update`""
    if ($WithoutAutoLaunch) { return @($arguments) }
    return @('AUTOLAUNCHAPP=True', $arguments)
}

# waits up to the timeout for the recorder's file and returns what it wrote, or $null.
function Wait-Launch {
    param([Parameter(Mandatory = $true)][string]$Record, [int]$Seconds = 30)
    for ($waited = 0; $waited -lt $Seconds; $waited++) {
        if (Test-Path -LiteralPath $Record) {
            Start-Sleep -Milliseconds 500
            $fields = @{}
            foreach ($line in (Get-Content -LiteralPath $Record)) {
                $split = $line.IndexOf('=')
                if ($split -gt 0) { $fields[$line.Substring(0, $split)] = $line.Substring($split + 1) }
            }
            return $fields
        }
        Start-Sleep -Seconds 1
    }
    return $null
}

function Assert-Launched {
    param([Parameter(Mandatory = $true)][string]$Record, [Parameter(Mandatory = $true)][string]$Label)
    $launch = Wait-Launch -Record $Record
    Assert-That ($null -ne $launch) "$Label - the installed app was started within 30 s"
    Assert-That ($launch['path'] -ieq $InstalledApp) "$Label - the program started is $InstalledApp, not $($launch['path'])"
    Assert-That ($launch['args'] -ceq "--record|$Record|--after-update") "$Label - it was started with LAUNCHAPPARGS as its arguments, not '$($launch['args'])'"
    Assert-That ($launch['user'] -ieq $CurrentUser) "$Label - it runs as the installing user, $CurrentUser, not $($launch['user'])"
    Write-Measured "$Label - the started app runs as $($launch['user']), elevated: $($launch['elevated']); this run itself is elevated: $Elevated"
    Remove-Item -LiteralPath $Record -Force
}

function Assert-NotLaunched {
    param([Parameter(Mandatory = $true)][string]$Record, [Parameter(Mandatory = $true)][string]$Label)
    Assert-That ($null -eq (Wait-Launch -Record $Record -Seconds 15)) "$Label - nothing was started within 15 s"
}

function Uninstall-InstalledProduct {
    foreach ($product in (Get-InstalledProduct)) {
        $result = Invoke-Msi -Arguments @('/x', $product.ProductCode) -LogName "cleanup-$($product.ProductCode.Trim('{}'))"
        if ($result.ExitCode -ne 0) { Write-Warning "Removing $($product.ProductCode) exited $($result.ExitCode)." }
    }
}

New-Item -ItemType Directory -Force -Path $LogDirectory | Out-Null
$work = Join-Path ([IO.Path]::GetTempPath()) ("marshal-msi-test-" + [Guid]::NewGuid().ToString('N'))
$FixtureDirectory = Join-Path $work 'fixtures'
$MsiDirectory = Join-Path $work 'msi'
$RecordDirectory = Join-Path $work 'records'
New-Item -ItemType Directory -Force -Path $FixtureDirectory, $MsiDirectory, $RecordDirectory | Out-Null
$waiting = $null
$CurrentUser = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$Elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$OtherRunValue = 'MarshalInstallerTestOther'
# set after the clean-start guard passes, so a refusal on a host that has Marshal
# installed never uninstalls it or touches its Run value.
$cleanupArmed = $false

try {
    $appHash = Get-Sha256 $App
    $NoticesHash = Get-Sha256 $Notices
    $PathBefore = Get-UserPath

    Write-Output '== a clean start =='
    Assert-NothingInstalled -Label 'before the tests'
    Assert-That ($null -eq (Get-RunValue -Name 'Marshal')) 'before the tests - the HKCU Run value Marshal is absent'
    $cleanupArmed = $true
    foreach ($marker in $KeptMarkers) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $marker) | Out-Null
        Set-Content -LiteralPath $marker -Value 'kept by the uninstall'
    }

    Write-Output '== the release MSI =='
    $releaseDatabase = Open-MsiDatabase -Path $ReleaseMsi
    Assert-That ((Get-MsiProperty $releaseDatabase 'ProductVersion') -eq $InstallerVersion) "the declared ProductVersion is $InstallerVersion"
    Assert-That ((Get-MsiProperty $releaseDatabase 'UpgradeCode') -eq $UpgradeCode) 'the upgrade code is the fixed one'
    Assert-That ((Get-MsiProperty $releaseDatabase 'ProductName') -ceq $ProductName) 'the product name is Marshal'
    Assert-That ((Get-MsiProperty $releaseDatabase 'Manufacturer') -ceq 'axross') 'the manufacturer is axross'
    $upgrades = Get-MsiRowSet -Database $releaseDatabase -Query 'SELECT `UpgradeCode` FROM `Upgrade`'
    Assert-That ($upgrades.Count -gt 0 -and @($upgrades | Where-Object { $_[0] -ne $UpgradeCode }).Count -eq 0) "the Upgrade table names only Marshal's own upgrade code, so it never upgrades or removes the command line's MSI"
    $components = Get-MsiRowSet -Database $releaseDatabase -Query 'SELECT `ComponentId` FROM `Component`'
    Assert-That (@($components | Where-Object { $_[0] -eq $CommandLineComponent -or $_[0] -eq $CommandLineUpgradeCode }).Count -eq 0) "no component shares the command line's component code"
    Write-Measured "the release MSI declares ProductVersion $InstallerVersion"

    $releaseRecord = Join-Path $RecordDirectory 'release.txt'
    $result = Install-Msi -Path $ReleaseMsi -LogName 'release-install'
    Assert-ExitCode -Result $result -Label 'the release MSI installs'
    # measured before the assertions, so a run that fails them still shows where Windows registered the product.
    Write-Measured (Format-UninstallEntryMeasure)
    $registered = @(Get-InstalledProduct | ForEach-Object { "$($_.ProductCode) in context $($_.Context) ($($_.ContextName))" })
    Write-Measured "Windows Installer ProductsEx products named Marshal: $($registered.Count)$(if ($registered.Count -gt 0) { ': ' + ($registered -join '; ') })"
    Assert-Installed -Hash $appHash -Version $InstallerVersion -Label 'the release MSI'
    $product = (Get-InstalledProduct)[0]
    Assert-That ($product.Publisher -ceq 'axross') 'the listed publisher is axross'
    Write-Measured "the Windows Installer listing: ProductName $($product.Name), Publisher $($product.Publisher), VersionString $($product.Version), product code $($product.ProductCode), context $($product.Context) ($($product.ContextName))"
    Write-Measured "this run is elevated: $Elevated, so it shows nothing about a UAC prompt"
    Assert-That ($product.Context -eq 2) "the product is installed for the user, not the machine: its context is $($product.Context), $($product.ContextName)"
    Start-Sleep -Seconds 5
    $started = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -ieq $InstalledApp })
    Assert-That ($started.Count -eq 0) 'an install without AUTOLAUNCHAPP starts no marshal-app.exe'

    $result = Uninstall-Msi -Path $ReleaseMsi -LogName 'release-uninstall'
    Assert-ExitCode -Result $result -Label 'the release MSI uninstalls without a Run value'
    Assert-NothingInstalled -Label 'after the uninstall'
    Assert-KeptMarker -Label 'the release MSI'
    Write-Measured "after the uninstall: $(Format-UninstallEntryMeasure)"
    Assert-That (-not (Test-Path -LiteralPath $releaseRecord)) 'nothing recorded a launch'

    Write-Output '== fixtures =='
    $recorder = Join-Path $FixtureDirectory 'recorder.exe'
    Build-LaunchRecorder -Path $recorder
    $fixtureA = Copy-Fixture -Name 'a' -Source $recorder -Tail "`nfixture-a"
    $fixtureB = Copy-Fixture -Name 'b' -Source $recorder -Tail "`nfixture-b"
    $hashA = Get-Sha256 $fixtureA
    $hashB = Get-Sha256 $fixtureB
    Assert-That ($hashA -ne $hashB) 'the two fixtures differ'
    $b1 = Build-FixtureMsi -Name 'b-1.0.0' -FixturePath $fixtureB -Version '1.0.0' -Wix $WixDirectory
    $a2 = Build-FixtureMsi -Name 'a-2.0.0' -FixturePath $fixtureA -Version '2.0.0' -Wix $WixDirectory
    $b2 = Build-FixtureMsi -Name 'b-2.0.0' -FixturePath $fixtureB -Version '2.0.0' -Wix $WixDirectory
    $a1 = Build-FixtureMsi -Name 'a-1.0.0' -FixturePath $fixtureA -Version '1.0.0' -Wix $WixDirectory

    Write-Output '== starting the app after an install or upgrade =='
    $record = Join-Path $RecordDirectory 'launch.txt'
    Assert-ExitCode -Result (Install-Msi -Path $b1 -LogName 'fixture-b-1.0.0-install-arguments-only' -Property (Get-LaunchProperty -Record $record -WithoutAutoLaunch)) -Label 'version 1.0.0 installs with LAUNCHAPPARGS but no AUTOLAUNCHAPP'
    Assert-Installed -Hash $hashB -Version '1.0.0' -Label 'version 1.0.0'
    Assert-NotLaunched -Record $record -Label 'an install without AUTOLAUNCHAPP'

    Write-RunValue -Name 'Marshal' -Data "`"$InstalledApp`" --signed-in"
    Write-RunValue -Name $OtherRunValue -Data 'kept'
    Assert-ExitCode -Result (Install-Msi -Path $a2 -LogName 'fixture-a-2.0.0-upgrade-autolaunch' -Property (Get-LaunchProperty -Record $record)) -Label 'a higher version installs with AUTOLAUNCHAPP=True'
    Assert-Installed -Hash $hashA -Version '2.0.0' -Label 'the higher version replaced the file'
    Assert-Launched -Record $record -Label 'an upgrade with AUTOLAUNCHAPP=True'
    Assert-That ($null -ne (Get-RunValue -Name 'Marshal')) 'an upgrade keeps the HKCU Run value Marshal'

    Assert-ExitCode -Result (Install-Msi -Path $b2 -LogName 'fixture-b-2.0.0-same-version-autolaunch' -Property (Get-LaunchProperty -Record $record)) -Label 'a second build of the same version installs with AUTOLAUNCHAPP=True'
    Assert-Installed -Hash $hashB -Version '2.0.0' -Label 'the same-version rebuild replaced the file and left one product'
    Assert-Launched -Record $record -Label 'a same-version rebuild with AUTOLAUNCHAPP=True'

    $result = Install-Msi -Path $a1 -LogName 'fixture-a-1.0.0-downgrade' -Property (Get-LaunchProperty -Record $record)
    Assert-That ($result.ExitCode -ne 0) 'a lower version exits non-zero over a higher one'
    Assert-That ([bool](Select-String -LiteralPath $result.Log -SimpleMatch -Pattern $DowngradeMessage -Quiet)) "the log holds '$DowngradeMessage'"
    Assert-Installed -Hash $hashB -Version '2.0.0' -Label 'after the refused downgrade the higher version stays'
    Assert-NotLaunched -Record $record -Label 'a refused downgrade'
    Write-Measured "a lower version over a higher one: msiexec exit code $($result.ExitCode), the log holds the newer-version message, the higher version stays installed"

    Write-Output '== an uninstall removes the Run value, and never starts the app =='
    Assert-ExitCode -Result (Uninstall-Msi -Path $b2 -LogName 'fixture-b-2.0.0-uninstall-run-value' -Property (Get-LaunchProperty -Record $record)) -Label 'the installed version uninstalls with the Run value present'
    Assert-NothingInstalled -Label 'after the fixture uninstall'
    Assert-That ($null -eq (Get-RunValue -Name 'Marshal')) 'the uninstall removed the HKCU Run value Marshal'
    Assert-That ((Get-RunValue -Name $OtherRunValue) -ceq 'kept') 'the uninstall left the other Run values alone'
    Assert-NotLaunched -Record $record -Label 'an uninstall with AUTOLAUNCHAPP=True'
    Assert-KeptMarker -Label 'the fixture MSI'

    Assert-ExitCode -Result (Install-Msi -Path $a1 -LogName 'fixture-a-1.0.0-install') -Label 'version 1.0.0 installs again'
    Assert-ExitCode -Result (Uninstall-Msi -Path $a1 -LogName 'fixture-a-1.0.0-uninstall-no-run-value') -Label 'the installed version uninstalls with the Run value absent'
    Assert-NothingInstalled -Label 'after the uninstall without a Run value'
    Assert-That ($null -eq (Get-RunValue -Name 'Marshal')) 'the HKCU Run value Marshal is still absent'

    Write-Output '== the extraction the optional check relies on =='
    $extract = Join-Path $work 'extract'
    $result = Invoke-Msi -Arguments @('/a', "`"$ReleaseMsi`"", "TARGETDIR=`"$extract`"") -LogName 'release-extract'
    Assert-ExitCode -Result $result -Label 'msiexec /a extracts the release MSI'
    $extracted = @(Get-ChildItem -LiteralPath $extract -Recurse -File)
    $listing = ($extracted | ForEach-Object { $_.FullName.Substring($extract.Length).TrimStart('\') }) -join ', '
    Write-Measured "the extraction holds: $listing"
    $imageCopy = Join-Path $extract (Split-Path -Leaf $ReleaseMsi)
    $besidesImageCopy = @($extracted | Where-Object { $_.FullName -ne $imageCopy } | ForEach-Object { $_.Name } | Sort-Object)
    Assert-That (($besidesImageCopy -join ',') -eq 'marshal-app.exe,THIRD-PARTY-NOTICES.txt') "the extraction holds marshal-app.exe and THIRD-PARTY-NOTICES.txt and nothing else besides the administrative image's own .msi copy; it holds: $listing"
    $apps = @($extracted | Where-Object { $_.Name -eq 'marshal-app.exe' })
    Assert-That ((Get-Sha256 $apps[0].FullName) -eq $appHash) "the extracted marshal-app.exe has the release executable's SHA-256"
    $relative = $apps[0].FullName.Substring($extract.Length).TrimStart('\')
    Write-Measured "msiexec /a TARGETDIR=<dir> puts the app at <dir>\$relative"
    Assert-That (-not (Test-Path -LiteralPath $InstallDirectory)) 'the extraction installed nothing'

    Write-Output '== tables and metadata of the release MSI =='
    $tables = Get-MsiTableName -Database $releaseDatabase
    Write-Measured "the release MSI tables: $($tables -join ', ')"
    $actions = Get-MsiRowSet -Database $releaseDatabase -Query 'SELECT `Action`, `Type`, `Source`, `Target` FROM `CustomAction`'
    $launch = $null
    $removal = $null
    foreach ($action in $actions) {
        Write-Measured "CustomAction $($action -join ' | ')"
        # base type 19 shows an error message and stops, 18 runs an installed file and
        # 34 runs a program in a directory.
        $base = [int]$action[1] -band 0x3F
        if ($action[0] -eq 'LaunchMarshal') { $launch = $action }
        elseif ($action[0] -eq 'RemoveRunValue') { $removal = $action }
        else { Assert-That ($base -eq 19) "custom action $($action[0]) is an error-message type" }
    }
    Assert-That ($null -ne $launch -and ([int]$launch[1] -band 0x3F) -eq 18 -and $launch[2] -eq 'MarshalAppExe' -and $launch[3] -eq '[LAUNCHAPPARGS]') 'LaunchMarshal runs the installed marshal-app.exe with [LAUNCHAPPARGS] and nothing else'
    Assert-That ($null -ne $removal -and ([int]$removal[1] -band 0x3F) -eq 34 -and $removal[3] -eq '"[System64Folder]reg.exe" delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v Marshal /f') 'RemoveRunValue runs only reg.exe delete of the HKCU Run value Marshal'
    $conditions = Get-MsiRowSet -Database $releaseDatabase -Query 'SELECT `Action`, `Condition` FROM `InstallExecuteSequence`'
    foreach ($row in $conditions) {
        if ($row[0] -eq 'LaunchMarshal' -or $row[0] -eq 'RemoveRunValue') { Write-Measured "InstallExecuteSequence $($row[0]) runs when: $($row[1])" }
    }
    if ($tables -contains 'Dialog') {
        $dialogs = Get-MsiRowSet -Database $releaseDatabase -Query 'SELECT `Dialog` FROM `Dialog`'
        Assert-That ($dialogs.Count -eq 0) 'the Dialog table is empty'
    } else {
        Write-Measured 'the release MSI has no Dialog table'
    }
    $environment = if ($tables -contains 'Environment') { @(Get-MsiRowSet -Database $releaseDatabase -Query 'SELECT `Name` FROM `Environment`') } else { @() }
    Assert-That ($environment.Count -eq 0) 'the MSI writes no environment variable, so PATH stays as it was'

    $summary = Invoke-ComMethod -Target $releaseDatabase -Name 'SummaryInformation' -Kind 'GetProperty' -Arguments @(0)
    $strings = New-Object System.Collections.Generic.List[string]
    foreach ($id in 2, 3, 4, 5, 6, 8, 18) {
        $value = Invoke-ComMethod -Target $summary -Name 'Property' -Kind 'GetProperty' -Arguments @($id)
        Write-Measured "summary information $id = $value"
        if ($null -ne $value) { $strings.Add([string]$value) }
    }
    foreach ($table in $tables) {
        if ($table -eq 'Binary' -or $table -eq 'Icon') { continue }
        foreach ($row in (Get-MsiRowSet -Database $releaseDatabase -Query ('SELECT * FROM `{0}`' -f $table))) {
            foreach ($field in $row) { $strings.Add($field) }
        }
    }
    foreach ($property in (Get-MsiRowSet -Database $releaseDatabase -Query 'SELECT `Property`, `Value` FROM `Property`')) {
        Write-Measured "Property $($property[0]) = $($property[1])"
    }
    $identifying = @($env:COMPUTERNAME, $env:USERNAME, $env:USERPROFILE, $env:RUNNER_NAME, $env:GITHUB_WORKSPACE, $env:RUNNER_TEMP) |
        Where-Object { $_ -and $_.Length -ge 4 }
    foreach ($value in $identifying) {
        $hits = @($strings | Where-Object { $_.IndexOf($value, [StringComparison]::OrdinalIgnoreCase) -ge 0 })
        Assert-That ($hits.Count -eq 0) 'no MSI table or summary field holds a machine, account, or workspace name'
    }
    Assert-That ($strings.Count -gt 0) 'the metadata check read the MSI tables'
    # a positive control: the scan reads the strings that name the project, so a scan that read nothing useful cannot pass.
    Assert-That (@($strings | Where-Object { $_.IndexOf('axross', [StringComparison]::OrdinalIgnoreCase) -ge 0 }).Count -gt 0) "the strings the metadata check read hold 'axross'; it read $($strings.Count) strings"

    Write-Output '== an upgrade while a program runs from the install path =='
    $ping = Join-Path $env:SystemRoot 'System32\PING.EXE'
    $waitingFixture = Copy-Fixture -Name 'waiting' -Source $ping
    $waitingMsi = Build-FixtureMsi -Name 'waiting-5.0.0' -FixturePath $waitingFixture -Version '5.0.0' -Wix $WixDirectory
    $upgradeMsi = Build-FixtureMsi -Name 'b-5.0.1' -FixturePath $fixtureB -Version '5.0.1' -Wix $WixDirectory
    $waitingHash = Get-Sha256 $waitingFixture
    Assert-ExitCode -Result (Install-Msi -Path $waitingMsi -LogName 'waiting-install') -Label 'the waiting program installs as marshal-app.exe'
    Assert-Installed -Hash $waitingHash -Version '5.0.0' -Label 'the waiting program'
    $waiting = Start-Process -FilePath $InstalledApp -ArgumentList @('-n', '600', '127.0.0.1') -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 3
    Assert-That (-not $waiting.HasExited) 'the program runs from the install path'
    $result = Install-Msi -Path $upgradeMsi -LogName 'waiting-upgrade'
    $pending = (Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager').PendingFileRenameOperations
    $pendingForMarshal = @($pending | Where-Object { $_ -and $_ -like '*marshal*' })
    $installedHash = if (Test-Path -LiteralPath $InstalledApp) { Get-Sha256 $InstalledApp } else { 'absent' }
    $installedIs = 'unknown'
    if ($installedHash -eq $waitingHash) { $installedIs = 'the running program' }
    elseif ($installedHash -eq $hashB) { $installedIs = 'the new version' }
    elseif ($installedHash -eq 'absent') { $installedIs = 'absent' }
    Write-Measured "upgrade while marshal-app.exe runs from the install path: msiexec exit code $($result.ExitCode)"
    Write-Measured "after that upgrade the file at the install path is $installedIs; the program is still running: $(-not $waiting.HasExited)"
    Write-Measured "after that upgrade the listed versions are: $(((Get-InstalledProduct) | ForEach-Object { $_.Version }) -join ', ')"
    Write-Measured "after that upgrade the files in the install directory are: $(((Get-ChildItem -LiteralPath $InstallDirectory -Force -ErrorAction SilentlyContinue) | ForEach-Object { $_.Name }) -join ', ')"
    Write-Measured "after that upgrade PendingFileRenameOperations names marshal in $($pendingForMarshal.Count) entries: $($pendingForMarshal -join ' ; ')"
    $restartLines = @(Select-String -LiteralPath $result.Log -Pattern 'FilesInUse|Restart Manager|RmSession|reboot|ReplacedInUseFiles' | Select-Object -First 8 | ForEach-Object { $_.Line.Trim() })
    Write-Measured "that upgrade's log lines about files in use: $($restartLines -join ' ## ')"
    if (-not $waiting.HasExited) { $waiting.Kill(); $waiting.WaitForExit() }
    $installedAfter = if (Test-Path -LiteralPath $InstalledApp) { Get-Sha256 $InstalledApp } else { 'absent' }
    Write-Measured "after the program ended the file at the install path is $(if ($installedAfter -eq $hashB) { 'the new version' } elseif ($installedAfter -eq $waitingHash) { 'the program' } else { $installedAfter })"
    Write-Output 'All MSI installer checks passed.'
} finally {
    if ($null -ne $waiting -and -not $waiting.HasExited) { $waiting.Kill() }
    if ($cleanupArmed) {
        Uninstall-InstalledProduct
        Clear-RunValue -Name 'Marshal'
        Clear-RunValue -Name $OtherRunValue
        foreach ($marker in $KeptMarkers) { if (Test-Path -LiteralPath $marker) { Remove-Item -LiteralPath $marker -Force } }
    }
    if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue }
}

exit 0
