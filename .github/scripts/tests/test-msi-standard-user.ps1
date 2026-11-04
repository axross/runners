#Requires -Version 5.1
<#
.SYNOPSIS
    runs the check block's extraction and the MSI install as a standard user.

.DESCRIPTION
    the hosted Windows runner runs elevated, so a plain step cannot show whether
    the documented check block or the MSI needs administrator rights. this script
    creates a local user outside the Administrators group and starts a PowerShell
    process as that user through a secondary logon (Start-Process -Credential),
    which is an interactive logon with the user's profile loaded, as a person's
    logon is. that process refuses to run as an administrator or without the
    INTERACTIVE group in its token, runs the documented check block from
    docs/operations/cli.md against the dry run's real MSI, with only gh stubbed,
    then installs the MSI with msiexec /i, runs the installed binary, and runs the
    documented uninstall block. the outcome is printed on lines that start with
    MEASURED, and the script throws unless every step succeeded. the hosted
    Windows Server image blocks a non-elevated per-user install (msiexec exits
    1625), so the script prints the Windows Installer policy as found, sets the
    DisableMSI policy value to 0 for the run, and restores it at the end. it needs
    an elevated shell to create the user, and removes the user at the end. it refuses
    to run anywhere but a GitHub Actions runner, because it creates and deletes a
    local user and a folder under Public.
#>
param(
    [Parameter(Mandatory = $true)][string]$ReleaseDirectory,
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Document = (Join-Path (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))) 'docs/operations/cli.md')
)

$ErrorActionPreference = 'Stop'

if ($env:GITHUB_ACTIONS -ne 'true') {
    [Console]::Error.WriteLine('error: this test creates and deletes a local user and a folder under Public, so it runs only on a GitHub Actions runner (GITHUB_ACTIONS=true).')
    exit 2
}

if ($null -eq (Get-Command -Name jq -CommandType Application -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('error: jq is required on PATH for the gh stand-in; install it (for example winget install jqlang.jq, apt install jq or brew install jq).')
    exit 2
}

$ReleaseDirectory = (Resolve-Path -LiteralPath $ReleaseDirectory).Path
$UserName = 'marshalstd'
$Staging = Join-Path $env:PUBLIC 'marshal-standard-user'
$LogDirectory = Join-Path $env:RUNNER_TEMP 'install-block-logs'
$MsiName = "marshal-$Version-windows-x64.msi"
$InstallerAppId = '{000C101C-0000-0000-C000-000000000046}'
$InstallerPolicyPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Installer'
$UserInstallerPolicyPath = 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\Installer'
$RunTimeoutMilliseconds = 8 * 60 * 1000
$StatusDllInitFailed = -1073741502

function Write-Measured {
    param([Parameter(Mandatory = $true)][string]$Text)
    Write-Output "MEASURED: $Text"
}

function Get-RandomPassword {
    $characters = 'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789'.ToCharArray()
    $random = New-Object System.Security.Cryptography.RNGCryptoServiceProvider
    $bytes = New-Object byte[] 24
    $random.GetBytes($bytes)
    $random.Dispose()
    $body = -join ($bytes | ForEach-Object { $characters[$_ % $characters.Length] })
    return "Aa1!$body"
}

# one best-effort diagnostic: a failure to gather it is printed and never replaces the failure being reported.
function Write-Diagnostic {
    param([Parameter(Mandatory = $true)][string]$Title, [Parameter(Mandatory = $true)][scriptblock]$Gather, [object[]]$Arguments = @())
    Write-Output "-- $Title --"
    try { Write-Output ((& $Gather @Arguments | Out-String).TrimEnd()) } catch { Write-Output "(not available: $($_.Exception.Message))" }
}

function ConvertTo-Sddl {
    param([Parameter(Mandatory = $true)][byte[]]$Bytes)
    return ([wmiclass]'\\.\root\cimv2:Win32_SecurityDescriptorHelper').BinarySDToSDDL($Bytes).SDDL
}

# one binary security descriptor of a registry value as SDDL, or the reason there is none to show.
function Get-ComSecurityDescriptor {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$Name)
    try {
        $key = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
        if ($null -eq $key) { return "$Path $Name = (key absent)" }
        $value = $key.GetValue($Name)
        if ($null -eq $value) { return "$Path $Name = (not set)" }
        return "$Path $Name = $(ConvertTo-Sddl -Bytes $value)"
    } catch {
        return "$Path $Name = (not readable: $($_.Exception.Message))"
    }
}

