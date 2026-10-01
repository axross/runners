#Requires -Version 5.1
#Requires -RunAsAdministrator
<#
.SYNOPSIS
    one-time setup on the runner host: registers the supervisor and weekly
    rebuild scheduled tasks for the current user, and restricts every token
    file the host configuration lists to that user.

.DESCRIPTION
    Registers "<hostPrefix>-supervisor", which runs supervisor.ps1 at this
    user's sign-in, and "<hostPrefix>-weekly-rebuild", which runs
    rebuild-image.ps1 weekly. Both run as the current user at the limited run
    level, never elevated. Both point at this script's own directory and at
    the configuration file, so moving the checkout or the file breaks them
    silently until this script is run again.

    Safe to run again after editing the configuration or replacing a token
    file: re-registration replaces the tasks, and the permission step resets
    each token file's permissions and then restricts them again. Needs an
    elevated prompt because registering a sign-in task does.

.PARAMETER ConfigPath
    Path to the host configuration json file. The tasks keep the absolute path.

.PARAMETER WeeklyRebuildDayOfWeek
    Day the image is rebuilt.

.PARAMETER WeeklyRebuildTime
    Local time the image is rebuilt.
#>
param(
    [Parameter(Mandatory)][string]$ConfigPath,
    [string]$WeeklyRebuildDayOfWeek = 'Sunday',
    [string]$WeeklyRebuildTime = '03:00'
)

$ErrorActionPreference = 'Stop'
$InformationPreference = 'Continue'

. (Join-Path $PSScriptRoot 'host-configuration.ps1')

$plan = Read-HostConfiguration -Path $ConfigPath
$absoluteConfigPath = (Resolve-Path -LiteralPath $ConfigPath).Path
$currentUser = "$env:USERDOMAIN\$env:USERNAME"

# windows clients default to an execution policy that refuses local script files;
# bypass applies to these task processes only, not to the machine.
function Get-PowerShellArgument {
    param([Parameter(Mandatory)][string]$ScriptName)

    return "-NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $PSScriptRoot $ScriptName)`" -ConfigPath `"$absoluteConfigPath`""
}

function Register-SupervisorTask {
    $taskName = "$($plan.HostPrefix)-supervisor"
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument (Get-PowerShellArgument -ScriptName 'supervisor.ps1')
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $currentUser
    # a backstop for the supervisor process exiting, so the host is not left
    # without runners until the next sign-in. the slots wait out a docker desktop
    # restart themselves; whether task scheduler restarts a task that ends with a
    # non-zero exit code is not verified on a real host.
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit (New-TimeSpan -Days 0)
    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings `
        -User $currentUser -RunLevel Limited -Force | Out-Null
    Write-Information "Registered scheduled task '$taskName' (runs at sign-in for $currentUser)."
}

function Register-WeeklyRebuildTask {
    param([string]$DayOfWeek, [string]$Time)

    $taskName = "$($plan.HostPrefix)-weekly-rebuild"
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument (Get-PowerShellArgument -ScriptName 'rebuild-image.ps1')
    $trigger = New-ScheduledTaskTrigger -Weekly -DaysOfWeek $DayOfWeek -At $Time
    $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable
    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings `
        -User $currentUser -RunLevel Limited -Force | Out-Null
    Write-Information "Registered scheduled task '$taskName' ($DayOfWeek at $Time)."
}

function Protect-TokenFile {
    param([Parameter(Mandatory)]$Entry)

    if (-not (Test-Path -LiteralPath $Entry.TokenPath -PathType Leaf)) {
        Write-Warning "$($Entry.Path): token file not found at $($Entry.TokenPath) yet - create it and run this script again to restrict its permissions."
        return
    }
    # reset first, so an entry granted by hand to another account, or inherited
    # from the folder, is dropped. then remove inheritance, so the grant below is
    # the only entry on the file, rather than an addition to what the folder
    # allows.
    & icacls $Entry.TokenPath /reset | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "icacls could not reset the permissions of $($Entry.TokenPath)."
    }
    & icacls $Entry.TokenPath /inheritance:r | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "icacls could not remove inherited permissions from $($Entry.TokenPath)."
    }
    & icacls $Entry.TokenPath /grant:r "${currentUser}:(R)" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "icacls could not restrict $($Entry.TokenPath) to $currentUser."
    }
    Write-Information "Restricted $($Entry.TokenPath) to $currentUser (read-only)."
}

Register-SupervisorTask
Register-WeeklyRebuildTask -DayOfWeek $WeeklyRebuildDayOfWeek -Time $WeeklyRebuildTime
foreach ($entry in $plan.Repositories) {
    Protect-TokenFile -Entry $entry
}

Write-Information 'Done. Sign out and back in, or start the supervisor task from Task Scheduler, to start it now.'
