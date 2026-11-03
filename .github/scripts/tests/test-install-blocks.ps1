#Requires -Version 5.1
<#
.SYNOPSIS
    runs the Windows install blocks of docs/operations/cli.md against stand-ins.

.DESCRIPTION
    runs the optional check, migration, uninstall, and bare-binary blocks, each
    extracted from the document, with stand-ins for gh, msiexec (through
    Start-Process), Unblock-File, the Windows Installer COM object, and marshal.exe.
    each stand-in records its calls, so every refusal is produced and a case
    asserts what ran and what did not. LOCALAPPDATA points into a temporary
    directory. the cases that write the user PATH in the registry run on Windows
    only and put the value back; elsewhere they print SKIP. runs under Windows
    PowerShell 5.1 and PowerShell 7. see docs/operations/cli.md.
#>
param(
    [string]$Document = (Join-Path (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))) 'docs/operations/cli.md')
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'install-block-helpers.ps1')
Assert-JqAvailable

$script:Document = $Document
$env:STANDIN_RECORD_CALLS = '1'
$OnWindows = Test-RunningOnWindows
$Version = '1.2.3'
$CommitSha = '0123456789abcdef0123456789abcdef01234567'
$UpgradeCode = '{512F67C6-97ED-4562-84F3-2584FA3BA29C}'
$InstallerName = "marshal-$Version-windows-x64.msi"
$script:Failures = 0
$script:Cases = 0
$script:Skipped = 0
$script:CaseName = ''
$script:Scenario = @{}
$script:Calls = New-Object System.Collections.Generic.List[string]
$script:Result = $null

Add-Type -TypeDefinition @'
using System.Runtime.CompilerServices;
public class FakeWindowsInstaller {
    public string[] Codes = new string[0];
    public string LastUpgradeCode;
    [IndexerName("RelatedProducts")]
    public string[] this[string upgradeCode] {
        get { LastUpgradeCode = upgradeCode; return Codes; }
    }
}
'@

# the stand-ins -----------------------------------------------------------------

function Unblock-File {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidOverwritingBuiltInCmdlets', '', Justification = 'a stand-in must shadow the cmdlet the block calls')]
    param([string]$LiteralPath)
    $script:Calls.Add("Unblock-File $LiteralPath")
}

# stands in for msiexec, which the blocks start through Start-Process. /a unpacks
# the scenario's files under TARGETDIR; /x records the product code; /i is never
# expected from a block, so it throws.
function Start-Process {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidOverwritingBuiltInCmdlets', '', Justification = 'a stand-in must shadow the cmdlet the block calls')]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'a stand-in changes no state')]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSReviewUnusedParameter', '', Justification = 'the block passes -Wait and -PassThru')]
    param([string]$FilePath, $ArgumentList, [switch]$Wait, [switch]$PassThru)
    $arguments = @($ArgumentList)
    $script:Calls.Add("Start-Process $FilePath $($arguments -join ' ')")
    if ($FilePath -ne 'msiexec.exe') { throw "The block started $FilePath, not msiexec.exe." }
    $scenario = $script:Scenario
    $code = 0
    if ($arguments[0] -eq '/a') {
        $code = $scenario.UnpackExit
        $targetArgument = @($arguments | Where-Object { $_ -like 'TARGETDIR=*' })[0]
        $target = $targetArgument.Substring('TARGETDIR='.Length).Trim('"')
        if ($code -eq 0) {
            foreach ($relative in $scenario.UnpackedBinaries) {
                Write-StandInMarshal -Path (Join-Path $target $relative) -Version $scenario.UnpackedVersion -VersionExit $scenario.UnpackedVersionExit
            }
            if (@($scenario.UnpackedBinaries).Count -eq 0) { New-Item -ItemType Directory -Force -Path $target | Out-Null }
        }
    } elseif ($arguments[0] -eq '/x') {
        $code = $scenario.RemoveExit
    } else {
        throw "The block ran msiexec.exe $($arguments -join ' ')."
    }
    return [pscustomobject]@{ ExitCode = $code; Handle = 0 }
}

