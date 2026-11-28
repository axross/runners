#Requires -Version 7.0
<#
.SYNOPSIS
    tests the removal check of cpu-affinity-smoke.ps1 against a stand-in docker.

.DESCRIPTION
    needs no Docker daemon or image. loads Confirm-CpuSmokeRemoval from the smoke
    script through PowerShell's parser, because running that script would run the
    whole smoke test, and puts a stand-in docker first on PATH. the stand-in
    answers each docker ps call from a scripted list of exit code and output, and
    records the calls. needs bash, so it runs on Linux and macOS, not Windows.
    asserts outcomes, messages, the logged wait and call counts; the only timing
    assertion is a lower bound, so a loaded host cannot make it fail.
#>
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'smoke-docker-commands.ps1')

$smokeScript = Join-Path $PSScriptRoot 'cpu-affinity-smoke.ps1'
$parseErrors = $null
$tree = [Management.Automation.Language.Parser]::ParseFile($smokeScript, [ref]$null, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw "$smokeScript does not parse: $($parseErrors[0].Message)" }
$definition = $tree.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Confirm-CpuSmokeRemoval' }, $true)
if ($null -eq $definition) { throw "Confirm-CpuSmokeRemoval is not defined in $smokeScript" }
. ([scriptblock]::Create($definition.Extent.Text))

$standIn = @'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$STAND_IN_DIR/calls"
calls=$(( $(wc -l < "$STAND_IN_DIR/calls") ))
total=$(( $(wc -l < "$STAND_IN_DIR/script") ))
if (( calls > total )); then calls=$total; fi
entry=$(sed -n "${calls}p" "$STAND_IN_DIR/script")
output="${entry#*:}"
if [ -n "$output" ]; then printf '%b\n' "$output"; fi
exit "${entry%%:*}"
'@

$name = 'cpu-smoke-test-container'
$psArguments = "ps -a --filter name=^$name$ --format {{.Names}}|{{.State}}"
$script:Failures = 0
$script:Cases = 0

# runs the check once against a scripted docker and compares what it did with what the case expects.
function Test-Case {
    param(
        [string]$Title,
        [string[]]$Script,
        [bool]$Diagnostic,
        [bool]$ExpectPass,
        [int]$ExpectCalls = -1,
        [int]$ExpectAtLeastCalls = 0,
        [int]$BoundMilliseconds = 10000,
        [string[]]$ExpectMessage = @(),
        [bool]$ExpectWaitLog = $false,
        [int]$MinimumMilliseconds = 0
    )

    $script:Cases++
    $problems = New-Object System.Collections.Generic.List[string]
    $directory = Join-Path ([IO.Path]::GetTempPath()) ("cpu-smoke-removal-" + [Guid]::NewGuid().ToString('N'))
    $previousPath = $env:PATH
    $previousDirectory = $env:STAND_IN_DIR
    try {
        $null = New-Item -ItemType Directory -Path "$directory/bin"
        Set-Content -LiteralPath "$directory/bin/docker" -Value $standIn
        & chmod +x "$directory/bin/docker"
        if ($LASTEXITCODE -ne 0) { throw "chmod of the stand-in docker failed with exit code $LASTEXITCODE" }
        Set-Content -LiteralPath "$directory/script" -Value $Script
        Set-Content -LiteralPath "$directory/calls" -Value '' -NoNewline
        $env:STAND_IN_DIR = $directory
        $env:PATH = "$directory/bin" + [IO.Path]::PathSeparator + $previousPath

        $passed = $false
        $message = ''
        $log = @()
        $clock = [Diagnostics.Stopwatch]::StartNew()
        try {
            $log = @(Confirm-CpuSmokeRemoval -Name $name -Diagnostic $Diagnostic -BoundMilliseconds $BoundMilliseconds 6>&1 | ForEach-Object { "$_" })
            $passed = $true
        } catch {
            $message = $_.Exception.Message
        }
        $elapsed = $clock.ElapsedMilliseconds

        $calls = @(Get-Content -LiteralPath "$directory/calls")
        if ($passed -ne $ExpectPass) { $problems.Add("expected pass=$ExpectPass, got pass=$passed (message: '$message')") }
        if ($ExpectCalls -ge 0 -and $calls.Count -ne $ExpectCalls) { $problems.Add("expected $ExpectCalls docker calls, got $($calls.Count)") }
        if ($calls.Count -lt $ExpectAtLeastCalls) { $problems.Add("expected at least $ExpectAtLeastCalls docker calls, got $($calls.Count)") }
        foreach ($call in $calls) {
            if ($call -cne $psArguments) { $problems.Add("unexpected docker call '$call'") }
        }
        foreach ($fragment in $ExpectMessage) {
            if (-not $message.Contains($fragment)) { $problems.Add("message '$message' lacks '$fragment'") }
        }
        $waitLogged = @($log | Where-Object { $_ -cmatch "^CPU smoke: $name waited [0-9]+ ms" }).Count -eq 1
        if ($ExpectWaitLog -ne $waitLogged) { $problems.Add("expected wait logged=$ExpectWaitLog, got $waitLogged (log: '$($log -join ' / ')')") }
        if ($elapsed -lt $MinimumMilliseconds) { $problems.Add("returned after $elapsed ms, before the $MinimumMilliseconds ms bound") }
    } finally {
        $env:PATH = $previousPath
        $env:STAND_IN_DIR = $previousDirectory
        Remove-Item -LiteralPath $directory -Recurse -Force -ErrorAction SilentlyContinue
    }

    if ($problems.Count -eq 0) {
        Write-Output "ok   - $Title"
    } else {
        $script:Failures++
        Write-Output "FAIL - $Title"
        foreach ($problem in $problems) { Write-Output "         $problem" }
    }
}

