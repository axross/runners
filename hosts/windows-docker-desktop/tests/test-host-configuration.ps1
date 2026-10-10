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
    sets none. the job container's `docker run` arguments, a slot job's
    positional arguments against its worker script block's parameters, and the
    stale container name pattern are built and checked without Docker. runs under the PowerShell that runs it, on Windows
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
    'default-labels-only.json'       = @('repositories[0].labels:', 'is always added')
    'labels-not-list.json'           = @('repositories[0].labels: must be a list of custom labels')
    'labels-null.json'               = @('repositories[0].labels: must be a list of custom labels')
    'axpc-label.json'                = @("repositories[0].labels: 'AXPC' is always added")
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
    'name-trailing-newline.json'     = @('repositories[0].name: must be lowercase letters, digits and hyphens')
    'owner-trailing-newline.json'    = @('repositories[0].owner: must be a GitHub owner name')
    'repository-trailing-newline.json' = @('repositories[0].repository: must be a GitHub repository name')
    'image-name-trailing-newline.json' = @('imageName: must be a local Docker image name')
    'label-trailing-newline.json'    = @('repositories[0].labels: each label must be a string of letters')
    'volume-suffix-trailing-newline.json' = @('repositories[0].volumes[0].suffix: must be lowercase letters, digits and hyphens')
    'mount-path-trailing-newline.json' = @('repositories[0].volumes[0].mountPath: must be an absolute container path')
    'leftover-host-prefix.json'      = @('configuration.hostPrefix: unknown field')
    'leftover-prefix.json'           = @('repositories[0].prefix: unknown field')
    'cpus-zero.json'                 = @('repositories[0].cpus: must be a number greater than 0 and at most 64')
    'cpus-negative.json'             = @('repositories[0].cpus: must be a number greater than 0 and at most 64')
    'cpus-too-many.json'             = @('repositories[0].cpus: must be a number greater than 0 and at most 64')
    'cpus-string.json'               = @('repositories[0].cpus: must be a number greater than 0 and at most 64')
    'cpus-rounds-to-zero.json'       = @('repositories[0].cpus: must be a number greater than 0 and at most 64, and large enough not to be written as 0')
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
    'no-labels-field.json' = @{ Contains = @("labels:            self-hosted, linux, x64, axpc`n"); Lacks = @() }
    'empty-labels.json'   = @{ Contains = @("labels:            self-hosted, linux, x64, axpc`n"); Lacks = @() }
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
    -Passed ($accepted.Text.Contains('labels:            self-hosted, linux, x64, axpc, example-label-one')) -Detail $accepted.Text
Assert-Case -Name 'example registers the default labels and the second custom label' `
    -Passed ($accepted.Text.Contains('labels:            self-hosted, linux, x64, axpc, example-label-two')) -Detail $accepted.Text

# the job container's arguments for the entry in accepted/explicit-limits.json,
# with the limits formatted the way the supervisor formats them for a slot job.
# the limits and the name must sit before the image, because Docker reads
# whatever follows the image as the container's own command.
$limitsPlan = Read-HostConfiguration -Path (Join-Path $acceptedDirectory 'explicit-limits.json')
$limitsEntry = $limitsPlan.Repositories[0]
$mountArguments = @('--mount', "type=volume,source=$($limitsEntry.Volumes[0].Name),target=$($limitsEntry.Volumes[0].MountPath)")
$jobName = Get-JobContainerName -EntryName $limitsEntry.Name -Slot 2 -Now (New-Object DateTime 2024, 3, 5, 6, 7, 8, 9)
$jobArguments = @(Get-JobContainerArgument -Name $jobName -Cpus (Format-CpuCount -Cpus $limitsEntry.Cpus) -CpusetCpus '5,11' -MemoryGb $limitsEntry.MemoryGb `
        -Mounts $mountArguments -JitConfigVariable 'ACTIONS_RUNNER_INPUT_JITCONFIG' -ImageName $limitsPlan.ImageName -RunCommand '/home/runner/run.sh')
$imageIndex = [Array]::IndexOf($jobArguments, $limitsPlan.ImageName)

function Test-FlagBeforeImage {
    param([string]$Flag, [string]$Value)

    $index = [Array]::IndexOf($jobArguments, $Flag)
    return $index -ge 0 -and $index + 1 -lt $imageIndex -and $jobArguments[$index + 1] -ceq $Value
}