function Write-ComDiagnostic {
    param([Parameter(Mandatory = $true)][datetime]$Since)
    Write-Diagnostic 'DistributedCOM events' -Arguments $Since -Gather {
        param($from)
        $found = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-DistributedCOM'; StartTime = $from } -ErrorAction SilentlyContinue | Select-Object -First 30)
        if ($found.Count -eq 0) { '(no DistributedCOM events)' } else { $found | Format-List TimeCreated, Id, Message }
    }
    Write-Diagnostic 'COM security descriptors as SDDL' -Arguments $InstallerAppId -Gather {
        param($appId)
        $ole = 'HKLM:\SOFTWARE\Microsoft\Ole'
        $policy = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\DCOM'
        $installer = "HKLM:\SOFTWARE\Classes\AppID\$appId"
        foreach ($name in 'DefaultLaunchPermission', 'DefaultAccessPermission', 'MachineLaunchRestriction', 'MachineAccessRestriction') { Get-ComSecurityDescriptor -Path $ole -Name $name }
        foreach ($name in 'MachineLaunchRestriction', 'MachineAccessRestriction') { Get-ComSecurityDescriptor -Path $policy -Name $name }
        foreach ($name in 'LaunchPermission', 'AccessPermission') { Get-ComSecurityDescriptor -Path $installer -Name $name }
    }
    Write-Diagnostic 'value names of the Windows Installer AppID key' -Arguments $InstallerAppId -Gather {
        param($appId)
        $key = Get-Item -LiteralPath "HKLM:\SOFTWARE\Classes\AppID\$appId" -ErrorAction SilentlyContinue
        if ($null -eq $key) { '(key absent)' } else { $key.GetValueNames() -join ', ' }
    }
    Write-Diagnostic 'security descriptor of the msiserver service' -Gather { & sc.exe sdshow msiserver }
    Write-Diagnostic 'msiserver and seclogon services' -Gather { Get-Service -Name msiserver, seclogon | Format-Table Name, Status, StartType -AutoSize }
}

# prints on success too: it proves which logon type the run was measured with.
function Write-LogonTypeDiagnostic {
    param([Parameter(Mandatory = $true)][string]$Account, [Parameter(Mandatory = $true)][datetime]$Since)
    Write-Diagnostic "Security events 4624 (logons) for $Account; LogonType 2 is interactive, 4 is batch" -Arguments $Account, $Since -Gather {
        param($user, $from)
        $events = @()
        try {
            $events = @(Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 4624; StartTime = $from } -ErrorAction Stop)
        } catch {
            if ($_.FullyQualifiedErrorId -notlike 'NoMatchingEventsFound*') { throw }
        }
        $lines = foreach ($entry in $events) {
            $data = @{}
            foreach ($item in ([xml]$entry.ToXml()).Event.EventData.Data) { $data[$item.Name] = $item.'#text' }
            if ($data['TargetUserName'] -eq $user) {
                '{0} LogonType={1} Process={2}' -f $entry.TimeCreated.ToString('s'), $data['LogonType'], [System.IO.Path]::GetFileName($data['ProcessName'])
            }
        }
        if (@($lines).Count -eq 0) { '(no 4624 events for the user)' } else { $lines }
    }
}

function Write-AncestryDiagnostic {
    Write-Diagnostic 'ancestry of this process' -Gather {
        $processId = $PID
        for ($depth = 0; $depth -lt 8 -and $processId -gt 0; $depth++) {
            $process = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $processId"
            if ($null -eq $process) { break }
            $services = @(Get-CimInstance -ClassName Win32_Service -Filter "ProcessId = $processId" | ForEach-Object { $_.Name })
            "$($process.Name) pid=$processId session=$($process.SessionId) services=$($services -join ',')"
            $processId = $process.ParentProcessId
        }
    }
    Write-Diagnostic 'quser' -Gather { & cmd.exe /c 'quser 2>&1' }
}

# everything worth reading after a failed run; each part is best effort.
function Write-FailureDiagnostic {
    param([Parameter(Mandatory = $true)][string]$Account, [Parameter(Mandatory = $true)][datetime]$Since)
    Write-Output '== the standard user run diagnostics =='
    Write-ComDiagnostic -Since $Since
    Write-Diagnostic "Security events 4625 (failed logons) for $Account" -Arguments $Account, $Since -Gather {
        param($user, $from)
        $found = @(Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 4625; StartTime = $from } -ErrorAction SilentlyContinue |
            Where-Object { $_.Message -like "*$user*" } | Select-Object -First 5)
        if ($found.Count -eq 0) { '(no events)' } else { $found | Format-List TimeCreated, Id, Message }
    }
    Write-AncestryDiagnostic
}

