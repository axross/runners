#Requires -Version 7.0
<#
.SYNOPSIS
    checks that the Rust validator accepts and rejects what the PowerShell
    validator does, with the same ordered errors and summaries.

.DESCRIPTION
    for every shared fixture in expectations.json, including the ones under
    shared/pwsh7/, this runs both validators and fails when their outcomes differ,
    when their error lists or summaries differ after the configuration path is
    normalized, or when an expected string is missing or an unwanted one is
    present in either output. it does the same for the axpc example configuration,
    and for a copy of it with a UTF-8 byte-order mark that it builds at run time,
    because a formatter or editor would strip a stored mark.

    it also runs only the PowerShell validator over the declared-divergence
    fixtures under rust/raw/ and fails when its outcome differs from the one
    recorded for it. its wording is not compared, and a fixture recorded as
    skipped is not run, because PowerShell cannot run it or its result depends on
    the platform's globalization data.

    before it compares anything, a self-test feeds the harness a deliberately wrong
    expectation and a stand-in validator that drops an error, and requires both to
    fail, so a harness that stopped comparing would be noticed.

    runs under PowerShell 7 only: it dot-sources the PowerShell validator, which is
    what the supervisor's -ValidateOnly mode runs, and the behaviour of Windows
    PowerShell 5.1 on a byte-order mark is not probed.
#>
param(
    [string]$Binary = (Join-Path $PSScriptRoot '../../cli/target/debug/runners')
)

$ErrorActionPreference = 'Stop'

$root = $PSScriptRoot
$repositoryRoot = Split-Path -Parent (Split-Path -Parent $root)
$hostDirectory = Join-Path $repositoryRoot 'hosts/windows-docker-desktop'
$example = Join-Path $hostDirectory 'runner-host.example.json'
if (-not (Test-Path -LiteralPath $Binary) -and (Test-Path -LiteralPath "$Binary.exe")) {
    $Binary = "$Binary.exe"
}
if (-not (Test-Path -LiteralPath $Binary)) {
    throw "The Rust binary $Binary does not exist; build it with mise run build:rust."
}

. (Join-Path $hostDirectory 'host-configuration.ps1')

# the PowerShell validator, run the way the supervisor's -ValidateOnly mode runs it.
# returns the outcome and the text the supervisor prints: the summary on success, the
# error message on failure.
function Invoke-PowerShellValidator {
    param([Parameter(Mandatory)][string]$Path)

    try {
        $plan = Read-HostConfiguration -Path $Path
    } catch {
        return [pscustomobject]@{ Outcome = 'reject'; Text = $_.Exception.Message }
    }
    return [pscustomobject]@{ Outcome = 'accept'; Text = (@(Get-PlanSummary -Plan $plan) -join "`n") }
}

# the Rust validator, run as the binary. exit 0 accepts and prints the summary on
# standard output; exit 1 rejects and prints the errors on standard error.
function Invoke-RustValidator {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$HostProfile)

    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = $Binary
    foreach ($argument in @('validate', '--config', $Path, '--profile', $HostProfile)) {
        $start.ArgumentList.Add($argument)
    }
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.Encoding]::UTF8
    $start.StandardErrorEncoding = [Text.Encoding]::UTF8
    $start.UseShellExecute = $false
    $process = [System.Diagnostics.Process]::Start($start)
    $output = $process.StandardOutput.ReadToEndAsync()
    $errors = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $text = $errors.Result
    if ($process.ExitCode -eq 0) { $text = $output.Result }
    $outcome = 'error'
    if ($process.ExitCode -eq 0) { $outcome = 'accept' }
    if ($process.ExitCode -eq 1) { $outcome = 'reject' }
    # a note about the checkout goes to standard error beside a summary, and is not
    # part of what the PowerShell validator prints.
    $lines = @($text -split "`r?`n" | Where-Object { -not $_.StartsWith('note: ') })
    return [pscustomobject]@{ Outcome = $outcome; Text = ($lines -join "`n").TrimEnd("`n") }
}