Assert-Case -Name 'job container name is the entry name, slot index and timestamp' `
    -Passed ($jobName -ceq 'example-repo-one-2-20240305060708009' -and (Test-FlagBeforeImage -Flag '--name' -Value $jobName)) -Detail "name: $jobName; arguments: $($jobArguments -join ' ')"
Assert-Case -Name 'job container gets its CPU limit before the image' `
    -Passed (Test-FlagBeforeImage -Flag '--cpus' -Value '1.5') -Detail ($jobArguments -join ' ')
Assert-Case -Name 'job container gets selected CPU IDs independently of quota' `
    -Passed (Test-FlagBeforeImage -Flag '--cpuset-cpus' -Value '5,11') -Detail 'affinity missing'
Assert-Case -Name 'job container gets its memory limit before the image' `
    -Passed (Test-FlagBeforeImage -Flag '--memory' -Value '16g') -Detail ($jobArguments -join ' ')
Assert-Case -Name 'job container gets no swap beyond its memory limit' `
    -Passed (Test-FlagBeforeImage -Flag '--memory-swap' -Value '16g') -Detail ($jobArguments -join ' ')
Assert-Case -Name 'job container takes the registration from the environment, not the command line' `
    -Passed ((Test-FlagBeforeImage -Flag '-e' -Value 'ACTIONS_RUNNER_INPUT_JITCONFIG') -and $jobArguments[-1] -ceq '/home/runner/run.sh') `
    -Detail ($jobArguments -join ' ')

$noMountArguments = @(Get-JobContainerArgument -Name $jobName -Cpus '1.5' -CpusetCpus '5,11' -MemoryGb 16 `
        -JitConfigVariable 'ACTIONS_RUNNER_INPUT_JITCONFIG' -ImageName 'actions-runner:local' -RunCommand '/home/runner/run.sh')