# copies the installer logs out of the staging folder, which the cleanup deletes, to the directory the job uploads.
function Save-InstallerLog {
    param([Parameter(Mandatory = $true)][string]$Source, [Parameter(Mandatory = $true)][string]$Destination)
    try {
        foreach ($name in 'install.log', 'probe.log') {
            $path = Join-Path $Source $name
            if (-not (Test-Path -LiteralPath $path)) { continue }
            New-Item -ItemType Directory -Force -Path $Destination | Out-Null
            Copy-Item -LiteralPath $path -Destination (Join-Path $Destination "standard-user-$name") -Force
        }
    } catch {
        Write-Output "(could not save the installer logs: $($_.Exception.Message))"
    }
}

# every value under one policy key, or the fact that the key is absent.
function Get-PolicyKeyReport {
    param([Parameter(Mandatory = $true)][string]$Path)
    $key = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($null -eq $key) { return "$Path (key absent)" }
    $names = @($key.GetValueNames())
    if ($names.Count -eq 0) { return "$Path (key present, no values)" }
    foreach ($name in $names) {
        $shown = if ($name -eq '') { '(default)' } else { $name }
        "$Path $shown = $($key.GetValue($name, $null, 'DoNotExpandEnvironmentNames')) ($($key.GetValueKind($name)))"
    }
}

# the state of the DisableMSI policy value: Presence is 'absent key', 'absent value' or 'value'.
function Get-InstallerPolicyState {
    $key = Get-Item -LiteralPath $InstallerPolicyPath -ErrorAction SilentlyContinue
    if ($null -eq $key) { return [pscustomobject]@{ Presence = 'absent key'; Data = $null; Kind = $null } }
    $data = $key.GetValue('DisableMSI', $null, 'DoNotExpandEnvironmentNames')
    if ($null -eq $data) { return [pscustomobject]@{ Presence = 'absent value'; Data = $null; Kind = $null } }
    return [pscustomobject]@{ Presence = 'value'; Data = $data; Kind = [string]$key.GetValueKind('DisableMSI') }
}

function Format-InstallerPolicyState {
    param([Parameter(Mandatory = $true)]$State)
    if ($State.Presence -ne 'value') { return $State.Presence }
    if ($State.Kind -eq 'DWord') { return "$($State.Data)" }
    return "$($State.Data) ($($State.Kind))"
}

# the hosted Windows Server image blocks a non-elevated per-user install unless DisableMSI is 0.
function Enable-InstallerPolicy {
    $null = New-Item -Path $InstallerPolicyPath -Force
    $null = New-ItemProperty -LiteralPath $InstallerPolicyPath -Name 'DisableMSI' -PropertyType DWord -Value 0 -Force
}

# puts the recorded state back: the old data and type, no value, or no key when this script created it.
function Restore-InstallerPolicy {
    param([Parameter(Mandatory = $true)]$Before)
    if ($Before.Presence -eq 'value') {
        $null = New-ItemProperty -LiteralPath $InstallerPolicyPath -Name 'DisableMSI' -PropertyType $Before.Kind -Value $Before.Data -Force
    } elseif (Test-Path -LiteralPath $InstallerPolicyPath) {
        Remove-ItemProperty -LiteralPath $InstallerPolicyPath -Name 'DisableMSI' -ErrorAction SilentlyContinue
        if ($Before.Presence -eq 'absent key') {
            $key = Get-Item -LiteralPath $InstallerPolicyPath
            if ($key.ValueCount -eq 0 -and $key.SubKeyCount -eq 0) { Remove-Item -LiteralPath $InstallerPolicyPath }
        }
    }
    $after = Get-InstallerPolicyState
    $wanted = Format-InstallerPolicyState -State $Before
    $found = Format-InstallerPolicyState -State $after
    Write-Output "restored the Windows Installer policy DisableMSI to: $found (before the run: $wanted)"
    $keyKept = $Before.Presence -eq 'absent key' -and $after.Presence -eq 'absent value'
    if ($found -ne $wanted -and -not $keyKept) { throw "DisableMSI reads $found after the restore, not $wanted" }
}