# compares both validators on one configuration and returns the problems found. the
# Rust validator is a parameter so the self-test can stand a faulty one in.
function Test-Parity {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)]$Expectation,
        [Parameter(Mandatory)][scriptblock]$RustValidator
    )

    $problems = New-Object System.Collections.Generic.List[string]
    $powershell = Invoke-PowerShellValidator -Path $Path
    $rust = & $RustValidator $Path $Expectation.profile
    $powershellText = $powershell.Text.Replace($Path, '<config>')
    $rustText = $rust.Text.Replace($Path, '<config>')
    if ($powershell.Outcome -cne $Expectation.outcome) {
        $problems.Add("$Name - PowerShell gave $($powershell.Outcome), the expectation says $($Expectation.outcome)")
    }
    if ($rust.Outcome -cne $powershell.Outcome) {
        $problems.Add("$Name - the validators disagree: PowerShell $($powershell.Outcome), Rust $($rust.Outcome)")
    } elseif ($rustText -cne $powershellText) {
        $problems.Add("$Name - the output differs`n--- PowerShell`n$powershellText`n--- Rust`n$rustText")
    }
    foreach ($validator in @(@('PowerShell', $powershellText), @('Rust', $rustText))) {
        foreach ($wanted in @($Expectation.contains)) {
            if (-not $validator[1].Contains($wanted)) {
                $problems.Add("$Name - the $($validator[0]) output lacks: $wanted")
            }
        }
        foreach ($unwanted in @($Expectation.lacks)) {
            if ($validator[1].Contains($unwanted)) {
                $problems.Add("$Name - the $($validator[0]) output has: $unwanted")
            }
        }
    }
    return $problems.ToArray()
}