$gone = '0:'
$listed = "0:$name|exited"

Test-Case -Title 'ordinary container gone at the first check passes after one call and logs the wait' `
    -Script @($gone) -Diagnostic $false -ExpectPass $true -ExpectCalls 1 -ExpectWaitLog $true
Test-Case -Title 'ordinary container listed for 3 checks and then gone passes after 4 calls and logs the wait' `
    -Script @($listed, $listed, $listed, $gone) -Diagnostic $false -ExpectPass $true -ExpectCalls 4 -ExpectWaitLog $true
Test-Case -Title 'ordinary container always listed fails after the bound with its name and last state' `
    -Script @($listed) -Diagnostic $false -ExpectPass $false -ExpectAtLeastCalls 2 -BoundMilliseconds 1000 -MinimumMilliseconds 1000 `
    -ExpectMessage @($name, 'not removed within 1 s', 'state exited')
Test-Case -Title 'diagnostic container listed passes after one call' `
    -Script @($listed) -Diagnostic $true -ExpectPass $true -ExpectCalls 1
Test-Case -Title 'diagnostic container not listed fails at once with the removed message' `
    -Script @($gone) -Diagnostic $true -ExpectPass $false -ExpectCalls 1 -ExpectMessage @($name, 'was removed')
Test-Case -Title 'docker ps failing at the first check fails at once with the exit code' `
    -Script @('3:') -Diagnostic $false -ExpectPass $false -ExpectCalls 1 -ExpectMessage @($name, 'exit code 3')
Test-Case -Title 'docker ps failing at the second check, after a listed result, fails at once with the exit code' `
    -Script @($listed, '3:') -Diagnostic $false -ExpectPass $false -ExpectCalls 2 -ExpectMessage @($name, 'exit code 3')
Test-Case -Title 'docker ps failing for a diagnostic container fails at once with the exit code' `
    -Script @('3:') -Diagnostic $true -ExpectPass $false -ExpectCalls 1 -ExpectMessage @($name, 'exit code 3')
Test-Case -Title 'output for another container fails at once and quotes the output' `
    -Script @('0:other-container|exited') -Diagnostic $false -ExpectPass $false -ExpectCalls 1 -ExpectMessage @($name, "'other-container|exited'")
Test-Case -Title 'output for a longer name that starts with this one fails at once' `
    -Script @("0:$name-2|exited") -Diagnostic $false -ExpectPass $false -ExpectCalls 1 -ExpectMessage @($name, "'$name-2|exited'")
Test-Case -Title 'an extra line fails at once and quotes the output' `
    -Script @("0:$name|exited\nother-container|exited") -Diagnostic $false -ExpectPass $false -ExpectCalls 1 -ExpectMessage @($name, "other-container|exited'")
Test-Case -Title 'output for another container fails a diagnostic container at once' `
    -Script @('0:other-container|exited') -Diagnostic $true -ExpectPass $false -ExpectCalls 1 -ExpectMessage @($name, "'other-container|exited'")

if ($script:Failures -gt 0) {
    Write-Output "$($script:Failures) of $($script:Cases) cases failed."
    exit 1
}
Write-Output "All $($script:Cases) removal check cases passed."