# the script the standard user runs, under Windows PowerShell 5.1.
# it writes every step to result.log and ends with a line that starts with RESULT.
$userScript = @'
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$log = Join-Path $here 'result.log'
function Write-Step { param([string]$Text) Add-Content -LiteralPath $log -Value $Text }

function Write-ProfileEnvironment {
    param([string]$Heading)
    Write-Step "$Heading; session id: $([System.Diagnostics.Process]::GetCurrentProcess().SessionId)"
    foreach ($name in 'TEMP', 'TMP', 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE') {
        Write-Step "  $name=$([Environment]::GetEnvironmentVariable($name))"
    }
}

function Invoke-ComMethod {
    param($Target, [string]$Name, [string]$Kind = 'InvokeMethod', $Arguments = $null)
    return $Target.GetType().InvokeMember($Name, $Kind, $null, $Target, $Arguments)
}

# after a failed check block: whether the installer service is needed to read the MSI, and what msiexec /a logs.
function Write-InstallerProbe {
    param([string]$Msi, [string]$Directory)
    try {
        $installer = New-Object -ComObject WindowsInstaller.Installer
        $database = Invoke-ComMethod -Target $installer -Name 'OpenDatabase' -Arguments @($Msi, 0)
        $view = Invoke-ComMethod -Target $database -Name 'OpenView' -Arguments @('SELECT `Value` FROM `Property` WHERE `Property`=''ProductVersion''')
        $null = Invoke-ComMethod -Target $view -Name 'Execute'
        $record = Invoke-ComMethod -Target $view -Name 'Fetch'
        Write-Step "probe: ProductVersion read from the MSI file: $(Invoke-ComMethod -Target $record -Name 'StringData' -Kind 'GetProperty' -Arguments @(1))"
    } catch {
        Write-Step "probe: reading ProductVersion from the MSI file failed: $($_.Exception.Message)"
    }
    try {
        $probeLog = Join-Path $Directory 'probe.log'
        $arguments = @('/a', "`"$Msi`"", '/qn', "TARGETDIR=`"$(Join-Path $Directory 'probe')`"", '/l*v', "`"$probeLog`"")
        $process = Start-Process -FilePath msiexec.exe -ArgumentList $arguments -Wait -PassThru -NoNewWindow
        $null = $process.Handle
        Write-Step "probe: msiexec /a exit code: $($process.ExitCode)"
        if (Test-Path -LiteralPath $probeLog) {
            Get-Content -LiteralPath $probeLog | Where-Object { $_ -match 'Error|Failed|System error|server|returning|1601|access' } |
                Select-Object -First 40 | ForEach-Object { Write-Step "probe.log: $_" }
        }
    } catch {
        Write-Step "probe: msiexec /a failed: $($_.Exception.Message)"
    }
}

