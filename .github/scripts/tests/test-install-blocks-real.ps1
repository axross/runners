#Requires -Version 5.1
<#
.SYNOPSIS
    runs the documented Windows install blocks against the dry run's real MSI.

.DESCRIPTION
    runs on the Windows leg of the Install Blocks job in merge-checks.yaml, under
    Windows PowerShell 5.1, on the release-build dry run's artifact. only gh is
    stubbed: it serves the artifact's real SHA256SUMS, a tag on main, and a passing
    attestation. the check, migration, and uninstall blocks come from
    docs/operations/cli.md and run with the real msiexec, the real Windows
    Installer COM object, and the real registry. the one manual install is a
    stand-in marshal.exe that differs from the installer's binary. outcomes the
    CLI document states are printed on lines that start with MEASURED. a failed
    check throws.
#>
param(
    [Parameter(Mandatory = $true)][string]$ReleaseDirectory,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$LogDirectory,
    [string]$Document = (Join-Path (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))) 'docs/operations/cli.md')
)

$ErrorActionPreference = 'Stop'
$script:Document = $Document
. (Join-Path $PSScriptRoot 'install-block-helpers.ps1')
Assert-JqAvailable

$CommitSha = '0123456789abcdef0123456789abcdef01234567'
$ReleaseDirectory = (Resolve-Path -LiteralPath $ReleaseDirectory).Path
$MsiName = "marshal-$Version-windows-x64.msi"
$BareName = "marshal-$Version-windows-x64.exe"
$GenuineMsi = Join-Path $ReleaseDirectory $MsiName
$GenuineBinary = Join-Path $ReleaseDirectory $BareName
$InstallDirectory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
$InstalledBinary = Join-Path $InstallDirectory 'marshal.exe'
$LogsDirectory = Join-Path $env:LOCALAPPDATA 'marshal\logs'
$MsiContextName = @{ 1 = 'user-managed'; 2 = 'user-unmanaged'; 4 = 'machine' }
$Work = Join-Path ([IO.Path]::GetTempPath()) ('marshal-install-blocks-real-' + [Guid]::NewGuid().ToString('N'))
$script:Calls = New-Object System.Collections.Generic.List[string]
$script:Scenario = @{
    Release = $ReleaseDirectory
    DownloadFail = $false
    TagFail = $false
    TagType = 'commit'
    TagSha = $CommitSha
    AnnotatedFail = $false
    AnnotatedSha = $CommitSha
    CompareFail = $false
    Compare = 'identical'
    AttestFail = $false
}

function Write-Measured {
    param([Parameter(Mandatory = $true)][string]$Text)
    Write-Output "MEASURED: $Text"
}

function Assert-That {
    param([Parameter(Mandatory = $true)][bool]$Condition, [Parameter(Mandatory = $true)][string]$Message)
    if (-not $Condition) { throw "FAILED: $Message" }
    Write-Output "ok   $Message"
}

