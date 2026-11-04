#Requires -Version 5.1
<#
.SYNOPSIS
    runs the check block's extraction and the MSI install as a standard user.

.DESCRIPTION
    the hosted Windows runner runs elevated, so a plain step cannot show whether
    the documented check block or the MSI needs administrator rights. this script
    creates a local user outside the Administrators group and runs a task as that
    user with a limited run level, after granting that user the batch logon right
    the task scheduler needs to start it. the task runs the documented check block from
    docs/operations/cli.md against the dry run's real MSI, with only gh stubbed,
    then installs the MSI with msiexec /i, runs the installed binary, and runs the
    documented uninstall block. the outcome is printed on lines that start with
    MEASURED, and the script throws unless every step succeeded. it needs an
    elevated shell to create the user and the task, and removes both at the end.
    it refuses to run anywhere but a GitHub Actions runner, because it creates and
    deletes a local user, a scheduled task and a folder under Public.
#>
param(
    [Parameter(Mandatory = $true)][string]$ReleaseDirectory,
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Document = (Join-Path (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))) 'docs/operations/cli.md')
)

$ErrorActionPreference = 'Stop'

if ($env:GITHUB_ACTIONS -ne 'true') {
    [Console]::Error.WriteLine('error: this test creates and deletes a local user, a scheduled task and a folder under Public, so it runs only on a GitHub Actions runner (GITHUB_ACTIONS=true).')
    exit 2
}

if ($null -eq (Get-Command -Name jq -CommandType Application -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('error: jq is required on PATH for the gh stand-in; install it (for example winget install jqlang.jq, apt install jq or brew install jq).')
    exit 2
}

$ReleaseDirectory = (Resolve-Path -LiteralPath $ReleaseDirectory).Path
$UserName = 'marshalstd'
$TaskName = 'marshal-standard-user-test'
$Staging = Join-Path $env:PUBLIC 'marshal-standard-user'
$PolicyDirectory = Join-Path $(if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }) 'marshal-standard-user-policy'
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

# runs secedit and returns its exit code and log text; a non-zero exit throws unless -AllowNonZeroExit is given.
function Invoke-Secedit {
    param([Parameter(Mandatory = $true)][string[]]$Arguments, [Parameter(Mandatory = $true)][string]$LogPath, [switch]$AllowNonZeroExit)
    if (Test-Path -LiteralPath $LogPath) { Remove-Item -LiteralPath $LogPath -Force }
    & secedit.exe @Arguments /log $LogPath | Out-Null
    $exitCode = $LASTEXITCODE
    $logText = if (Test-Path -LiteralPath $LogPath) { (Get-Content -LiteralPath $LogPath) -join "`n" } else { '' }
    if ($exitCode -ne 0 -and -not $AllowNonZeroExit) { throw "secedit $($Arguments -join ' ') exited $exitCode; log: $logText" }
    return [pscustomobject]@{ ExitCode = $exitCode; Log = $logText }
}