try {
    Write-ProfileEnvironment -Heading 'environment as passed in'
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    # a logon sets these from the user's profile, and this launch passes the parent's.
    $profileDirectory = [string](Get-ItemProperty -LiteralPath "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$($identity.User.Value)").ProfileImagePath
    $env:USERPROFILE = $profileDirectory
    $env:LOCALAPPDATA = [Environment]::GetFolderPath('LocalApplicationData')
    $env:APPDATA = [Environment]::GetFolderPath('ApplicationData')
    $env:TEMP = Join-Path $env:LOCALAPPDATA 'Temp'
    $env:TMP = $env:TEMP
    Write-ProfileEnvironment -Heading 'environment from the user profile'
    if (-not $env:LOCALAPPDATA.StartsWith($profileDirectory.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw "refused: LOCALAPPDATA ($env:LOCALAPPDATA) is not under the user's profile directory ($profileDirectory)"
    }
    New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null

    $version = (Get-Content -LiteralPath (Join-Path $here 'version.txt') -TotalCount 1).Trim()
    $script:Document = Join-Path $here 'cli.md'
    . (Join-Path $here 'install-block-helpers.ps1')
    $script:Calls = New-Object System.Collections.Generic.List[string]
    $script:Scenario = @{
        Release = $here; DownloadFail = $false; TagFail = $false; TagType = 'commit'
        TagSha = '0123456789abcdef0123456789abcdef01234567'; AnnotatedFail = $false
        AnnotatedSha = '0123456789abcdef0123456789abcdef01234567'; CompareFail = $false
        Compare = 'identical'; AttestFail = $false
    }
    $principal = [Security.Principal.WindowsPrincipal]$identity
    Write-Step "user: $($principal.Identity.Name)"
    Write-Step "administrator: $($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))"
    Write-Step ((whoami /groups | Out-String).Trim())
    $interactiveSid = 'S-1-5-4'
    if (@($identity.Groups | Where-Object { $_.Value -eq $interactiveSid }).Count -eq 0) {
        throw "refused: this run is not an interactive logon (the token has no INTERACTIVE group, $interactiveSid), so it does not measure what the CLI document describes"
    }
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'the run is an administrator' }

    $msiName = "marshal-$version-windows-x64.msi"
    $msi = Join-Path $here $msiName
    $block = Get-InstallBlock -Document $script:Document -Id 'windows-check'
    $block = Edit-BlockLine -Block $block -Prefix '$version =' -Line "`$version = '$version'"
    $block = Edit-BlockLine -Block $block -Prefix '$file =' -Line "`$file = '$msi'"
    try {
        $output = @(& ([scriptblock]::Create($block)) *>&1 | ForEach-Object { "$_" })
        Write-Step "check block: $($output -join ' | ')"
        if (-not ($output -join "`n").Contains('Passed:')) { throw 'the check block did not pass' }
    } catch {
        $checkFailure = $_
        Write-InstallerProbe -Msi $msi -Directory $here
        throw $checkFailure
    }

    $installDirectory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
    $installed = Join-Path $installDirectory 'marshal.exe'
    $logFile = Join-Path $here 'install.log'
    $process = Start-Process -FilePath msiexec.exe -ArgumentList @('/i', "`"$msi`"", '/qn', '/norestart', '/l*v', "`"$logFile`"") -Wait -PassThru -NoNewWindow
    $null = $process.Handle
    Write-Step "msiexec /i exit code: $($process.ExitCode)"
    if ($process.ExitCode -ne 0) { throw "msiexec /i exited $($process.ExitCode)" }
    if (-not (Test-Path -LiteralPath $installed)) { throw "$installed does not exist after the install" }
    $printed = (& $installed --version | Out-String).Trim()
    Write-Step "installed binary: $printed"
    if ($printed -ne "marshal $version") { throw "the installed binary printed '$printed'" }

    $block = Get-InstallBlock -Document $script:Document -Id 'windows-uninstall'
    $output = @(& ([scriptblock]::Create($block)) *>&1 | ForEach-Object { "$_" })
    Write-Step "uninstall block: $($output -join ' | ')"
    if (Test-Path -LiteralPath $installed) { throw 'the uninstall block left marshal.exe' }
    Write-Step 'RESULT: ok'
} catch {
    Write-Step "RESULT: failed: $($_.Exception.Message)"
    exit 1
}
'@

$password = Get-RandomPassword
$secure = New-Object System.Security.SecureString
foreach ($character in $password.ToCharArray()) { $secure.AppendChar($character) }
$secure.MakeReadOnly()
$policyBefore = $null
$policyTouched = $false
$runFailure = $null
$restoreFailure = $null
try {
    if (Get-LocalUser -Name $UserName -ErrorAction SilentlyContinue) { Remove-LocalUser -Name $UserName }
    $null = New-LocalUser -Name $UserName -Password $secure -AccountNeverExpires -PasswordNeverExpires -Description 'standard user for the marshal install test'
    $users = Get-LocalGroup -SID 'S-1-5-32-545'
    Add-LocalGroupMember -Group $users -Member $UserName
    $administrators = Get-LocalGroup -SID 'S-1-5-32-544'
    $members = @(Get-LocalGroupMember -Group $administrators | ForEach-Object { $_.Name })
    if (@($members | Where-Object { $_ -like "*\$UserName" }).Count -ne 0) { throw "$UserName is an administrator" }

    if (Test-Path -LiteralPath $Staging) { Remove-Item -LiteralPath $Staging -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $Staging | Out-Null
    foreach ($name in $MsiName, 'SHA256SUMS') { Copy-Item -LiteralPath (Join-Path $ReleaseDirectory $name) -Destination $Staging }
    Copy-Item -LiteralPath $Document -Destination (Join-Path $Staging 'cli.md')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'install-block-helpers.ps1') -Destination $Staging
    Set-Content -LiteralPath (Join-Path $Staging 'version.txt') -Value $Version
    $runScript = Join-Path $Staging 'run.ps1'
    Set-Content -LiteralPath $runScript -Value $userScript
    & icacls.exe $Staging /grant "*S-1-5-32-545:(OI)(CI)M" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "icacls exited $LASTEXITCODE" }

    # a scheduled task logs on as a batch job, so its token has no INTERACTIVE group, and msiexec could not reach
    # the Windows Installer service from it; a secondary logon is interactive, as a person's logon is.
    Write-Output '== the Windows Installer policy as found =='
    Write-Diagnostic "values under $InstallerPolicyPath" -Arguments $InstallerPolicyPath -Gather { param($path) Get-PolicyKeyReport -Path $path }
    Write-Diagnostic "values under $UserInstallerPolicyPath of $env:USERNAME" -Arguments $UserInstallerPolicyPath -Gather { param($path) Get-PolicyKeyReport -Path $path }
    $policyBefore = Get-InstallerPolicyState
    Write-Measured "Windows Installer policy DisableMSI before the standard user run: $(Format-InstallerPolicyState -State $policyBefore); set to 0 for the run (enabled for all applications, as Microsoft documents for Windows client versions with no policy); restored afterwards"
    $policyTouched = $true
    Enable-InstallerPolicy

    $credential = New-Object System.Management.Automation.PSCredential("$env:COMPUTERNAME\$UserName", $secure)
    $testStart = Get-Date
    $failure = $null
    $exitCode = $null
    $process = $null
    try {
        $process = Start-Process -FilePath powershell.exe -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$runScript`"" -Credential $credential -LoadUserProfile -WorkingDirectory $Staging -PassThru
    } catch {
        $failure = "the standard user process could not be started: $($_.Exception.Message)"
    }
    if ($null -ne $process) {
        $null = $process.Handle
        if ($process.WaitForExit($RunTimeoutMilliseconds)) {
            $exitCode = $process.ExitCode
        } else {
            & taskkill.exe /F /T /PID $process.Id | Out-Null
            $failure = 'the standard user process did not finish within 8 minutes and was killed'
        }
    }
    if ($exitCode -eq $StatusDllInitFailed) { $failure = "the standard user process could not start (exit code $exitCode, STATUS_DLL_INIT_FAILED)" }

    $logPath = Join-Path $Staging 'result.log'
    $logText = if (Test-Path -LiteralPath $logPath) { (Get-Content -LiteralPath $logPath) -join "`n" } else { '' }
    Write-Output '== the standard user run =='
    Write-Output $logText
    Write-Output "process exit code: $exitCode"
    Write-LogonTypeDiagnostic -Account $UserName -Since $testStart
    $ok = ($null -eq $failure) -and $logText.Contains('RESULT: ok')
    Write-Measured "a standard local user without administrator rights ($UserName, secondary logon with the interactive logon type, not elevated, with DisableMSI set to 0) $(if ($ok) { 'ran the check block, installed the MSI with msiexec /i, ran the installed binary, and ran the uninstall block' } else { 'did not complete the check block, the install, and the uninstall' }); process exit code $exitCode"
    if ($null -eq $failure -and -not $ok) { $failure = 'the standard user run failed; the check block or the install needs more than a standard user, or the run could not be set up' }
    if ($null -ne $failure) {
        Write-FailureDiagnostic -Account $UserName -Since $testStart
        throw $failure
    }
} catch {
    $runFailure = $_
    throw
} finally {
    if ($policyTouched) {
        try {
            Restore-InstallerPolicy -Before $policyBefore
        } catch {
            $restoreFailure = $_
            Write-Output "could not restore the Windows Installer policy DisableMSI: $($_.Exception.Message)"
        }
    }
    Save-InstallerLog -Source $Staging -Destination $LogDirectory
    if (Get-LocalUser -Name $UserName -ErrorAction SilentlyContinue) { Remove-LocalUser -Name $UserName }
    if (Test-Path -LiteralPath $Staging) { Remove-Item -LiteralPath $Staging -Recurse -Force -ErrorAction SilentlyContinue }
    # a throw here replaces the failure in flight, so the message carries both.
    if ($null -ne $restoreFailure) {
        $original = if ($null -ne $runFailure) { "; the run had failed first: $($runFailure.Exception.Message)" } else { '' }
        throw "the Windows Installer policy DisableMSI was not restored: $($restoreFailure.Exception.Message)$original"
    }
}

# the CI shell wrapper exits with the last native exit code, which a stop that reported "no instance" leaves at 1.
exit 0