function Invoke-ComMethod {
    param([Parameter(Mandatory = $true)]$Target, [Parameter(Mandatory = $true)][string]$Name, [string]$Kind = 'InvokeMethod', $Arguments = $null)
    return $Target.GetType().InvokeMember($Name, $Kind, $null, $Target, $Arguments)
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

function Get-UserPathKey {
    return [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
}

function Get-InstallDirectoryPathEntry {
    $key = Get-UserPathKey
    try {
        $value = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    } finally {
        $key.Close()
    }
    $wanted = $InstallDirectory.TrimEnd('\')
    return , @($value -split ';' | Where-Object { $_.Trim() -ne '' -and [Environment]::ExpandEnvironmentVariables($_.Trim()).TrimEnd('\') -ieq $wanted })
}

function Add-InstallDirectoryToPath {
    $key = Get-UserPathKey
    try {
        $value = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $key.SetValue('Path', ($value.TrimEnd(';') + ';%LOCALAPPDATA%\Programs\marshal'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
    } finally {
        $key.Close()
    }
}

function Assert-NothingInstalled {
    param([string]$Label)
    Assert-That (-not (Test-Path -LiteralPath $InstalledBinary)) "$Label - marshal.exe is not at the install path"
    Assert-That ((Get-InstalledProduct).Count -eq 0) "$Label - no product is listed as marshal"
    Assert-That ((Get-InstallDirectoryPathEntry).Count -eq 0) "$Label - the user PATH holds no install directory"
}

# runs msiexec with a verbose log, waits for it, and retries while another
# installation holds the Windows Installer service (1618).
function Invoke-Msi {
    param([Parameter(Mandatory = $true)][string[]]$Arguments, [Parameter(Mandatory = $true)][string]$LogName)
    $log = Join-Path $LogDirectory "$LogName.log"
    $argumentList = @($Arguments) + @('/qn', '/norestart', '/l*v', "`"$log`"")
    for ($attempt = 1; $attempt -le 6; $attempt++) {
        if (Test-Path -LiteralPath $log) { Remove-Item -LiteralPath $log -Force }
        $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList $argumentList -Wait -PassThru -NoNewWindow
        $null = $process.Handle
        if ($process.ExitCode -ne 1618) { break }
        Start-Sleep -Seconds 10
    }
    return $process.ExitCode
}

# runs a documented block with each named assignment line replaced, and returns
# what it wrote and the refusal it ended with, if any.
function Invoke-DocumentedBlock {
    param([string]$Id, [hashtable]$Replace = @{})
    $block = Get-InstallBlock -Document $script:Document -Id $Id
    foreach ($prefix in $Replace.Keys) {
        $block = Edit-BlockLine -Block $block -Prefix $prefix -Line $Replace[$prefix]
    }
    Assert-NoCommentLine -Id $Id -Block $block
    $scriptBlock = [scriptblock]::Create($block)
    $failure = $null
    $output = @()
    try {
        $output = @(& $scriptBlock *>&1 | ForEach-Object { "$_" })
    } catch {
        $failure = $_.Exception.Message
    }
    return [pscustomobject]@{ Output = ($output -join "`n"); Failure = $failure }
}

function Invoke-CheckBlock {
    param([string]$BlockVersion, [string]$Path)
    return Invoke-DocumentedBlock -Id 'windows-check' -Replace @{
        '$version =' = "`$version = '$BlockVersion'"
        '$file =' = "`$file = '$Path'"
    }
}

function Get-ZoneIdentifier {
    param([string]$Path)
    return ((Get-Content -LiteralPath $Path -Stream Zone.Identifier -ErrorAction SilentlyContinue) -join "`n")
}

New-Item -ItemType Directory -Force -Path $Work, $LogDirectory | Out-Null
Initialize-StandInMarshal -Directory $Work
$key = Get-UserPathKey
$originalPath = $key.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
$originalKind = [Microsoft.Win32.RegistryValueKind]::ExpandString
if ($null -ne $originalPath) { $originalKind = $key.GetValueKind('Path') }
$key.Close()
# set after the clean-start guard passes, so a refusal on a host that has marshal
# installed never uninstalls it or rewrites its user PATH.
$cleanupArmed = $false

try {
    foreach ($required in @($GenuineMsi, $GenuineBinary, (Join-Path $ReleaseDirectory 'SHA256SUMS'))) {
        Assert-That (Test-Path -LiteralPath $required -PathType Leaf) "the artifact holds $(Split-Path -Leaf $required)"
    }
    $msiHash = Get-Sha256 $GenuineMsi
    $binaryHash = Get-Sha256 $GenuineBinary
    $listed = @(Get-Content -LiteralPath (Join-Path $ReleaseDirectory 'SHA256SUMS') | Where-Object { ($_ -split '\s+')[1] -eq $MsiName })
    Assert-That ($listed.Count -eq 1 -and ($listed[0] -split '\s+')[0] -eq $msiHash) 'the artifact SHA256SUMS lists the MSI with its SHA-256'
    Assert-NothingInstalled -Label 'before the tests'
    $cleanupArmed = $true
    $result = Invoke-DocumentedBlock -Id 'windows-uninstall'
    Assert-That ($null -eq $result.Failure) "the uninstall block passes when nothing is installed ($($result.Failure))"
    Assert-That ($result.Output.Contains('nothing to uninstall')) "the uninstall block says there is nothing to uninstall: $($result.Output)"

    Write-Output '== the check block passes a genuine installer given as a renamed copy with a download mark =='
    $downloads = Join-Path $Work 'Downloads'
    New-Item -ItemType Directory -Force -Path $downloads | Out-Null
    $renamed = Join-Path $downloads "marshal $Version (1).msi"
    Copy-Item -LiteralPath $GenuineMsi -Destination $renamed
    Set-Content -LiteralPath $renamed -Stream Zone.Identifier -Value "[ZoneTransfer]`r`nZoneId=3`r`nHostUrl=https://github.com/axross/runners/releases"
    $markBefore = Get-ZoneIdentifier $renamed
    Assert-That ($markBefore -like '*ZoneId=3*') 'the renamed copy carries a Zone.Identifier mark'
    $stampBefore = (Get-Item -LiteralPath $renamed).LastWriteTimeUtc
    $result = Invoke-CheckBlock -BlockVersion $Version -Path $renamed
    Assert-That ($null -eq $result.Failure) "the check block passes ($($result.Failure))"
    Assert-That ($result.Output -like '*Passed:*') 'the check block prints its passed line'
    Assert-That ((Get-Sha256 $renamed) -eq $msiHash) 'the check block left the file unchanged'
    Assert-That ((Get-ZoneIdentifier $renamed) -eq $markBefore) 'the check block left the download mark unchanged'
    Assert-That ((Get-Item -LiteralPath $renamed).LastWriteTimeUtc -eq $stampBefore) 'the check block left the file time unchanged'
    Assert-NothingInstalled -Label 'after the check block'
    Assert-That (@($script:Calls | Where-Object { $_.StartsWith("gh attestation verify $renamed --repo axross/runners --signer-workflow axross/runners/.github/workflows/release.yaml --source-ref refs/heads/main --source-digest $CommitSha ") -and $_.EndsWith('--deny-self-hosted-runners') }).Count -eq 1) 'the attestation was verified on the downloaded file with the pins'

    Write-Output '== the check block refuses a genuine installer under another version name =='
    $served = Join-Path $Work 'served'
    New-Item -ItemType Directory -Force -Path $served | Out-Null
    Set-Content -LiteralPath (Join-Path $served 'SHA256SUMS') -Value "$msiHash  marshal-9.9.9-windows-x64.msi"
    $script:Scenario.Release = $served
    $result = Invoke-CheckBlock -BlockVersion '9.9.9' -Path $renamed
    $script:Scenario.Release = $ReleaseDirectory
    Assert-That ($null -ne $result.Failure -and $result.Failure.StartsWith('refused: ')) 'the check block refuses'
    Assert-That ($result.Failure.Contains("is not marshal 9.9.9: marshal $Version")) "the refusal is at the version step: $($result.Failure)"
    Assert-NothingInstalled -Label 'after the refused check'
    Assert-That ((Get-Sha256 $renamed) -eq $msiHash -and (Get-ZoneIdentifier $renamed) -eq $markBefore) 'the refused check left the file and its mark unchanged'

    Write-Output '== migration from a manual install, then install, then uninstall =='
    $manualVersion = '0.0.1-manual'
    Write-StandInMarshal -Path $InstalledBinary -Version $manualVersion
    Add-InstallDirectoryToPath
    Assert-That ((Get-Sha256 $InstalledBinary) -ne $binaryHash) 'the seeded manual copy differs from the installer binary'
    Assert-That ((Get-InstallDirectoryPathEntry).Count -eq 1) 'the manual install is on the user PATH'
    $result = Invoke-DocumentedBlock -Id 'windows-migrate'
    Assert-That ($null -eq $result.Failure) "the migration block passes ($($result.Failure))"
    Assert-That ($result.Output.Contains("Removed $InstalledBinary, the empty directory $InstallDirectory, the user PATH entry for $InstallDirectory.")) "the migration block names what it removed: $($result.Output)"
    Assert-NothingInstalled -Label 'after the migration block'
    Assert-That (-not (Test-Path -LiteralPath $InstallDirectory)) 'the migration block removed the empty install directory'

    $code = Invoke-Msi -Arguments @('/i', "`"$GenuineMsi`"") -LogName 'migrated-install'
    Assert-That ($code -eq 0) "the MSI installs after the migration (msiexec exit code $code)"
    Assert-That ((Get-Sha256 $InstalledBinary) -eq $binaryHash) 'the installed file is the installer binary'
    Assert-That ((& $InstalledBinary --version | Out-String).Trim() -eq "marshal $Version") "marshal --version at the install path prints 'marshal $Version'"
    Assert-That ((Get-InstallDirectoryPathEntry).Count -eq 1) 'the install put the directory on the user PATH once'

    New-Item -ItemType Directory -Force -Path $LogsDirectory | Out-Null
    $marker = Join-Path $LogsDirectory 'install-blocks-test.marker'
    Set-Content -LiteralPath $marker -Value 'kept by the uninstall'
    $result = Invoke-DocumentedBlock -Id 'windows-uninstall'
    Assert-That ($null -eq $result.Failure) "the uninstall block passes ($($result.Failure))"
    Assert-NothingInstalled -Label 'after the uninstall block'
    Assert-That (Test-Path -LiteralPath $marker) 'the uninstall block left the logs'
    Remove-Item -LiteralPath $marker -Force

    Write-Output '== the recovery after a skipped migration: the migration block, then uninstall and install again =='
    $code = Invoke-Msi -Arguments @('/i', "`"$GenuineMsi`"") -LogName 'recovery-first-install'
    Assert-That ($code -eq 0) "the MSI installs (msiexec exit code $code)"
    Assert-That ((Get-InstalledProduct).Count -eq 1) 'the product is listed after the install'
    $result = Invoke-DocumentedBlock -Id 'windows-migrate'
    Assert-That ($null -eq $result.Failure) "the migration block passes while the product is installed ($($result.Failure))"
    Assert-That ($result.Output.Contains('The marshal installer is installed, and what was removed was part of it too; uninstall it and install it again.')) "the migration block says that it removed part of the installed product: $($result.Output)"
    Assert-That (-not (Test-Path -LiteralPath $InstalledBinary)) 'the migration block removed the installed file'
    Assert-That ((Get-InstalledProduct).Count -eq 1) 'the product is still listed after the migration block'
    $result = Invoke-DocumentedBlock -Id 'windows-uninstall'
    Assert-That ($null -eq $result.Failure) "the uninstall block passes without marshal.exe ($($result.Failure))"
    Assert-NothingInstalled -Label 'after the uninstall that follows the migration block'
    $code = Invoke-Msi -Arguments @('/i', "`"$GenuineMsi`"") -LogName 'recovery-second-install'
    Assert-That ($code -eq 0) "the MSI installs again (msiexec exit code $code)"
    Assert-That ((Get-Sha256 $InstalledBinary) -eq $binaryHash) 'the installed file is the installer binary again'
    Assert-That ((Get-InstallDirectoryPathEntry).Count -eq 1) 'the install put the directory on the user PATH once'
    $result = Invoke-DocumentedBlock -Id 'windows-uninstall'
    Assert-That ($null -eq $result.Failure) "the uninstall block passes ($($result.Failure))"
    Assert-NothingInstalled -Label 'after the final uninstall'

    Write-Output '== installing over a manual copy without the migration block =='
    Write-StandInMarshal -Path $InstalledBinary -Version $manualVersion
    $manualHash = Get-Sha256 $InstalledBinary
    Add-InstallDirectoryToPath
    $code = Invoke-Msi -Arguments @('/i', "`"$GenuineMsi`"") -LogName 'over-manual-install'
    $atPath = 'absent'
    if (Test-Path -LiteralPath $InstalledBinary) {
        $hash = Get-Sha256 $InstalledBinary
        if ($hash -eq $binaryHash) { $atPath = "the installer's binary" }
        elseif ($hash -eq $manualHash) { $atPath = 'the manual copy' }
        else { $atPath = 'another file' }
    }
    $printed = if (Test-Path -LiteralPath $InstalledBinary) { (& $InstalledBinary --version 2>&1 | Out-String).Trim() } else { 'no file' }
    Write-Measured "installing the MSI over a manual copy at the install path, without the migration block: msiexec exit code $code; the file at the install path is $atPath; marshal --version there prints '$printed'; products listed: $((Get-InstalledProduct).Count); the install directory is on the user PATH $((Get-InstallDirectoryPathEntry).Count) time(s)"
    $result = Invoke-DocumentedBlock -Id 'windows-uninstall'
    Write-Measured "the uninstall block after that install: $(if ($null -eq $result.Failure) { 'passed' } else { $result.Failure }); the file at the install path afterwards: $(if (Test-Path -LiteralPath $InstalledBinary) { 'present' } else { 'absent' })"
    Write-Output 'All Windows install block checks passed.'
} finally {
    if ($cleanupArmed) {
        foreach ($product in (Get-InstalledProduct)) {
            $null = Invoke-Msi -Arguments @('/x', $product.ProductCode) -LogName "cleanup-$($product.ProductCode.Trim('{}'))"
        }
        if (Test-Path -LiteralPath $InstallDirectory) { Remove-Item -LiteralPath $InstallDirectory -Recurse -Force -ErrorAction SilentlyContinue }
        $key = Get-UserPathKey
        try {
            if ($null -eq $originalPath) { $key.DeleteValue('Path', $false) } else { $key.SetValue('Path', $originalPath, $originalKind) }
        } finally {
            $key.Close()
        }
    }
    Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue
}

# the CI shell wrapper exits with the last native exit code, which a stop that reported "no instance" leaves at 1.
exit 0
