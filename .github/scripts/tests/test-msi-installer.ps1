#Requires -Version 5.1
<#
.SYNOPSIS
    installs, upgrades, refuses a downgrade, and removes MSIs with the real
    msiexec, and reads the release MSI's tables.

.DESCRIPTION
    runs on the Windows build leg of release-build.yaml, after build-msi.ps1 has
    built the release MSI. it builds throwaway MSIs at fixed test versions from
    two copies of the release binary that differ in trailing bytes, so every
    install is judged by the installed file's SHA-256. those copies are only
    packed and hashed, never run. the one program that runs from the install path
    is a copy of PING.EXE packed as marshal.exe, for the upgrade-while-running
    case; the unmodified release binary alone prints its version. each outcome the
    CLI document states is printed on a line that starts with MEASURED. a failed
    check throws, so nothing is skipped silently. see docs/operations/cli.md.
#>
param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$ReleaseMsi,
    [Parameter(Mandatory = $true)][string]$ReleaseVersion,
    [Parameter(Mandatory = $true)][string]$InstallerVersion,
    [Parameter(Mandatory = $true)][string]$WixDirectory,
    [Parameter(Mandatory = $true)][string]$LogDirectory
)

$ErrorActionPreference = 'Stop'

# the upgrade code in marshal.wxs never changes; a change there fails here.
$UpgradeCode = '{512F67C6-97ED-4562-84F3-2584FA3BA29C}'
$DowngradeMessage = 'A newer version of marshal is already installed.'
$InstallDirectory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
$InstalledBinary = Join-Path $InstallDirectory 'marshal.exe'
$LogsDirectory = Join-Path $env:LOCALAPPDATA 'marshal\logs'
$LogsMarker = Join-Path $LogsDirectory 'installer-test.marker'
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
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$LogName)
    return Invoke-Msi -Arguments @('/i', "`"$Path`"") -LogName $LogName
}

function Uninstall-Msi {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$LogName)
    return Invoke-Msi -Arguments @('/x', "`"$Path`"") -LogName $LogName
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
# three contexts.
function Get-InstalledProduct {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $listed = Invoke-ComMethod -Target $installer -Name 'ProductsEx' -Kind 'GetProperty' -Arguments @('', '', 7)
    $products = @()
    foreach ($item in $listed) {
        try {
            $name = [string](Invoke-ComMethod -Target $item -Name 'InstallProperty' -Kind 'GetProperty' -Arguments @('ProductName'))
        } catch {
            # a product that cannot report a name is not marshal.
            continue
        }
        if ($name -ne 'marshal') { continue }
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
# marshal; a root that does not exist holds none.
function Get-UninstallEntryName {
    param([Parameter(Mandatory = $true)][string]$Root)
    if (-not (Test-Path -LiteralPath $Root)) { return , @() }
    $names = @(Get-ChildItem -LiteralPath $Root | Where-Object {
            $properties = Get-ItemProperty -LiteralPath $_.PSPath
            $properties.PSObject.Properties['DisplayName'] -and $properties.DisplayName -eq 'marshal'
        } | ForEach-Object { $_.PSChildName })
    return , $names
}

# one line with the count and the key names of the marshal entries under both roots.
function Format-UninstallEntryMeasure {
    $hkcu = Get-UninstallEntryName -Root $HkcuUninstallRoot
    $hklm = Get-UninstallEntryName -Root $HklmUninstallRoot
    return "Uninstall entries named marshal under HKCU: $($hkcu.Count)$(if ($hkcu.Count -gt 0) { ' (' + ($hkcu -join ', ') + ')' }); under HKLM: $($hklm.Count)$(if ($hklm.Count -gt 0) { ' (' + ($hklm -join ', ') + ')' })"
}

function Get-RelatedProductCode {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $codes = @()
    foreach ($code in (Invoke-ComMethod -Target $installer -Name 'RelatedProducts' -Kind 'GetProperty' -Arguments @($UpgradeCode))) { $codes += $code }
    return , $codes
}

function Get-UserPathEntry {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    try {
        $value = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    } finally {
        $key.Close()
    }
    return , @($value -split ';' | Where-Object { $_.Trim() -ne '' })
}

function Get-InstallDirectoryPathEntry {
    $wanted = $InstallDirectory.TrimEnd('\')
    return , @((Get-UserPathEntry) | Where-Object { $_.Trim().TrimEnd('\') -ieq $wanted })
}

function Assert-Installed {
    param([Parameter(Mandatory = $true)][string]$Hash, [Parameter(Mandatory = $true)][string]$Version, [Parameter(Mandatory = $true)][string]$Label)
    Assert-That (Test-Path -LiteralPath $InstalledBinary -PathType Leaf) "$Label - marshal.exe is at $InstalledBinary"
    Assert-That ((Get-Sha256 $InstalledBinary) -eq $Hash) "$Label - the installed file has the expected SHA-256"
    Assert-That ((Get-InstallDirectoryPathEntry).Count -eq 1) "$Label - the user PATH holds the install directory once"
    $products = Get-InstalledProduct
    Assert-That ($products.Count -eq 1) "$Label - one product is listed as marshal"
    Assert-That ($products[0].Version -eq $Version) "$Label - the listed version is $Version"
    Assert-That ((Get-RelatedProductCode).Count -eq 1) "$Label - one product holds the upgrade code"
}

function Assert-NothingInstalled {
    param([Parameter(Mandatory = $true)][string]$Label)
    Assert-That (-not (Test-Path -LiteralPath $InstalledBinary)) "$Label - marshal.exe is gone"
    Assert-That (-not (Test-Path -LiteralPath $InstallDirectory)) "$Label - the install directory is gone"
    Assert-That ((Get-InstallDirectoryPathEntry).Count -eq 0) "$Label - the user PATH holds no install directory"
    Assert-That ((Get-InstalledProduct).Count -eq 0) "$Label - no product is listed as marshal"
    Assert-That ((Get-RelatedProductCode).Count -eq 0) "$Label - no product holds the upgrade code"
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
    & $BuildScript -Binary $FixturePath -Version $Version -Output $output -WixDirectory $Wix | Out-Null
    return $output
}

function Assert-ExitCode {
    param([Parameter(Mandatory = $true)]$Result, [Parameter(Mandatory = $true)][string]$Label)
    Assert-That ($Result.ExitCode -eq 0) "$Label - msiexec exited 0 (log: $($Result.Log))"
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
New-Item -ItemType Directory -Force -Path $FixtureDirectory, $MsiDirectory | Out-Null
$waiting = $null
# set after the clean-start guard passes, so a refusal on a host that has marshal
# installed never uninstalls it.
$cleanupArmed = $false

try {
    $binaryHash = Get-Sha256 $Binary

    Write-Output '== a clean start =='
    Assert-NothingInstalled -Label 'before the tests'
    $cleanupArmed = $true
    New-Item -ItemType Directory -Force -Path $LogsDirectory | Out-Null
    Set-Content -LiteralPath $LogsMarker -Value 'kept by the uninstall'

    Write-Output '== the release MSI =='
    $releaseDatabase = Open-MsiDatabase -Path $ReleaseMsi
    Assert-That ((Get-MsiProperty $releaseDatabase 'ProductVersion') -eq $InstallerVersion) "the declared ProductVersion is $InstallerVersion"
    Assert-That ((Get-MsiProperty $releaseDatabase 'UpgradeCode') -eq $UpgradeCode) 'the upgrade code is the fixed one'
    Assert-That ((Get-MsiProperty $releaseDatabase 'ProductName') -eq 'marshal') 'the product name is marshal'
    Assert-That ((Get-MsiProperty $releaseDatabase 'Manufacturer') -eq 'axross') 'the manufacturer is axross'
    Write-Measured "the release MSI declares ProductVersion $InstallerVersion for release $ReleaseVersion"

    $result = Install-Msi -Path $ReleaseMsi -LogName 'release-install'
    Assert-ExitCode -Result $result -Label 'the release MSI installs'
    # measured before the assertions, so a run that fails them still shows where Windows registered the product.
    # Windows lists this user-unmanaged product under HKLM Uninstall (measured on windows-latest), so the
    # per-user install is proved by the Windows Installer context below, not by where the Uninstall entry lives.
    Write-Measured (Format-UninstallEntryMeasure)
    $registered = @(Get-InstalledProduct | ForEach-Object { "$($_.ProductCode) in context $($_.Context) ($($_.ContextName))" })
    Write-Measured "Windows Installer ProductsEx products named marshal: $($registered.Count)$(if ($registered.Count -gt 0) { ': ' + ($registered -join '; ') })"
    Assert-Installed -Hash $binaryHash -Version $InstallerVersion -Label 'the release MSI'
    $product = (Get-InstalledProduct)[0]
    Assert-That ($product.Publisher -eq 'axross') 'the listed publisher is axross'
    $printed = (& $InstalledBinary --version | Out-String).Trim()
    Assert-That ($LASTEXITCODE -eq 0) 'the installed binary exits 0 for --version'
    Assert-That ($printed -eq "marshal $ReleaseVersion") "the installed binary prints 'marshal $ReleaseVersion'"
    Write-Measured "the user PATH entry the MSI wrote: $((Get-InstallDirectoryPathEntry)[0])"
    Write-Measured "the Windows Installer listing: ProductName $($product.Name), Publisher $($product.Publisher), VersionString $($product.Version), product code $($product.ProductCode), context $($product.Context) ($($product.ContextName))"
    $elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    Write-Measured "this run is elevated: $elevated, so it shows nothing about a UAC prompt"
    Assert-That ($product.Context -eq 2) "the product is installed for the user, not the machine: its context is $($product.Context), $($product.ContextName)"

    $result = Uninstall-Msi -Path $ReleaseMsi -LogName 'release-uninstall'
    Assert-ExitCode -Result $result -Label 'the release MSI uninstalls'
    Assert-NothingInstalled -Label 'after the uninstall'
    Assert-That (Test-Path -LiteralPath $LogsMarker) 'the uninstall leaves %LOCALAPPDATA%\marshal\logs'
    Write-Measured "after the uninstall: $(Format-UninstallEntryMeasure)"

    Write-Output '== fixtures =='
    $fixtureA = Copy-Fixture -Name 'a' -Source $Binary -Tail "`nfixture-a"
    $fixtureB = Copy-Fixture -Name 'b' -Source $Binary -Tail "`nfixture-b"
    $hashA = Get-Sha256 $fixtureA
    $hashB = Get-Sha256 $fixtureB
    Assert-That ($hashA -ne $hashB) 'the two fixtures differ'
    Assert-That (($hashA -ne $binaryHash) -and ($hashB -ne $binaryHash)) 'both fixtures differ from the release binary'
    $b1 = Build-FixtureMsi -Name 'b-1.0.0' -FixturePath $fixtureB -Version '1.0.0' -Wix $WixDirectory
    $a2 = Build-FixtureMsi -Name 'a-2.0.0' -FixturePath $fixtureA -Version '2.0.0' -Wix $WixDirectory
    $b2 = Build-FixtureMsi -Name 'b-2.0.0' -FixturePath $fixtureB -Version '2.0.0' -Wix $WixDirectory
    $a1 = Build-FixtureMsi -Name 'a-1.0.0' -FixturePath $fixtureA -Version '1.0.0' -Wix $WixDirectory

    Write-Output '== install, upgrade, same-version rebuild, downgrade =='
    Assert-ExitCode -Result (Install-Msi -Path $b1 -LogName 'fixture-b-1.0.0-install') -Label 'version 1.0.0 installs'
    Assert-Installed -Hash $hashB -Version '1.0.0' -Label 'version 1.0.0'

    Assert-ExitCode -Result (Install-Msi -Path $a2 -LogName 'fixture-a-2.0.0-install') -Label 'a higher version installs'
    Assert-Installed -Hash $hashA -Version '2.0.0' -Label 'the higher version replaced the file'

    Assert-ExitCode -Result (Install-Msi -Path $b2 -LogName 'fixture-b-2.0.0-install') -Label 'a second build of the same version installs'
    Assert-Installed -Hash $hashB -Version '2.0.0' -Label 'the same-version rebuild replaced the file and left one product'

    $result = Install-Msi -Path $a1 -LogName 'fixture-a-1.0.0-downgrade'
    Assert-That ($result.ExitCode -ne 0) 'a lower version exits non-zero over a higher one'
    Assert-That ([bool](Select-String -LiteralPath $result.Log -SimpleMatch -Pattern $DowngradeMessage -Quiet)) "the log holds '$DowngradeMessage'"
    Assert-Installed -Hash $hashB -Version '2.0.0' -Label 'after the refused downgrade the higher version stays'
    Write-Measured "a lower version over a higher one: msiexec exit code $($result.ExitCode), the log holds the newer-version message, the higher version stays installed"

    Assert-ExitCode -Result (Uninstall-Msi -Path $b2 -LogName 'fixture-b-2.0.0-uninstall') -Label 'the installed version uninstalls'
    Assert-NothingInstalled -Label 'after the fixture uninstall'
    Assert-That (Test-Path -LiteralPath $LogsMarker) 'the fixture uninstall leaves %LOCALAPPDATA%\marshal\logs'

    Write-Output '== the extraction the optional check relies on =='
    $extract = Join-Path $work 'extract'
    $result = Invoke-Msi -Arguments @('/a', "`"$ReleaseMsi`"", "TARGETDIR=`"$extract`"") -LogName 'release-extract'
    Assert-ExitCode -Result $result -Label 'msiexec /a extracts the release MSI'
    $extracted = @(Get-ChildItem -LiteralPath $extract -Recurse -File)
    $listing = ($extracted | ForEach-Object { $_.FullName.Substring($extract.Length).TrimStart('\') }) -join ', '
    Write-Measured "the extraction holds: $listing"
    $imageCopy = Join-Path $extract (Split-Path -Leaf $ReleaseMsi)
    $besidesImageCopy = @($extracted | Where-Object { $_.FullName -ne $imageCopy })
    Assert-That ($besidesImageCopy.Count -eq 1 -and $besidesImageCopy[0].Name -eq 'marshal.exe') "the extraction holds marshal.exe and nothing else besides the administrative image's own .msi copy; it holds: $listing"
    $binaries = @($extracted | Where-Object { $_.Name -eq 'marshal.exe' })
    Assert-That ($binaries.Count -eq 1) 'the extraction holds one marshal.exe'
    Assert-That ((Get-Sha256 $binaries[0].FullName) -eq $binaryHash) 'the extracted marshal.exe has the release binary SHA-256'
    $relative = $binaries[0].FullName.Substring($extract.Length).TrimStart('\')
    Write-Measured "msiexec /a TARGETDIR=<dir> puts the binary at <dir>\$relative"
    Assert-That (-not (Test-Path -LiteralPath $InstallDirectory)) 'the extraction installed nothing'

    Write-Output '== tables and metadata of the release MSI =='
    $tables = Get-MsiTableName -Database $releaseDatabase
    Write-Measured "the release MSI tables: $($tables -join ', ')"
    if ($tables -contains 'CustomAction') {
        $actions = Get-MsiRowSet -Database $releaseDatabase -Query 'SELECT `Action`, `Type`, `Source`, `Target` FROM `CustomAction`'
        foreach ($action in $actions) {
            Write-Measured "CustomAction $($action -join ' | ')"
            # base type 19 shows an error message and stops; any other base type runs code.
            Assert-That (([int]$action[1] -band 0x3F) -eq 19) "custom action $($action[0]) is an error-message type"
        }
    } else {
        Write-Measured 'the release MSI has no CustomAction table'
    }
    if ($tables -contains 'Dialog') {
        $dialogs = Get-MsiRowSet -Database $releaseDatabase -Query 'SELECT `Dialog` FROM `Dialog`'
        Assert-That ($dialogs.Count -eq 0) 'the Dialog table is empty'
    } else {
        Write-Measured 'the release MSI has no Dialog table'
    }

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
    Assert-ExitCode -Result (Install-Msi -Path $waitingMsi -LogName 'waiting-install') -Label 'the waiting program installs as marshal.exe'
    Assert-Installed -Hash $waitingHash -Version '5.0.0' -Label 'the waiting program'
    $waiting = Start-Process -FilePath $InstalledBinary -ArgumentList @('-n', '600', '127.0.0.1') -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 3
    Assert-That (-not $waiting.HasExited) 'the program runs from the install path'
    $result = Install-Msi -Path $upgradeMsi -LogName 'waiting-upgrade'
    $pending = (Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager').PendingFileRenameOperations
    $pendingForMarshal = @($pending | Where-Object { $_ -and $_ -like '*marshal*' })
    $installedHash = if (Test-Path -LiteralPath $InstalledBinary) { Get-Sha256 $InstalledBinary } else { 'absent' }
    $installedIs = 'unknown'
    if ($installedHash -eq $waitingHash) { $installedIs = 'the running program' }
    elseif ($installedHash -eq $hashB) { $installedIs = 'the new version' }
    elseif ($installedHash -eq 'absent') { $installedIs = 'absent' }
    Write-Measured "upgrade while marshal.exe runs from the install path: msiexec exit code $($result.ExitCode)"
    Write-Measured "after that upgrade the file at the install path is $installedIs; the program is still running: $(-not $waiting.HasExited)"
    Write-Measured "after that upgrade the listed versions are: $(((Get-InstalledProduct) | ForEach-Object { $_.Version }) -join ', ')"
    Write-Measured "after that upgrade the files in the install directory are: $(((Get-ChildItem -LiteralPath $InstallDirectory -Force -ErrorAction SilentlyContinue) | ForEach-Object { $_.Name }) -join ', ')"
    Write-Measured "after that upgrade PendingFileRenameOperations names marshal in $($pendingForMarshal.Count) entries: $($pendingForMarshal -join ' ; ')"
    $restartLines = @(Select-String -LiteralPath $result.Log -Pattern 'FilesInUse|Restart Manager|RmSession|reboot|ReplacedInUseFiles' | Select-Object -First 8 | ForEach-Object { $_.Line.Trim() })
    Write-Measured "that upgrade's log lines about files in use: $($restartLines -join ' ## ')"
    if (-not $waiting.HasExited) { $waiting.Kill(); $waiting.WaitForExit() }
    $installedAfter = if (Test-Path -LiteralPath $InstalledBinary) { Get-Sha256 $InstalledBinary } else { 'absent' }
    Write-Measured "after the program ended the file at the install path is $(if ($installedAfter -eq $hashB) { 'the new version' } elseif ($installedAfter -eq $waitingHash) { 'the program' } else { $installedAfter })"
    Write-Output 'All MSI installer checks passed.'
} finally {
    if ($null -ne $waiting -and -not $waiting.HasExited) { $waiting.Kill() }
    if ($cleanupArmed) {
        Uninstall-InstalledProduct
        if (Test-Path -LiteralPath $LogsMarker) { Remove-Item -LiteralPath $LogsMarker -Force }
    }
    if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue }
}

# the CI shell wrapper exits with the last native exit code, which a stop that reported "no instance" leaves at 1.
exit 0