$emptyMountArguments = @(Get-JobContainerArgument -Name $jobName -Cpus '1.5' -CpusetCpus '5,11' -MemoryGb 16 -Mounts @() `
        -JitConfigVariable 'ACTIONS_RUNNER_INPUT_JITCONFIG' -ImageName 'actions-runner:local' -RunCommand '/home/runner/run.sh')
foreach ($case in @(@('omitted', $noMountArguments), @('empty', $emptyMountArguments))) {
    $arguments = $case[1]
    $blank = @($arguments | Where-Object { [string]::IsNullOrEmpty($_) })
    Assert-Case -Name "job container with $($case[0]) mounts has no blank argument and preserves the command after its guard" `
        -Passed ($blank.Count -eq 0 -and $arguments[-7] -ceq 'actions-runner:local' -and $arguments[-1] -ceq '/home/runner/run.sh' -and $arguments[-2] -ceq '5,11') `
        -Detail ($arguments -join ' ')
}

# the slot job's arguments are positional, so the keys of Get-SlotWorkerArgument
# must be the worker script block's parameter names in the same order, and each
# value must fit the type of the parameter at its position.
$syntaxTree = [System.Management.Automation.Language.Parser]::ParseFile($supervisor, [ref]$null, [ref]$null)
$workerAssignment = $syntaxTree.Find({
        param($node)
        $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
        $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
        $node.Left.VariablePath.UserPath -ceq 'WorkerScript'
    }, $true)
$workerBlock = $workerAssignment.Right.Find({ param($node) $node -is [System.Management.Automation.Language.ScriptBlockExpressionAst] }, $true)
$workerParameters = @($workerBlock.ScriptBlock.ParamBlock.Parameters)
$workerParameterNames = @($workerParameters | ForEach-Object { $_.Name.VariablePath.UserPath })

$workerArguments = Get-SlotWorkerArgument -Entry $limitsEntry -Slot 3 -ScriptRoot 'C:\host' -ImageName $limitsPlan.ImageName `
    -Mounts $mountArguments -RunCommand '/home/runner/run.sh' -JitConfigVariable 'ACTIONS_RUNNER_INPUT_JITCONFIG' `
    -InitialBackoffSeconds 5 -MaxBackoffSeconds 300
$workerArgumentNames = @($workerArguments.Keys)
$workerArgumentValues = @($workerArguments.Values)

Assert-Case -Name 'slot job arguments are keyed by the worker parameters, in order' `
    -Passed ($workerParameterNames.Count -gt 0 -and ($workerParameterNames -join ',') -ceq ($workerArgumentNames -join ',')) `
    -Detail "parameters: $($workerParameterNames -join ', '); arguments: $($workerArgumentNames -join ', ')"
$misfits = @()
for ($position = 0; $position -lt [Math]::Min($workerParameters.Count, $workerArgumentValues.Count); $position++) {
    if (-not $workerParameters[$position].StaticType.IsInstanceOfType($workerArgumentValues[$position])) {
        $misfits += "$($workerParameterNames[$position]) at $position"
    }
}
Assert-Case -Name 'slot job argument values fit their worker parameter types' -Passed ($misfits.Count -eq 0) -Detail "misfit: $($misfits -join ', ')"
Assert-Case -Name 'slot job arguments carry the entry, the slot, and the formatted limits' `
    -Passed ($workerArguments.Cpus -ceq '1.5' -and $workerArguments.MemoryGb -eq 16 -and $workerArguments.Slot -eq 3 `
        -and $workerArguments.CpuAffinityCount -eq 2 -and $workerArguments.CpuAffinityOffset -eq 4 `
        -and $workerArguments.EntryName -ceq 'example-repo-one' -and $workerArguments.Owner -ceq 'example-owner' `
        -and $workerArguments.Repository -ceq 'example-repo-one' -and $workerArguments.TokenPath -ceq $limitsEntry.TokenPath `
        -and ($workerArguments.Labels -join ',') -ceq ($limitsEntry.Labels -join ',') -and ($workerArguments.Mounts -join ',') -ceq ($mountArguments -join ',') `
        -and $workerArguments.ImageName -ceq 'actions-runner:local' -and $workerArguments.InitialBackoffSeconds -eq 5 -and $workerArguments.MaxBackoffSeconds -eq 300) `
    -Detail (($workerArguments.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join '; ')

# the stale-container cleanup matches only the containers Get-JobContainerName
# gives this entry.
$ownPattern = Get-JobContainerNamePattern -Name 'example-repo-one'
$stamp = '20240305060708009'
Assert-Case -Name 'stale container pattern matches the name Get-JobContainerName gives' `
    -Passed ($jobName -cmatch $ownPattern) -Detail "pattern: $ownPattern; name: $jobName"
$notOwn = [ordered]@{
    'a non-numeric slot'         = "example-repo-one-x-$stamp"
    'a 16-digit timestamp'       = "example-repo-one-1-$($stamp.Substring(1))"
    'a longer name before it'    = "xexample-repo-one-1-$stamp"
    'another entry extending it' = "example-repo-one-extra-1-$stamp"
    'a trailing newline'         = "example-repo-one-1-$stamp`n"
}
foreach ($case in $notOwn.Keys) {
    Assert-Case -Name "stale container pattern rejects $case" -Passed ($notOwn[$case] -cnotmatch $ownPattern) -Detail "pattern: $ownPattern; name: $($notOwn[$case])"
}

$previousCulture = [System.Threading.Thread]::CurrentThread.CurrentCulture
$germanCulture = $null
try {
    $germanCulture = New-Object System.Globalization.CultureInfo 'de-DE'
} catch {
    Write-Output 'SKIP  CPU count keeps its decimal point under a comma-decimal culture - the de-DE culture is not available'
}
if ($null -ne $germanCulture) {
    try {
        [System.Threading.Thread]::CurrentThread.CurrentCulture = $germanCulture
        $germanCpus = Format-CpuCount -Cpus 1.5
    } finally {
        [System.Threading.Thread]::CurrentThread.CurrentCulture = $previousCulture
    }
    Assert-Case -Name 'CPU count keeps its decimal point under a comma-decimal culture' -Passed ($germanCpus -ceq '1.5') -Detail "formatted: $germanCpus"
}

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

. (Join-Path $PSScriptRoot 'test-cpu-affinity.ps1')
. (Join-Path $PSScriptRoot 'test-diagnostics.ps1')

if ($failures.Count -gt 0) {
    Write-Output "$($failures.Count) check(s) failed."
    exit 1
}
Write-Output 'All host configuration checks passed.'
