#Requires -Version 5.1
<#
.SYNOPSIS
    checks the host configuration validation through the supervisor's
    -ValidateOnly mode, and a few rules the host scripts must keep.

.DESCRIPTION
    each fixture under fixtures/ breaks exactly one rule and must be rejected
    with a non-zero exit code and a message naming the offending field. each
    fixture under accepted/ sits on the edge of a rule and must be accepted.
    the example configuration must be accepted and plan distinct names and
    volume names for its two repositories, with the default limits where it
    sets none. the job container's `docker run` arguments are built and checked
    without Docker. runs under the PowerShell that runs it, on Windows
    PowerShell 5.1 as well as PowerShell 7, and calls neither Docker nor
    GitHub.
#>
$ErrorActionPreference = 'Stop'

$hostDirectory = Split-Path -Parent $PSScriptRoot
$supervisor = Join-Path $hostDirectory 'supervisor.ps1'
$example = Join-Path $hostDirectory 'runner-host.example.json'
$fixtureDirectory = Join-Path $PSScriptRoot 'fixtures'
$acceptedDirectory = Join-Path $PSScriptRoot 'accepted'
$powershell = (Get-Process -Id $PID).Path

. (Join-Path $hostDirectory 'host-configuration.ps1')
. (Join-Path $hostDirectory 'docker-commands.ps1')

$failures = New-Object System.Collections.Generic.List[string]

# runs the supervisor in -ValidateOnly mode and returns its exit code and output.
function Invoke-Validation {
    param([Parameter(Mandatory)][string]$ConfigPath)

    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = @(& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $supervisor -ConfigPath $ConfigPath -ValidateOnly 2>&1 |
                ForEach-Object { "$_" })
        return [pscustomobject]@{ ExitCode = $LASTEXITCODE; Text = $output -join "`n" }
    } finally {
        $ErrorActionPreference = $previous
    }
}

# reports one case as passed or failed and records a failure.
function Assert-Case {
    param([string]$Name, [bool]$Passed, [string]$Detail)

    if ($Passed) {
        Write-Output "PASS  $Name"
    } else {
        Write-Output "FAIL  $Name - $Detail"
        $failures.Add($Name)
    }
}

# each fixture maps to the text its rejection message must contain.
$rejections = [ordered]@{
    'missing-labels.json'            = @('repositories[0].labels: required field is missing')
    'empty-labels.json'              = @('repositories[0].labels: at least one custom label is required')
    'default-labels-only.json'       = @('repositories[0].labels:', 'is always added')
    'duplicate-repository.json'      = @('repositories[1].repository: duplicate repository')
    'colliding-name.json'            = @('repositories[1].name: name', 'collides')
    'colliding-name-nested.json'     = @('repositories[1].name: name', 'collides')
    'colliding-name-nested-reverse.json' = @('repositories[1].name: name', 'collides')
    'missing-owner.json'             = @('repositories[0].owner: required field is missing')
    'missing-token-path.json'        = @('repositories[0].tokenPath: required field is missing')
    'missing-volumes.json'           = @('repositories[0].volumes: required field is missing')
    'missing-name.json'              = @('repositories[0].name: required field is missing')
    'name-bad-pattern.json'          = @('repositories[0].name: must be lowercase letters, digits and hyphens')
    'name-too-long.json'             = @('repositories[0].name: name is 65 characters', 'at most 64')
    'leftover-host-prefix.json'      = @('configuration.hostPrefix: unknown field')
    'leftover-prefix.json'           = @('repositories[0].prefix: unknown field')
    'cpus-zero.json'                 = @('repositories[0].cpus: must be a number greater than 0 and at most 64')
    'cpus-negative.json'             = @('repositories[0].cpus: must be a number greater than 0 and at most 64')
    'cpus-too-many.json'             = @('repositories[0].cpus: must be a number greater than 0 and at most 64')
    'cpus-string.json'               = @('repositories[0].cpus: must be a number greater than 0 and at most 64')
    'memory-zero.json'               = @('repositories[0].memoryGb: must be an integer from 1 to 256')
    'memory-too-many.json'           = @('repositories[0].memoryGb: must be an integer from 1 to 256')
    'memory-fractional.json'         = @('repositories[0].memoryGb: must be an integer from 1 to 256')
    'memory-string.json'             = @('repositories[0].memoryGb: must be an integer from 1 to 256')
    'slots-zero.json'                = @('repositories[0].slots: must be an integer')
    'mount-path-injection.json'      = @('repositories[0].volumes[0].mountPath: must be an absolute container path')
    'mount-path-dotdot.json'         = @("repositories[0].volumes[0].mountPath: must not contain '..'")
    'slots-too-many.json'            = @('repositories[0].slots: must be an integer from 1 to 16')
    'token-path-relative.json'       = @('repositories[0].tokenPath: must be an absolute Windows path')
    'token-path-drive-relative.json' = @('repositories[0].tokenPath: must be an absolute Windows path')
    'token-path-rooted-without-drive.json' = @('repositories[0].tokenPath: must be an absolute Windows path')
    'shared-token-path.json'         = @('repositories[1].tokenPath: token file', 'is already used at repositories[0]')
    'unknown-field.json'             = @('repositories[0].label: unknown field')
}