# runs every shared fixture of an expectations table found under -Root, and returns
# the problems found and one report line per fixture. -Root holds expectations.json
# and the shared/ directory.
function Invoke-Parity {
    param(
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)][scriptblock]$RustValidator
    )

    $problems = New-Object System.Collections.Generic.List[string]
    $report = New-Object System.Collections.Generic.List[string]
    $expectations = Get-Content -LiteralPath (Join-Path $Root 'expectations.json') -Raw | ConvertFrom-Json
    $shared = @($expectations.PSObject.Properties | Where-Object { $_.Name.StartsWith('shared/') })
    $files = @(Get-ChildItem -LiteralPath (Join-Path $Root 'shared') -Recurse -File -Filter '*.json' |
            ForEach-Object { $_.FullName.Substring($Root.Length + 1).Replace('\', '/') })
    foreach ($name in $files) {
        if ($expectations.PSObject.Properties.Name -notcontains $name) {
            $problems.Add("$name - the fixture has no expectation")
        }
    }
    foreach ($property in $shared) {
        if ($files -notcontains $property.Name) {
            $problems.Add("$($property.Name) - the expectation has no fixture")
            continue
        }
        $found = @(Test-Parity -Path (Join-Path $Root $property.Name) -Name $property.Name -Expectation $property.Value -RustValidator $RustValidator)
        foreach ($problem in $found) { $problems.Add($problem) }
        if ($found.Count -eq 0) { $report.Add("PASS  $($property.Name)") } else { $report.Add("FAIL  $($property.Name)") }
    }
    return [pscustomobject]@{ Problems = $problems.ToArray(); Report = $report.ToArray() }
}

$rustValidator = { param($Path, $HostProfile) Invoke-RustValidator -Path $Path -HostProfile $HostProfile }

# the self-test: the harness must fail on a wrong expectation and on a validator
# that drops an error. each runs on a copy of one real fixture with two errors.
function Test-Harness {
    $scratch = Join-Path ([IO.Path]::GetTempPath()) "parity-selftest-$PID"
    New-Item -ItemType Directory -Force -Path (Join-Path $scratch 'shared/rejected') | Out-Null
    try {
        $name = 'shared/rejected/integer-literals.json'
        Copy-Item -LiteralPath (Join-Path $root $name) -Destination (Join-Path $scratch $name)
        $real = (Get-Content -LiteralPath (Join-Path $root 'expectations.json') -Raw | ConvertFrom-Json).$name
        $write = {
            param($Expectation)
            $table = [ordered]@{}
            $table[$name] = $Expectation
            [IO.File]::WriteAllText((Join-Path $scratch 'expectations.json'), ($table | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding $false))
        }

        & $write $real
        $clean = @((Invoke-Parity -Root $scratch -RustValidator $rustValidator).Problems)

        $wrong = [pscustomobject]@{
            profile  = $real.profile
            outcome  = $real.outcome
            contains = @($real.contains) + @('text that neither validator prints')
            lacks    = @($real.lacks)
        }
        & $write $wrong
        $wrongExpectation = @((Invoke-Parity -Root $scratch -RustValidator $rustValidator).Problems)

        & $write $real
        $dropsAnError = {
            param($Path, $HostProfile)
            $result = Invoke-RustValidator -Path $Path -HostProfile $HostProfile
            $lines = @($result.Text -split "`n")
            return [pscustomobject]@{ Outcome = $result.Outcome; Text = ($lines[0..($lines.Count - 2)] -join "`n") }
        }
        $droppedError = @((Invoke-Parity -Root $scratch -RustValidator $dropsAnError).Problems)
    } finally {
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
    $faults = New-Object System.Collections.Generic.List[string]
    if ($clean.Count -ne 0) { $faults.Add("the harness fails on a correct fixture: $($clean -join '; ')") }
    if ($wrongExpectation.Count -eq 0) { $faults.Add('the harness passes a deliberately wrong expectation') }
    if ($droppedError.Count -eq 0) { $faults.Add('the harness passes a validator that drops an error') }
    return $faults.ToArray()
}

$faults = @(Test-Harness)
if ($faults.Count -gt 0) {
    $faults | ForEach-Object { Write-Output "FAIL  self-test - $_" }
    exit 1
}
Write-Output 'PASS  self-test: the harness fails on a wrong expectation and on a validator that drops an error'

$problems = New-Object System.Collections.Generic.List[string]
$shared = Invoke-Parity -Root $root -RustValidator $rustValidator
$shared.Report | ForEach-Object { Write-Output $_ }
foreach ($problem in $shared.Problems) { $problems.Add($problem) }

# the example configuration, and the same file with a byte-order mark.
$exampleExpectation = [pscustomobject]@{
    profile  = 'axpc'
    outcome  = 'accept'
    contains = @('labels:            self-hosted, linux, x64, axpc, example-label-one')
    lacks    = @()
}
$found = @(Test-Parity -Path $example -Name 'the axpc example configuration' -Expectation $exampleExpectation -RustValidator $rustValidator)
foreach ($problem in $found) { $problems.Add($problem) }
if ($found.Count -eq 0) { Write-Output 'PASS  the axpc example configuration' } else { Write-Output 'FAIL  the axpc example configuration' }

$scratch = Join-Path ([IO.Path]::GetTempPath()) "parity-bom-$PID"
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
try {
    $withMark = Join-Path $scratch 'runner-host.example.json'
    $bytes = [IO.File]::ReadAllBytes($example)
    [IO.File]::WriteAllBytes($withMark, ([byte[]](0xEF, 0xBB, 0xBF) + $bytes))
    $found = @(Test-Parity -Path $withMark -Name 'the axpc example configuration with a byte-order mark' -Expectation $exampleExpectation -RustValidator $rustValidator)
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
foreach ($problem in $found) { $problems.Add($problem) }
if ($found.Count -eq 0) { Write-Output 'PASS  the axpc example configuration with a byte-order mark' } else { Write-Output 'FAIL  the axpc example configuration with a byte-order mark' }

# the declared divergences: only PowerShell's outcome is checked, not its wording.
$expectations = Get-Content -LiteralPath (Join-Path $root 'expectations.json') -Raw | ConvertFrom-Json
foreach ($property in @($expectations.PSObject.Properties | Where-Object { $_.Name.StartsWith('rust/raw/') })) {
    $recorded = $property.Value.powershell
    if ($recorded -ceq 'skip') {
        Write-Output "SKIP  $($property.Name) - PowerShell's result is not compared"
        continue
    }
    $actual = (Invoke-PowerShellValidator -Path (Join-Path $root $property.Name)).Outcome
    if ($actual -ceq $recorded) {
        Write-Output "PASS  $($property.Name) - PowerShell $actual"
    } else {
        Write-Output "FAIL  $($property.Name) - PowerShell gave $actual, the expectation records $recorded"
        $problems.Add("$($property.Name) - PowerShell gave $actual, the expectation records $recorded")
    }
}

if ($problems.Count -gt 0) {
    Write-Output ''
    $problems | ForEach-Object { Write-Output $_ }
    Write-Output "$($problems.Count) parity problem(s)."
    exit 1
}
Write-Output 'The validators agree on every shared fixture.'