function New-Object {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidOverwritingBuiltInCmdlets', '', Justification = 'a stand-in must shadow the cmdlet the block calls')]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'a stand-in changes no state')]
    param([Parameter(Position = 0)][string]$TypeName, [Parameter(Position = 1)]$ArgumentList, [string]$ComObject)
    if ($ComObject -eq 'WindowsInstaller.Installer') {
        $script:Calls.Add('New-Object -ComObject WindowsInstaller.Installer')
        if ($script:Scenario.InstallerFail) { throw 'Windows Installer is not available.' }
        $installer = Microsoft.PowerShell.Utility\New-Object FakeWindowsInstaller
        $installer.Codes = @($script:Scenario.Products)
        $script:Scenario.Installer = $installer
        return $installer
    }
    Microsoft.PowerShell.Utility\New-Object @PSBoundParameters
}

# the harness -------------------------------------------------------------------

$Work = Join-Path ([IO.Path]::GetTempPath()) ('marshal-install-blocks-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $Work | Out-Null
Initialize-StandInMarshal -Directory $Work
$OriginalLocalAppData = $env:LOCALAPPDATA
$OriginalTemp = $env:TEMP
$OriginalTmp = $env:TMP
$OriginalTmpdir = $env:TMPDIR
$OriginalUserProfile = $env:USERPROFILE

function Fail {
    param([string]$Message)
    Write-Output "FAIL $($script:CaseName): $Message"
    $script:Failures++
}

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { Fail $Message }
}

# Open-Case <name>: fresh directories, an empty call log, and the default scenario.
function Open-Case {
    param([string]$Name)
    $script:Cases++
    $script:CaseName = $Name
    $script:CaseDirectory = Join-Path $Work 'case'
    if (Test-Path -LiteralPath $script:CaseDirectory) { Remove-Item -LiteralPath $script:CaseDirectory -Recurse -Force }
    foreach ($child in 'release', 'downloads', 'local', 'tmp') {
        New-Item -ItemType Directory -Force -Path (Join-Path $script:CaseDirectory $child) | Out-Null
    }
    $env:LOCALAPPDATA = Join-Path $script:CaseDirectory 'local'
    $env:USERPROFILE = Join-Path $script:CaseDirectory 'profile'
    $env:TEMP = Join-Path $script:CaseDirectory 'tmp'
    $env:TMP = $env:TEMP
    $env:TMPDIR = $env:TEMP
    $script:Calls.Clear()
    $script:Result = $null
    $script:Scenario = @{
        Release = Join-Path $script:CaseDirectory 'release'
        DownloadFail = $false
        TagFail = $false
        TagType = 'commit'
        TagSha = $CommitSha
        AnnotatedFail = $false
        AnnotatedSha = $CommitSha
        CompareFail = $false
        Compare = 'identical'
        AttestFail = $false
        UnpackExit = 0
        UnpackedBinaries = @('Programs\marshal\marshal.exe')
        UnpackedVersion = $Version
        UnpackedVersionExit = 0
        RemoveExit = 0
        Products = @()
        InstallerFail = $false
    }
}

function Complete-Case {
    Write-Output "ok   $($script:CaseName)"
}

function Skip-Case {
    param([string]$Reason)
    $script:Skipped++
    Write-Output "SKIP $($script:CaseName): $Reason"
}

# Invoke-Block <id> <replacements>: runs the documented block with each named
# assignment line replaced, and keeps what it wrote and how it ended.
function Invoke-Block {
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
    $script:Result = [pscustomobject]@{ Output = ($output -join "`n"); Failure = $failure }
}

function Assert-Passed {
    if ($null -ne $script:Result.Failure) { Fail "the block failed: $($script:Result.Failure)" }
}

