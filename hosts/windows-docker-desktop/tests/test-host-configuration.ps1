#Requires -Version 5.1
<#
.SYNOPSIS
    checks the host configuration validation through the supervisor's
    -ValidateOnly mode, and a few rules the host scripts must keep.

.DESCRIPTION
    each fixture under fixtures/ breaks exactly one rule and must be rejected
    with a non-zero exit code and a message naming the offending field. each
    fixture under accepted/ sits on the edge of a rule and must be accepted.
    the example configuration must be accepted and plan distinct container
    prefixes and volume names for its two repositories. runs under the
    PowerShell that runs it, on Windows PowerShell 5.1 as well as PowerShell 7,
    and calls neither Docker nor GitHub.
#>
$ErrorActionPreference = 'Stop'

$hostDirectory = Split-Path -Parent $PSScriptRoot
$supervisor = Join-Path $hostDirectory 'supervisor.ps1'
$example = Join-Path $hostDirectory 'runner-host.example.json'
$fixtureDirectory = Join-Path $PSScriptRoot 'fixtures'
$acceptedDirectory = Join-Path $PSScriptRoot 'accepted'
$powershell = (Get-Process -Id $PID).Path

$failures = New-Object System.Collections.Generic.List[string]

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
    'colliding-prefix.json'          = @('repositories[1].prefix: container prefix', 'collides')
    'colliding-prefix-nested.json'   = @('repositories[1].prefix: container prefix', 'collides')
    'colliding-prefix-override.json' = @('repositories[1].prefix: container prefix', 'collides')
    'missing-owner.json'             = @('repositories[0].owner: required field is missing')
    'missing-token-path.json'        = @('repositories[0].tokenPath: required field is missing')
    'missing-volumes.json'           = @('repositories[0].volumes: required field is missing')
    'missing-host-prefix.json'       = @('hostPrefix: required field is missing')
    'slots-zero.json'                = @('repositories[0].slots: must be an integer')
    'mount-path-injection.json'      = @('repositories[0].volumes[0].mountPath: must be an absolute container path')
    'mount-path-dotdot.json'         = @("repositories[0].volumes[0].mountPath: must not contain '..'")
    'slots-too-many.json'            = @('repositories[0].slots: must be an integer from 1 to 16')
    'token-path-relative.json'       = @('repositories[0].tokenPath: must be an absolute Windows path')
    'token-path-drive-relative.json' = @('repositories[0].tokenPath: must be an absolute Windows path')
    'token-path-rooted-without-drive.json' = @('repositories[0].tokenPath: must be an absolute Windows path')
    'shared-token-path.json'         = @('repositories[1].tokenPath: token file', 'is already used at repositories[0]')
    'prefix-too-long.json'           = @('repositories[0].prefix: container prefix is', 'at most 64')
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
    'no-volumes.json'     = @{ Contains = @('Host configuration is valid: 1 repositories'); Lacks = @('volume:') }
    'unc-token-path.json' = @{ Contains = @('token file:        \\example-server\example-share\example-repo-one.token'); Lacks = @() }
    'max-slots.json'      = @{ Contains = @('slots:             16'); Lacks = @() }
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

$prefixes = @([regex]::Matches($accepted.Text, '(?m)^\s+container prefix:\s+(\S+)$') | ForEach-Object { $_.Groups[1].Value })
$volumes = @([regex]::Matches($accepted.Text, '(?m)^\s+volume:\s+(\S+) ->') | ForEach-Object { $_.Groups[1].Value })
Assert-Case -Name 'example plans two distinct container prefixes' `
    -Passed ($prefixes.Count -eq 2 -and @($prefixes | Select-Object -Unique).Count -eq 2) -Detail "prefixes: $($prefixes -join ', ')"
Assert-Case -Name 'example plans distinct volume names' `
    -Passed ($volumes.Count -gt 0 -and @($volumes | Select-Object -Unique).Count -eq $volumes.Count) -Detail "volumes: $($volumes -join ', ')"
Assert-Case -Name 'example registers the default labels and the custom label' `
    -Passed ($accepted.Text.Contains('labels:            self-hosted, linux, x64, example-label-one')) -Detail $accepted.Text

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
