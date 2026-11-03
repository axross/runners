#Requires -Version 5.1
<#
.SYNOPSIS
    runs the check block's extraction and the MSI install as a standard user.

.DESCRIPTION
    the hosted Windows runner runs elevated, so a plain step cannot show whether
    the documented check block or the MSI needs administrator rights. this script
    creates a local user outside the Administrators group and runs a task as that
    user with a limited run level. the task runs the documented check block from
    docs/operations/cli.md against the dry run's real MSI, with only gh stubbed,
    then installs the MSI with msiexec /i, runs the installed binary, and runs the
    documented uninstall block. the outcome is printed on lines that start with
    MEASURED, and the script throws unless every step succeeded. it needs an
    elevated shell to create the user and the task, and removes both at the end.
#>
param(
    [Parameter(Mandatory = $true)][string]$ReleaseDirectory,
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Document = (Join-Path (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))) 'docs/operations/cli.md')
)

$ErrorActionPreference = 'Stop'

if ($null -eq (Get-Command -Name jq -CommandType Application -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('error: jq is required on PATH for the gh stand-in; install it (for example winget install jqlang.jq, apt install jq or brew install jq).')
    exit 2
}

$ReleaseDirectory = (Resolve-Path -LiteralPath $ReleaseDirectory).Path
$UserName = 'marshalstd'
$TaskName = 'marshal-standard-user-test'
$Staging = Join-Path $env:PUBLIC 'marshal-standard-user'
$MsiName = "marshal-$Version-windows-x64.msi"

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

# the task script. it runs as the standard user under Windows PowerShell 5.1,
# writes every step to result.log, and ends with a line that starts with RESULT.
$taskScript = @'
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$log = Join-Path $here 'result.log'
function Write-Step { param([string]$Text) Add-Content -LiteralPath $log -Value $Text }
try {
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
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    Write-Step "user: $($principal.Identity.Name)"
    Write-Step "administrator: $($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))"
    Write-Step ((whoami /groups | Out-String).Trim())
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'the task runs as an administrator' }

    $msiName = "marshal-$version-windows-x64.msi"
    $msi = Join-Path $here $msiName
    $block = Get-InstallBlock -Document $script:Document -Id 'windows-check'
    $block = Edit-BlockLine -Block $block -Prefix '$version =' -Line "`$version = '$version'"
    $block = Edit-BlockLine -Block $block -Prefix '$file =' -Line "`$file = '$msi'"
    $output = @(& ([scriptblock]::Create($block)) *>&1 | ForEach-Object { "$_" })
    Write-Step "check block: $($output -join ' | ')"
    if (-not ($output -join "`n").Contains('Passed:')) { throw 'the check block did not pass' }

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
    Set-Content -LiteralPath (Join-Path $Staging 'run.ps1') -Value $taskScript
    & icacls.exe $Staging /grant "*S-1-5-32-545:(OI)(CI)M" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "icacls exited $LASTEXITCODE" }

    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $Staging 'run.ps1')`""
    $null = Register-ScheduledTask -TaskName $TaskName -Action $action -User "$env:COMPUTERNAME\$UserName" -Password $password -RunLevel Limited
    Start-ScheduledTask -TaskName $TaskName
    $logPath = Join-Path $Staging 'result.log'
    # a task that has not run yet is Ready too, with the last result 267011
    # (SCHED_S_TASK_HAS_NOT_RUN), so the wait ends on the task's RESULT line, or on
    # a task that is no longer running and has a result, such as one that never started the script.
    $notRunYet = 267011
    $deadline = (Get-Date).AddMinutes(8)
    do {
        Start-Sleep -Seconds 5
        $state = (Get-ScheduledTask -TaskName $TaskName).State
        $info = Get-ScheduledTaskInfo -TaskName $TaskName
        $logText = if (Test-Path -LiteralPath $logPath) { (Get-Content -LiteralPath $logPath) -join "`n" } else { '' }
        $finished = $logText -match '(?m)^RESULT:' -or ($state -ne 'Running' -and $info.LastTaskResult -ne $notRunYet)
    } while (-not $finished -and (Get-Date) -lt $deadline)
    if (-not $finished) { throw "the standard user task did not finish within 8 minutes; task state: $state; last task result: $($info.LastTaskResult); log: $logText" }
    Write-Output '== the standard user run =='
    Write-Output $logText
    Write-Output "task state: $state; last task result: $($info.LastTaskResult)"
    $ok = $logText.Contains('RESULT: ok')
    Write-Measured "a standard local user without administrator rights ($UserName, run level Limited) $(if ($ok) { 'ran the check block, installed the MSI with msiexec /i, ran the installed binary, and ran the uninstall block' } else { 'did not complete the check block, the install, and the uninstall' }); task last result $($info.LastTaskResult)"
    if (-not $ok) { throw 'the standard user run failed; the check block or the install needs more than a standard user, or the run could not be set up' }
} finally {
    if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) { Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false }
    if (Get-LocalUser -Name $UserName -ErrorAction SilentlyContinue) { Remove-LocalUser -Name $UserName }
    if (Test-Path -LiteralPath $Staging) { Remove-Item -LiteralPath $Staging -Recurse -Force -ErrorAction SilentlyContinue }
}