# the holders of one user right in an exported policy, as the entries of its line
# (a SID with a leading *, or an account name); empty when the policy has no such line.
function Get-RightHolder {
    param([Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Lines, [Parameter(Mandatory = $true)][string]$Right)
    foreach ($line in $Lines) {
        if ($line -match "^$Right\s*=\s*(.*)$") {
            return @($Matches[1].Split(',') | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' })
        }
    }
    return @()
}

function Export-UserRight {
    param([Parameter(Mandatory = $true)][string]$Directory, [Parameter(Mandatory = $true)][string]$Name)
    $path = Join-Path $Directory "$Name.inf"
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
    $null = Invoke-Secedit -Arguments @('/export', '/cfg', $path, '/areas', 'USER_RIGHTS') -LogPath (Join-Path $Directory "$Name.log")
    return $path
}

# adds the user to "Log on as a batch job" (SeBatchLogonRight) and proves it with a second export.
# the right is held by Administrators by default, not by Users, and a task registered with a
# user name and password logs on as a batch job, so without it the task is registered but never starts.
# it only lets the test launch a process as the user; a person installing logs on interactively.
function Grant-BatchLogonRight {
    param([Parameter(Mandatory = $true)][string]$Sid, [Parameter(Mandatory = $true)][string]$Account, [Parameter(Mandatory = $true)][string]$Directory)
    New-Item -ItemType Directory -Force -Path $Directory | Out-Null
    $exported = Export-UserRight -Directory $Directory -Name 'before'
    $lines = New-Object 'System.Collections.Generic.List[string]'
    $lines.AddRange([string[]]@(Get-Content -LiteralPath $exported))
    $entry = "*$Sid"
    $right = -1
    $section = -1
    for ($index = 0; $index -lt $lines.Count; $index++) {
        if ($right -lt 0 -and $lines[$index] -match '^SeBatchLogonRight\s*=') { $right = $index }
        if ($section -lt 0 -and $lines[$index].Trim() -eq '[Privilege Rights]') { $section = $index }
    }
    if ($right -ge 0) {
        $current = ($lines[$right] -replace '^SeBatchLogonRight\s*=\s*', '').Trim()
        $lines[$right] = if ($current -eq '') { "SeBatchLogonRight = $entry" } else { "SeBatchLogonRight = $current,$entry" }
    } else {
        if ($section -lt 0) {
            $lines.Add('[Privilege Rights]')
            $section = $lines.Count - 1
        }
        $lines.Insert($section + 1, "SeBatchLogonRight = $entry")
    }
    $template = Join-Path $Directory 'grant.inf'
    Set-Content -LiteralPath $template -Value $lines -Encoding Unicode
    # secedit can exit non-zero on a warning about an exported entry it cannot map while the grant is applied,
    # so the second export decides, not this exit code.
    $configure = Invoke-Secedit -Arguments @('/configure', '/db', (Join-Path $Directory 'grant.sdb'), '/cfg', $template, '/areas', 'USER_RIGHTS') -LogPath (Join-Path $Directory 'grant.log') -AllowNonZeroExit
    if ($configure.ExitCode -ne 0) { Write-Output "secedit /configure exited $($configure.ExitCode); log: $($configure.Log)" }

    $after = @(Get-Content -LiteralPath (Export-UserRight -Directory $Directory -Name 'after'))
    foreach ($name in 'SeBatchLogonRight', 'SeDenyBatchLogonRight') {
        Write-Output "user right $name = $(@(Get-RightHolder -Lines $after -Right $name) -join ',')"
    }
    # an export lists a local account by its SID or, on some systems, by name, so either proves the grant.
    $holders = @(Get-RightHolder -Lines $after -Right 'SeBatchLogonRight')
    if (@($entry, $Account, "$env:COMPUTERNAME\$Account" | Where-Object { $holders -contains $_ }).Count -eq 0) { throw "SeBatchLogonRight does not hold $entry after the grant; secedit /configure exited $($configure.ExitCode); log: $($configure.Log)" }
}

# one best-effort diagnostic: a failure to gather it is printed and never replaces the failure being reported.
function Write-Diagnostic {
    param([Parameter(Mandatory = $true)][string]$Title, [Parameter(Mandatory = $true)][scriptblock]$Gather, [object[]]$Arguments = @())
    Write-Output "-- $Title --"
    try { Write-Output ((& $Gather @Arguments | Out-String).TrimEnd()) } catch { Write-Output "(not available: $($_.Exception.Message))" }
}

# the task, its principal and settings, and the scheduler and logon events since $Since. it prints
# no password and no task XML; the principal and settings are read as properties.
function Write-TaskDiagnostic {
    param([Parameter(Mandatory = $true)][string]$Name, [Parameter(Mandatory = $true)][string]$Account, [Parameter(Mandatory = $true)][datetime]$Since)
    Write-Output '== the standard user task diagnostics =='
    Write-Diagnostic 'task info' -Arguments $Name -Gather {
        param($task)
        Get-ScheduledTaskInfo -TaskName $task | Format-List LastRunTime, LastTaskResult, NumberOfMissedRuns, NextRunTime
    }
    Write-Diagnostic 'task principal' -Arguments $Name -Gather {
        param($task)
        (Get-ScheduledTask -TaskName $task).Principal | Format-List UserId, LogonType, RunLevel
    }
    Write-Diagnostic 'task settings' -Arguments $Name -Gather {
        param($task)
        (Get-ScheduledTask -TaskName $task).Settings | Format-List DisallowStartIfOnBatteries, StopIfGoingOnBatteries, Enabled, ExecutionTimeLimit
    }
    Write-Diagnostic 'Task Scheduler operational events for the task' -Arguments $Name, $Since -Gather {
        param($task, $from)
        $found = @(Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-TaskScheduler/Operational'; StartTime = $from } -ErrorAction SilentlyContinue |
            Where-Object { $_.Message -like "*$task*" } | Select-Object -First 30)
        if ($found.Count -eq 0) { '(no events; the log may be disabled)' } else { $found | Format-List TimeCreated, Id, Message }
    }
    Write-Diagnostic "Security events 4625 (failed logons) for $Account" -Arguments $Account, $Since -Gather {
        param($user, $from)
        $found = @(Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 4625; StartTime = $from } -ErrorAction SilentlyContinue |
            Where-Object { $_.Message -like "*$user*" } | Select-Object -First 5)
        if ($found.Count -eq 0) { '(no events)' } else { $found | Format-List TimeCreated, Id, Message }
    }
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
    Grant-BatchLogonRight -Sid (Get-LocalUser -Name $UserName).SID.Value -Account $UserName -Directory $PolicyDirectory

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
    # the defaults of a task registered without settings do not start it on battery power.
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 10)
    $testStart = Get-Date
    # best effort: the operational log is off by default, and the diagnostics read it when the task does not start.
    try { & wevtutil.exe sl Microsoft-Windows-TaskScheduler/Operational /e:true | Out-Null } catch { Write-Output "(could not enable the Task Scheduler operational log: $($_.Exception.Message))" }
    $null = Register-ScheduledTask -TaskName $TaskName -Action $action -Settings $settings -User "$env:COMPUTERNAME\$UserName" -Password $password -RunLevel Limited
    Start-ScheduledTask -TaskName $TaskName
    $logPath = Join-Path $Staging 'result.log'
    # a task that has not run yet is Ready too, with the last result 267011
    # (SCHED_S_TASK_HAS_NOT_RUN), so the wait ends on the task's RESULT line, or on
    # a task that is no longer running and has a result, such as one that never started the script.
    # a task that is still in that state with no log after $startGrace seconds is not going to start.
    $notRunYet = 267011
    $startGrace = 90
    $started = Get-Date
    $deadline = $started.AddMinutes(8)
    do {
        Start-Sleep -Seconds 5
        $state = (Get-ScheduledTask -TaskName $TaskName).State
        $info = Get-ScheduledTaskInfo -TaskName $TaskName
        $logText = if (Test-Path -LiteralPath $logPath) { (Get-Content -LiteralPath $logPath) -join "`n" } else { '' }
        $finished = $logText -match '(?m)^RESULT:' -or ($state -ne 'Running' -and $info.LastTaskResult -ne $notRunYet)
        $neverStarted = -not $finished -and $info.LastTaskResult -eq $notRunYet -and $state -ne 'Running' -and -not (Test-Path -LiteralPath $logPath) -and ((Get-Date) - $started).TotalSeconds -ge $startGrace
    } while (-not $finished -and -not $neverStarted -and (Get-Date) -lt $deadline)
    if ($neverStarted) {
        Write-TaskDiagnostic -Name $TaskName -Account $UserName -Since $testStart
        throw "the standard user task did not start within $startGrace seconds; task state: $state; last task result: $($info.LastTaskResult)"
    }
    if (-not $finished) {
        Write-TaskDiagnostic -Name $TaskName -Account $UserName -Since $testStart
        throw "the standard user task did not finish within 8 minutes; task state: $state; last task result: $($info.LastTaskResult); log: $logText"
    }
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
    if (Test-Path -LiteralPath $PolicyDirectory) { Remove-Item -LiteralPath $PolicyDirectory -Recurse -Force -ErrorAction SilentlyContinue }
}

# the CI shell wrapper exits with the last native exit code, which a stop that reported "no instance" leaves at 1.
exit 0