function Assert-Refused {
    param([string]$Text)
    $failure = $script:Result.Failure
    if ($null -eq $failure) { Fail "the block passed, expected a refusal containing '$Text': $($script:Result.Output)"; return }
    if (-not $failure.StartsWith('refused: ')) { Fail "the block failed without 'refused:': $failure"; return }
    if ($failure.IndexOf($Text, [StringComparison]::Ordinal) -lt 0) { Fail "the refusal '$failure' does not contain '$Text'" }
}

function Assert-Called {
    param([string]$Text)
    if (-not (@($script:Calls | Where-Object { $_.Contains($Text) }).Count -gt 0)) {
        Fail "'$Text' was not called; calls: $($script:Calls -join '; ')"
    }
}

function Assert-NotCalled {
    param([string]$Text)
    if (@($script:Calls | Where-Object { $_.Contains($Text) }).Count -gt 0) {
        Fail "'$Text' was called; calls: $($script:Calls -join '; ')"
    }
}

# an installer starts a /i or /x: neither the check block nor a refusal may.
function Assert-InstallsNothing {
    Assert-NotCalled 'msiexec.exe /i'
    Assert-NotCalled 'msiexec.exe /x'
}

function Assert-Absent {
    param([string]$Path)
    if (Test-Path -LiteralPath $Path) { Fail "$Path exists" }
}

function Assert-PathPresent {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { Fail "$Path does not exist" }
}

function Assert-CleanTemp {
    $left = @(Get-ChildItem -LiteralPath $env:TEMP -Force)
    if ($left.Count -gt 0) { Fail "the block left temporary files: $(($left | ForEach-Object { $_.Name }) -join ', ')" }
}

# the check block ---------------------------------------------------------------

# Initialize-Check: an installer of known bytes in the downloads folder, with a
# real download mark on Windows, and the SHA256SUMS line for the name the version gives.
function Initialize-Check {
    param([string]$DownloadedName = $InstallerName)
    $script:Installer = Join-Path (Join-Path $script:CaseDirectory 'downloads') $DownloadedName
    Set-Content -LiteralPath $script:Installer -Value 'installer bytes'
    if ($OnWindows) { Set-Content -LiteralPath $script:Installer -Stream Zone.Identifier -Value "[ZoneTransfer]`r`nZoneId=3" }
    $script:InstallerHash = Get-Sha256 $script:Installer
    Set-Content -LiteralPath (Join-Path $script:Scenario.Release 'SHA256SUMS') -Value "$($script:InstallerHash)  $InstallerName"
}

function Invoke-Check {
    Invoke-Block -Id 'windows-check' -Replace @{
        '$version =' = "`$version = '$Version'"
        '$file =' = "`$file = '$($script:Installer)'"
    }
}

function Assert-CheckLeftTheFile {
    Assert-True ((Get-Sha256 $script:Installer) -eq $script:InstallerHash) 'the downloaded file changed'
    if ($OnWindows) {
        Assert-True ($null -ne (Get-Content -LiteralPath $script:Installer -Stream Zone.Identifier -ErrorAction SilentlyContinue)) 'the download mark is gone'
    }
    Assert-CleanTemp
}

