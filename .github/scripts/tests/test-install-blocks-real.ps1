#Requires -Version 5.1
<#
.SYNOPSIS
    runs the documented Windows install blocks against the dry run's real MSI.

.DESCRIPTION
    runs in the Install Blocks job of merge-checks.yaml, under Windows PowerShell
    5.1, on the release-build dry run's artifact. only gh is stubbed: it serves
    the artifact's real SHA256SUMS, a tag on main, and a passing attestation. the
    check and uninstall blocks come from docs/operations/cli.md and run with the
    real msiexec, the real Windows Installer COM object, and the real registry.
    the installed app is never started. a failed check throws.
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
$UpgradeCode = '{F53C5F78-BA20-4941-9091-988AFDFD656B}'
$ReleaseDirectory = (Resolve-Path -LiteralPath $ReleaseDirectory).Path
$MsiName = "marshal-$Version-windows-x64.msi"
$AppName = "marshal-$Version-windows-x64.exe"
$GenuineMsi = Join-Path $ReleaseDirectory $MsiName
$GenuineApp = Join-Path $ReleaseDirectory $AppName
$InstallDirectory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
$InstalledApp = Join-Path $InstallDirectory 'marshal-app.exe'
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

function Assert-That {
    param([Parameter(Mandatory = $true)][bool]$Condition, [Parameter(Mandatory = $true)][string]$Message)
    if (-not $Condition) { throw "FAILED: $Message" }
    Write-Output "ok   $Message"
}

function Get-RelatedProductCode {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    return , @($installer.GetType().InvokeMember('RelatedProducts', 'GetProperty', $null, $installer, @($UpgradeCode)))
}

function Assert-NothingInstalled {
    param([string]$Label)
    Assert-That (-not (Test-Path -LiteralPath $InstalledApp)) "$Label - marshal-app.exe is not at the install path"
    Assert-That ((Get-RelatedProductCode).Count -eq 0) "$Label - no product holds Marshal's upgrade code"
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
# set after the clean-start guard passes, so a refusal on a host that has Marshal
# installed never uninstalls it.
$cleanupArmed = $false

try {
    foreach ($required in @($GenuineMsi, $GenuineApp, (Join-Path $ReleaseDirectory 'SHA256SUMS'))) {
        Assert-That (Test-Path -LiteralPath $required -PathType Leaf) "the artifact holds $(Split-Path -Leaf $required)"
    }
    $msiHash = Get-Sha256 $GenuineMsi
    $appHash = Get-Sha256 $GenuineApp
    $sums = @(Get-Content -LiteralPath (Join-Path $ReleaseDirectory 'SHA256SUMS'))
    $listed = @($sums | Where-Object { ($_ -split '\s+')[1] -eq $MsiName })
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
    $otherApp = ('1' * 64)
    Set-Content -LiteralPath (Join-Path $served 'SHA256SUMS') -Value @("$otherApp  marshal-9.9.9-windows-x64.exe", "$msiHash  marshal-9.9.9-windows-x64.msi")
    $script:Scenario.Release = $served
    $result = Invoke-CheckBlock -BlockVersion '9.9.9' -Path $renamed
    $script:Scenario.Release = $ReleaseDirectory
    Assert-That ($null -ne $result.Failure -and $result.Failure.StartsWith('refused: ')) 'the check block refuses'
    Assert-That ($result.Failure.Contains('the app in the installer does not match the marshal-9.9.9-windows-x64.exe line of SHA256SUMS')) "the refusal is at the app step: $($result.Failure)"
    Assert-NothingInstalled -Label 'after the refused check'
    Assert-That ((Get-Sha256 $renamed) -eq $msiHash -and (Get-ZoneIdentifier $renamed) -eq $markBefore) 'the refused check left the file and its mark unchanged'

    Write-Output '== install, then the uninstall block =='
    $code = Invoke-Msi -Arguments @('/i', "`"$GenuineMsi`"") -LogName 'install'
    Assert-That ($code -eq 0) "the MSI installs (msiexec exit code $code)"
    Assert-That ((Get-Sha256 $InstalledApp) -eq $appHash) 'the installed marshal-app.exe matches its SHA256SUMS line'
    $result = Invoke-DocumentedBlock -Id 'windows-uninstall'
    Assert-That ($null -eq $result.Failure) "the uninstall block passes ($($result.Failure))"
    Assert-NothingInstalled -Label 'after the uninstall block'
    Write-Output 'All Windows install block checks passed.'
} finally {
    if ($cleanupArmed) {
        foreach ($product in (Get-RelatedProductCode)) {
            $null = Invoke-Msi -Arguments @('/x', $product) -LogName "cleanup-$($product.Trim('{}'))"
        }
    }
    Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue
}

exit 0