foreach ($fixture in $rejections.Keys) {
    $result = Invoke-Validation -ConfigPath (Join-Path $fixtureDirectory $fixture)
    $missing = @($rejections[$fixture] | Where-Object { -not $result.Text.Contains($_) })
    Assert-Case -Name "rejects $fixture" -Passed ($result.ExitCode -ne 0 -and $missing.Count -eq 0) `
        -Detail "exit code $($result.ExitCode); message lacks: $($missing -join ' | '); message: $($result.Text)"
}

$unlisted = @(Get-ChildItem -LiteralPath $fixtureDirectory -Filter '*.json' | Where-Object { -not $rejections.Contains($_.Name) })
Assert-Case -Name 'every fixture has an expectation' -Passed ($unlisted.Count -eq 0) `
    -Detail "no expectation for: $($unlisted.Name -join ', ')"

# each fixture maps to the text its plan summary must contain and the text it
# must not.
$acceptances = [ordered]@{
    'no-volumes.json'     = @{ Contains = @('Host configuration is valid: 1 repositories', 'cpus:              2', 'memory:            8 GB'); Lacks = @('volume:') }
    'unc-token-path.json' = @{ Contains = @('token file:        \\example-server\example-share\example-repo-one.token'); Lacks = @() }
    'max-slots.json'      = @{ Contains = @('slots:             16'); Lacks = @() }
    'explicit-limits.json' = @{ Contains = @('cpus:              1.5', 'memory:            16 GB'); Lacks = @('cpus:              2') }
    'limit-bounds.json'   = @{ Contains = @('cpus:              64', 'memory:            256 GB'); Lacks = @() }
    'max-name-length.json' = @{ Contains = @("name:              $('n' * 64)"); Lacks = @() }
    'similar-names.json'  = @{ Contains = @('Host configuration is valid: 3 repositories', "name:              example-repos`n"); Lacks = @() }
}

foreach ($fixture in $acceptances.Keys) {
    $result = Invoke-Validation -ConfigPath (Join-Path $acceptedDirectory $fixture)
    $missing = @($acceptances[$fixture].Contains | Where-Object { -not $result.Text.Contains($_) })
    $unwanted = @($acceptances[$fixture].Lacks | Where-Object { $result.Text.Contains($_) })
    Assert-Case -Name "accepts $fixture" -Passed ($result.ExitCode -eq 0 -and $missing.Count -eq 0 -and $unwanted.Count -eq 0) `
        -Detail "exit code $($result.ExitCode); message lacks: $($missing -join ' | '); message has: $($unwanted -join ' | '); message: $($result.Text)"
}

$unexpected = @(Get-ChildItem -LiteralPath $acceptedDirectory -Filter '*.json' | Where-Object { -not $acceptances.Contains($_.Name) })
Assert-Case -Name 'every accepted fixture has an expectation' -Passed ($unexpected.Count -eq 0) `
    -Detail "no expectation for: $($unexpected.Name -join ', ')"

$accepted = Invoke-Validation -ConfigPath $example
Assert-Case -Name 'accepts the example configuration' -Passed ($accepted.ExitCode -eq 0) -Detail $accepted.Text

$names = @([regex]::Matches($accepted.Text, '(?m)^\s+name:\s+(\S+)$') | ForEach-Object { $_.Groups[1].Value })
$volumes = @([regex]::Matches($accepted.Text, '(?m)^\s+volume:\s+(\S+) ->') | ForEach-Object { $_.Groups[1].Value })
Assert-Case -Name 'example plans two distinct names' `
    -Passed ($names.Count -eq 2 -and @($names | Select-Object -Unique).Count -eq 2) -Detail "names: $($names -join ', ')"
Assert-Case -Name 'example plans distinct volume names' `
    -Passed ($volumes.Count -gt 0 -and @($volumes | Select-Object -Unique).Count -eq $volumes.Count) -Detail "volumes: $($volumes -join ', ')"
Assert-Case -Name 'example plans the container name pattern for each name' `
    -Passed (@($names | Where-Object { -not $accepted.Text.Contains("containers:        $_-<index>-<timestamp>") }).Count -eq 0) -Detail $accepted.Text
Assert-Case -Name 'example plans the default limits where it sets none and its own where it does' `
    -Passed ($accepted.Text.Contains("cpus:              2`n") -and $accepted.Text.Contains("memory:            8 GB`n") `
        -and $accepted.Text.Contains("cpus:              4`n") -and $accepted.Text.Contains("memory:            16 GB")) -Detail $accepted.Text
Assert-Case -Name 'example registers the default labels and the custom label' `
    -Passed ($accepted.Text.Contains('labels:            self-hosted, linux, x64, example-label-one')) -Detail $accepted.Text

# the job container's arguments, built the way the slot worker builds them. the
# limits and the name must sit before the image, because Docker reads whatever
# follows the image as the container's own command.
$jobName = Get-JobContainerName -EntryName 'example-repo-one' -Slot 2 -Now (New-Object DateTime 2024, 3, 5, 6, 7, 8, 9)
$jobArguments = @(Get-JobContainerArgument -Name $jobName -Cpus (Format-CpuCount -Cpus 1.5) -MemoryGb 16 `
        -Mounts @('--mount', 'type=volume,source=example-repo-one-npm,target=/home/runner/.npm') `
        -JitConfigVariable 'ACTIONS_RUNNER_INPUT_JITCONFIG' -ImageName 'actions-runner:local' -RunCommand '/home/runner/run.sh')
$imageIndex = [Array]::IndexOf($jobArguments, 'actions-runner:local')

function Test-FlagBeforeImage {
    param([string]$Flag, [string]$Value)

    $index = [Array]::IndexOf($jobArguments, $Flag)
    return $index -ge 0 -and $index + 1 -lt $imageIndex -and $jobArguments[$index + 1] -ceq $Value
}

Assert-Case -Name 'job container name is the entry name, slot index and timestamp' `
    -Passed ($jobName -ceq 'example-repo-one-2-20240305060708009' -and (Test-FlagBeforeImage -Flag '--name' -Value $jobName)) -Detail "name: $jobName; arguments: $($jobArguments -join ' ')"
Assert-Case -Name 'job container gets its CPU limit before the image' `
    -Passed (Test-FlagBeforeImage -Flag '--cpus' -Value '1.5') -Detail ($jobArguments -join ' ')
Assert-Case -Name 'job container gets its memory limit before the image' `
    -Passed (Test-FlagBeforeImage -Flag '--memory' -Value '16g') -Detail ($jobArguments -join ' ')
Assert-Case -Name 'job container gets no swap beyond its memory limit' `
    -Passed (Test-FlagBeforeImage -Flag '--memory-swap' -Value '16g') -Detail ($jobArguments -join ' ')
Assert-Case -Name 'job container takes the registration from the environment, not the command line' `
    -Passed ((Test-FlagBeforeImage -Flag '-e' -Value 'ACTIONS_RUNNER_INPUT_JITCONFIG') -and $jobArguments[-2] -ceq 'actions-runner:local' -and $jobArguments[-1] -ceq '/home/runner/run.sh') `
    -Detail ($jobArguments -join ' ')

$previousCulture = [System.Threading.Thread]::CurrentThread.CurrentCulture
try {
    [System.Threading.Thread]::CurrentThread.CurrentCulture = New-Object System.Globalization.CultureInfo 'de-DE'
    $germanCpus = Format-CpuCount -Cpus 1.5
} catch {
    $germanCpus = '1.5'
} finally {
    [System.Threading.Thread]::CurrentThread.CurrentCulture = $previousCulture
}
Assert-Case -Name 'CPU count keeps its decimal point under a comma-decimal culture' -Passed ($germanCpus -ceq '1.5') -Detail "formatted: $germanCpus"

$hostFiles = @(Get-ChildItem -LiteralPath $hostDirectory -Recurse -File -Include '*.ps1', '*.json', '*.md')
$nonAscii = @($hostFiles | Where-Object { @([IO.File]::ReadAllBytes($_.FullName) | Where-Object { $_ -gt 127 }).Count -gt 0 })
Assert-Case -Name 'host files are ASCII-only' -Passed ($nonAscii.Count -eq 0) -Detail "non-ASCII: $($nonAscii.Name -join ', ')"

# patterns that would put the registration on a command line, or widen a
# container's reach, must not appear in the host scripts.
$forbidden = @('--jitconfig', '--privileged', 'docker.sock', 'Invoke-Expression')
$scripts = @(Get-ChildItem -LiteralPath $hostDirectory -File -Filter '*.ps1')
foreach ($pattern in $forbidden) {
    $hits = @($scripts | Where-Object { (Get-Content -LiteralPath $_.FullName -Raw).Contains($pattern) })
    Assert-Case -Name "host scripts never use $pattern" -Passed ($hits.Count -eq 0) -Detail "found in: $($hits.Name -join ', ')"
}

if ($failures.Count -gt 0) {
    Write-Output "$($failures.Count) check(s) failed."
    exit 1
}
Write-Output 'All host configuration checks passed.'