Open-Case 'check: a genuine installer passes and installs nothing'
Initialize-Check
Invoke-Check
Assert-Passed
Assert-True ($script:Result.Output.Contains('Passed:')) "no passed line: $($script:Result.Output)"
Assert-Called "gh release download v$Version --repo axross/runners --pattern SHA256SUMS --dir "
Assert-NotCalled "--pattern $InstallerName"
Assert-Called "gh attestation verify $($script:Installer) --repo axross/runners --signer-workflow axross/runners/.github/workflows/release.yaml --source-ref refs/heads/main --source-digest $CommitSha --deny-self-hosted-runners"
Assert-Called 'Unblock-File '
Assert-NotCalled "Unblock-File $($script:Installer)"
Assert-Called 'Start-Process msiexec.exe /a '
Assert-NotCalled "/a `"$($script:Installer)`""
Assert-InstallsNothing
Assert-CheckLeftTheFile
Complete-Case

Open-Case 'check: the file may be called anything and sit in a folder with spaces'
Initialize-Check
New-Item -ItemType Directory -Force -Path (Join-Path $script:CaseDirectory 'my downloads') | Out-Null
$renamed = Join-Path (Join-Path $script:CaseDirectory 'my downloads') 'marshal (1).msi'
Move-Item -LiteralPath $script:Installer -Destination $renamed
$script:Installer = $renamed
Invoke-Check
Assert-Passed
Assert-InstallsNothing
Complete-Case

Open-Case 'check: an annotated tag is dereferenced'
Initialize-Check
$script:Scenario.TagType = 'tag'
$script:Scenario.TagSha = 'ffffffffffffffffffffffffffffffffffffffff'
Invoke-Check
Assert-Passed
Assert-Called 'gh api repos/axross/runners/git/tags/ffffffffffffffffffffffffffffffffffffffff'
Assert-Called "compare/main...$CommitSha"
Complete-Case

# a block that ended with exit would end this process, so the finally block names the
# case before the process goes. it runs before the other refusals for that reason.
Open-Case 'check: a refusal does not end the calling shell'
Initialize-Check
$script:Scenario.AttestFail = $true
$continued = $false
try {
    Invoke-Check
    $continued = $true
} finally {
    if (-not $continued) { Write-Output "FAIL $($script:CaseName): the block ended the calling shell" }
}
Assert-True $continued 'the calling shell did not continue after the refusal'
Assert-Refused 'the attestation does not verify'
Complete-Case

Open-Case 'check: a commit behind main passes'
Initialize-Check
$script:Scenario.Compare = 'behind'
Invoke-Check
Assert-Passed
Assert-True ($script:Result.Output.Contains('Passed:')) "no passed line: $($script:Result.Output)"
Assert-Called "compare/main...$CommitSha"
Assert-Called "gh attestation verify $($script:Installer) "
Assert-InstallsNothing
Complete-Case

$refusals = @(
    @{ Name = 'SHA256SUMS that cannot be downloaded'; Set = { $script:Scenario.DownloadFail = $true }; Text = 'SHA256SUMS could not be downloaded'; NotCalled = @('gh attestation') },
    @{ Name = 'a tag that cannot be read'; Set = { $script:Scenario.TagFail = $true }; Text = "the tag v$Version could not be read"; NotCalled = @('gh attestation') },
    @{ Name = 'an annotated tag that cannot be read'; Set = { $script:Scenario.TagType = 'tag'; $script:Scenario.AnnotatedFail = $true }; Text = "the annotated tag v$Version could not be read"; NotCalled = @('gh attestation') },
    @{ Name = 'a tag that is not a commit'; Set = { $script:Scenario.TagType = 'tree' }; Text = "v$Version does not resolve to a commit"; NotCalled = @('gh attestation') },
    @{ Name = 'a commit that cannot be compared with main'; Set = { $script:Scenario.CompareFail = $true }; Text = 'could not be compared with main'; NotCalled = @('gh attestation') },
    @{ Name = 'a commit ahead of main'; Set = { $script:Scenario.Compare = 'ahead' }; Text = 'is not on main (ahead)'; NotCalled = @('gh attestation', 'Start-Process') },
    @{ Name = 'a commit diverged from main'; Set = { $script:Scenario.Compare = 'diverged' }; Text = 'is not on main (diverged)'; NotCalled = @('gh attestation', 'Start-Process') },
    @{ Name = 'an attestation that does not verify'; Set = { $script:Scenario.AttestFail = $true }; Text = 'the attestation does not verify'; NotCalled = @('Unblock-File', 'Start-Process') },
    @{ Name = 'an installer msiexec cannot unpack'; Set = { $script:Scenario.UnpackExit = 1603 }; Text = 'msiexec could not unpack the installer (exit code 1603)'; NotCalled = @() },
    @{ Name = 'an installer with no marshal.exe'; Set = { $script:Scenario.UnpackedBinaries = @() }; Text = 'holds 0 files named marshal.exe, not 1'; NotCalled = @() },
    @{ Name = 'an installer with two marshal.exe'; Set = { $script:Scenario.UnpackedBinaries = @('a\marshal.exe', 'b\marshal.exe') }; Text = 'holds 2 files named marshal.exe, not 1'; NotCalled = @() },
    @{ Name = 'a binary that prints another version'; Set = { $script:Scenario.UnpackedVersion = '0.0.1' }; Text = "is not marshal ${Version}: marshal 0.0.1"; NotCalled = @() },
    @{ Name = 'a binary that does not run'; Set = { $script:Scenario.UnpackedVersionExit = 3 }; Text = 'is not marshal'; NotCalled = @() }
)
foreach ($refusal in $refusals) {
    Open-Case "check: $($refusal.Name)"
    Initialize-Check
    & $refusal.Set
    Invoke-Check
    Assert-Refused $refusal.Text
    foreach ($text in $refusal.NotCalled) { Assert-NotCalled $text }
    Assert-InstallsNothing
    Assert-CheckLeftTheFile
    Complete-Case
}

Open-Case 'check: no line for the file name the version gives'
Initialize-Check
Set-Content -LiteralPath (Join-Path $script:Scenario.Release 'SHA256SUMS') -Value "$($script:InstallerHash)  marshal-9.9.9-windows-x64.msi"
Invoke-Check
Assert-Refused "does not match the $InstallerName line of SHA256SUMS"
Assert-NotCalled 'gh api'
Assert-NotCalled 'gh attestation'
Assert-CheckLeftTheFile
Complete-Case

Open-Case 'check: a checksum that does not match'
Initialize-Check
Set-Content -LiteralPath (Join-Path $script:Scenario.Release 'SHA256SUMS') -Value ('0' * 64 + "  $InstallerName")
Invoke-Check
Assert-Refused "does not match the $InstallerName line of SHA256SUMS"
Assert-NotCalled 'gh api'
Assert-NotCalled 'gh attestation'
Assert-NotCalled 'Start-Process'
Assert-CheckLeftTheFile
Complete-Case

Open-Case 'check: two lines for the file name'
Initialize-Check
Set-Content -LiteralPath (Join-Path $script:Scenario.Release 'SHA256SUMS') -Value @("$($script:InstallerHash)  $InstallerName", "$($script:InstallerHash)  $InstallerName")
Invoke-Check
Assert-Refused "does not match the $InstallerName line of SHA256SUMS"
Assert-NotCalled 'gh attestation'
Complete-Case

Open-Case 'check: a file that is not there'
Initialize-Check
Remove-Item -LiteralPath $script:Installer -Force
Invoke-Check
Assert-Refused 'could not be read'
Assert-NotCalled 'gh attestation'
Complete-Case

# the migration block -----------------------------------------------------------

$ManualDirectory = { Join-Path $env:LOCALAPPDATA 'Programs\marshal' }

# the user PATH is a Windows registry value, so off Windows the block ends at its
# PATH step with a refusal, after it has stopped the instance and removed the file.
Open-Case 'migrate: the marshal installer is installed, so its file goes too'
$script:Scenario.Products = @('{11111111-2222-3333-4444-555555555555}')
$manual = Join-Path (& $ManualDirectory) 'marshal.exe'
Write-StandInMarshal -Path $manual -Version '0.0.1'
Invoke-Block -Id 'windows-migrate'
if ($OnWindows) {
    Assert-Passed
    Assert-True ($script:Result.Output.Contains('The marshal installer is installed, and what was removed was part of it too; uninstall it and install it again.')) "no note: $($script:Result.Output)"
    Assert-True ($script:Result.Output.Contains('marshal.exe')) "the removal is not named: $($script:Result.Output)"
} else {
    Assert-Refused 'the user PATH could not be updated'
}
Assert-True ((Get-StandInCall $manual) -contains 'stop') 'the instance was not asked to stop'
Assert-Absent $manual
Assert-InstallsNothing
Assert-True ($script:Scenario.Installer.LastUpgradeCode -eq $UpgradeCode) "the products were looked up by '$($script:Scenario.Installer.LastUpgradeCode)', not the fixed upgrade code"
Complete-Case

Open-Case 'migrate: an installer that cannot be listed'
$script:Scenario.InstallerFail = $true
Invoke-Block -Id 'windows-migrate'
Assert-Refused 'the installed products could not be listed'
Complete-Case

Open-Case 'migrate: an instance that does not stop'
$manual = Join-Path (& $ManualDirectory) 'marshal.exe'
Write-StandInMarshal -Path $manual -Version '0.0.1' -StopOutput 'the control channel did not answer' -StopExit 1
Invoke-Block -Id 'windows-migrate'
Assert-Refused 'the instance did not stop: the control channel did not answer'
Assert-PathPresent $manual
Complete-Case

Open-Case 'migrate: the manual install and its PATH entry are removed'
if (-not $OnWindows) {
    Skip-Case 'the user PATH is a Windows registry value'
} else {
    $manual = Join-Path (& $ManualDirectory) 'marshal.exe'
    Write-StandInMarshal -Path $manual -Version '0.0.1'
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    $originalPath = $key.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $originalKind = if ($null -ne $originalPath) { $key.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
    # recording would leave marshal.exe.calls in the directory the block must remove.
    $env:STANDIN_RECORD_CALLS = '0'
    try {
        $seeded = 'C:\keep-one;%LOCALAPPDATA%\Programs\marshal;C:\keep-two;' + (& $ManualDirectory) + '\'
        $key.SetValue('Path', $seeded, [Microsoft.Win32.RegistryValueKind]::ExpandString)
        Invoke-Block -Id 'windows-migrate'
        Assert-Passed
        Assert-Called 'New-Object -ComObject WindowsInstaller.Installer'
        foreach ($removal in 'marshal.exe', 'the empty directory', 'the user PATH entry for') {
            Assert-True ($script:Result.Output.Contains($removal)) "the output does not name '$removal': $($script:Result.Output)"
        }
        Assert-True (-not $script:Result.Output.Contains('nothing to migrate')) "the output says nothing was there: $($script:Result.Output)"
        Assert-True (-not $script:Result.Output.Contains('The marshal installer is installed')) "the output mentions an installer that is not installed: $($script:Result.Output)"
        Assert-Absent $manual
        Assert-Absent (& $ManualDirectory)
        $after = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        Assert-True ($after -eq 'C:\keep-one;C:\keep-two') "the user PATH holds '$after', not the two other entries"
        Assert-True ($key.GetValueKind('Path') -eq [Microsoft.Win32.RegistryValueKind]::ExpandString) 'the user PATH is no longer an expandable string'
    } finally {
        if ($null -eq $originalPath) { $key.DeleteValue('Path', $false) } else { $key.SetValue('Path', $originalPath, $originalKind) }
        $key.Close()
        $env:STANDIN_RECORD_CALLS = '1'
    }
    Complete-Case
}

Open-Case 'migrate: there is no manual install'
if (-not $OnWindows) {
    Skip-Case 'the user PATH is a Windows registry value'
} else {
    Invoke-Block -Id 'windows-migrate'
    Assert-Passed
    Assert-True ($script:Result.Output.Contains('nothing to migrate')) "no note: $($script:Result.Output)"
    Assert-True (-not $script:Result.Output.Contains('Removed')) "the output claims a removal: $($script:Result.Output)"
    Complete-Case
}

# the uninstall block -----------------------------------------------------------

Open-Case 'uninstall: no product is installed'
Invoke-Block -Id 'windows-uninstall'
Assert-Passed
Assert-True ($script:Result.Output.Contains('nothing to uninstall')) "no note: $($script:Result.Output)"
Assert-NotCalled 'Start-Process'
Complete-Case

Open-Case 'uninstall: the instance is stopped and the product removed'
$script:Scenario.Products = @('{11111111-2222-3333-4444-555555555555}')
$installed = Join-Path (& $ManualDirectory) 'marshal.exe'
Write-StandInMarshal -Path $installed -Version $Version
Invoke-Block -Id 'windows-uninstall'
Assert-Passed
Assert-Called 'Start-Process msiexec.exe /x {11111111-2222-3333-4444-555555555555} /qn /norestart'
Assert-True ((Get-StandInCall $installed) -contains 'stop') 'the instance was not asked to stop'
Assert-True ($script:Scenario.Installer.LastUpgradeCode -eq $UpgradeCode) 'the products were not looked up by the fixed upgrade code'
Complete-Case

Open-Case 'uninstall: an instance that does not stop'
$script:Scenario.Products = @('{11111111-2222-3333-4444-555555555555}')
$installed = Join-Path (& $ManualDirectory) 'marshal.exe'
Write-StandInMarshal -Path $installed -Version $Version -StopOutput 'the control channel did not answer' -StopExit 1
Invoke-Block -Id 'windows-uninstall'
Assert-Refused 'the instance did not stop: the control channel did not answer'
Assert-NotCalled 'msiexec.exe /x'
Complete-Case

Open-Case 'uninstall: msiexec that fails'
$script:Scenario.Products = @('{11111111-2222-3333-4444-555555555555}')
$script:Scenario.RemoveExit = 1603
Invoke-Block -Id 'windows-uninstall'
Assert-Refused 'exit code 1603'
Complete-Case

# the bare-binary block ---------------------------------------------------------

$BareVersion = '0.1.0'
$BareAsset = "marshal-$BareVersion-windows-x64.exe"

function Initialize-Bare {
    param([string]$AssetVersion = $BareVersion, [string]$VersionAtInstallPath = '')
    Write-StandInMarshal -Path (Join-Path $script:Scenario.Release $BareAsset) -Version $AssetVersion -VersionAtInstallPath $VersionAtInstallPath
    $hash = Get-Sha256 (Join-Path $script:Scenario.Release $BareAsset)
    Set-Content -LiteralPath (Join-Path $script:Scenario.Release 'SHA256SUMS') -Value "$hash  $BareAsset"
}

Open-Case 'bare binary: a genuine binary is installed'
Initialize-Bare
Invoke-Block -Id 'windows-bare'
Assert-Passed
$target = Join-Path (& $ManualDirectory) 'marshal.exe'
Assert-PathPresent $target
Assert-True ((Get-Sha256 $target) -eq (Get-Sha256 (Join-Path $script:Scenario.Release $BareAsset))) 'the installed file is not the downloaded one'
Assert-Called "gh attestation verify "
Assert-Called "--source-digest $CommitSha --deny-self-hosted-runners"
Complete-Case

Open-Case 'bare binary: a commit behind main passes'
Initialize-Bare
$script:Scenario.Compare = 'behind'
Invoke-Block -Id 'windows-bare'
Assert-Passed
Assert-Called "compare/main...$CommitSha"
Assert-Called 'gh attestation verify '
Assert-PathPresent (Join-Path (& $ManualDirectory) 'marshal.exe')
Complete-Case

Open-Case 'bare binary: a running instance is stopped before the binary is replaced'
Initialize-Bare
$target = Join-Path (& $ManualDirectory) 'marshal.exe'
Write-StandInMarshal -Path $target -Version '0.0.1'
Invoke-Block -Id 'windows-bare'
Assert-Passed
Assert-True ((Get-StandInCall $target) -contains 'stop') 'the old copy was not asked to stop'
Assert-True ((Get-Sha256 $target) -eq (Get-Sha256 (Join-Path $script:Scenario.Release $BareAsset))) 'the binary was not replaced'
Complete-Case

Open-Case 'bare binary: a checksum that does not match'
Initialize-Bare
Set-Content -LiteralPath (Join-Path $script:Scenario.Release 'SHA256SUMS') -Value ('0' * 64 + "  $BareAsset")
Invoke-Block -Id 'windows-bare'
Assert-Refused "$BareAsset does not match SHA256SUMS"
Assert-Absent (Join-Path (& $ManualDirectory) 'marshal.exe')
Assert-NotCalled 'gh attestation'
Complete-Case

Open-Case 'bare binary: a commit that is not on main'
Initialize-Bare
$script:Scenario.Compare = 'diverged'
Invoke-Block -Id 'windows-bare'
Assert-Refused 'is not on main (diverged)'
Assert-Absent (Join-Path (& $ManualDirectory) 'marshal.exe')
Complete-Case

Open-Case 'bare binary: an attestation that does not verify'
Initialize-Bare
$script:Scenario.AttestFail = $true
Invoke-Block -Id 'windows-bare'
Assert-Refused 'the attestation does not verify'
Assert-Absent (Join-Path (& $ManualDirectory) 'marshal.exe')
Complete-Case

Open-Case 'bare binary: a binary that reports another version once installed'
Initialize-Bare -VersionAtInstallPath '0.9.9'
Invoke-Block -Id 'windows-bare'
Assert-Refused "the installed binary is not marshal ${BareVersion}: marshal 0.9.9"
Assert-PathPresent (Join-Path (& $ManualDirectory) 'marshal.exe')
Complete-Case

Open-Case 'bare binary: an older binary under a newer name leaves the install path alone'
Initialize-Bare -AssetVersion '0.0.9'
$target = Join-Path (& $ManualDirectory) 'marshal.exe'
Write-StandInMarshal -Path $target -Version '0.0.1'
$before = Get-Sha256 $target
Invoke-Block -Id 'windows-bare'
Assert-Refused "is not marshal ${BareVersion}: marshal 0.0.9"
Assert-True ((Get-Sha256 $target) -eq $before) 'the install path changed'
Assert-True ((Get-StandInCall $target).Count -eq 0) 'the installed copy was asked to stop'
Complete-Case

Open-Case 'bare binary: an instance that does not stop leaves the install path alone'
Initialize-Bare
$target = Join-Path (& $ManualDirectory) 'marshal.exe'
Write-StandInMarshal -Path $target -Version '0.0.1' -StopOutput 'the control channel did not answer' -StopExit 1
$before = Get-Sha256 $target
Invoke-Block -Id 'windows-bare'
Assert-Refused 'the instance did not stop: the control channel did not answer'
Assert-True ((Get-Sha256 $target) -eq $before) 'the install path changed'
Complete-Case

# ------------------------------------------------------------------------------

$env:LOCALAPPDATA = $OriginalLocalAppData
$env:TEMP = $OriginalTemp
$env:TMP = $OriginalTmp
$env:TMPDIR = $OriginalTmpdir
$env:USERPROFILE = $OriginalUserProfile
Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue

Write-Output ''
if ($script:Failures -ne 0) {
    Write-Output "$($script:Failures) failure(s) in $($script:Cases - $script:Skipped) cases that ran, $($script:Skipped) skipped."
    exit 1
}
Write-Output "$($script:Cases - $script:Skipped) install block cases passed, $($script:Skipped) skipped under PowerShell $($PSVersionTable.PSVersion)."
# the CI shell wrapper exits with the last native exit code, which a stop that reported "no instance" leaves at 1.
exit 0
