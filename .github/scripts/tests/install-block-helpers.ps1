# helpers that the Windows install block tests dot-source: extracting a
# documented block from docs/operations/cli.md, replacing its assignment lines,
# and the stand-in for gh. written for Windows PowerShell 5.1 and 7.

# ends the script with exit code 2 and one line when jq is not on PATH, because the
# gh stand-in applies the blocks' --jq expressions with it.
function Assert-JqAvailable {
    if ($null -eq (Get-Command -Name jq -CommandType Application -ErrorAction SilentlyContinue)) {
        [Console]::Error.WriteLine('error: jq is required on PATH for the gh stand-in; install it (for example winget install jqlang.jq, apt install jq or brew install jq).')
        exit 2
    }
}

function Test-RunningOnWindows {
    return [Environment]::OSVersion.Platform -eq 'Win32NT'
}

function Get-Sha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

# returns the code of the fenced block that follows the marker comment
# `<!-- install-block: <id> -->`, and throws unless the marker occurs exactly once
# and a non-empty fence follows it.
function Get-InstallBlock {
    param([Parameter(Mandatory = $true)][string]$Document, [Parameter(Mandatory = $true)][string]$Id)
    $marker = "<!-- install-block: $Id -->"
    $state = 'outside'
    $markers = 0
    $fences = 0
    $code = New-Object System.Collections.Generic.List[string]
    foreach ($line in (Get-Content -LiteralPath $Document)) {
        if ($state -eq 'outside') {
            if ($line -eq $marker) { $markers++; $state = 'marked' }
        } elseif ($state -eq 'marked') {
            if ($line.Trim() -eq '') { continue }
            if ($line.StartsWith('```')) { $state = 'inside' } else { $state = 'outside' }
        } elseif ($line.StartsWith('```')) {
            $fences++
            $state = 'outside'
        } else {
            $code.Add($line)
        }
    }
    if ($markers -ne 1 -or $fences -ne 1 -or $code.Count -eq 0) {
        throw "$Document does not hold exactly one non-empty fenced block after the marker for '$Id'."
    }
    return ($code.ToArray() -join "`n")
}

# replaces the one line that starts with the prefix (after indentation) by the new
# line, and throws unless exactly one line does.
function Edit-BlockLine {
    param(
        [Parameter(Mandatory = $true)][string]$Block,
        [Parameter(Mandatory = $true)][string]$Prefix,
        [Parameter(Mandatory = $true)][string]$Line
    )
    $replaced = 0
    $lines = foreach ($existing in ($Block -split "`n")) {
        if ($existing.TrimStart().StartsWith($Prefix)) { $replaced++; "  $Line" } else { $existing }
    }
    if ($replaced -ne 1) { throw "The block holds $replaced lines that start with '$Prefix', not 1." }
    return ($lines -join "`n")
}

function Assert-NoCommentLine {
    param([Parameter(Mandatory = $true)][string]$Id, [Parameter(Mandatory = $true)][string]$Block)
    if ($Block -match '(?m)^\s*#') {
        throw "The block $Id has a comment line."
    }
}

# the stand-in for the gh calls the Windows blocks make, which the install block
# tests share. it reads $script:Scenario (Release, DownloadFail, TagFail, TagType,
# TagSha, AnnotatedFail, AnnotatedSha, CompareFail, Compare, AttestFail), records
# each call in $script:Calls, and sets $LASTEXITCODE as the real gh does. an API
# read has a canned response body, and the caller's --jq expression is applied to
# it with jq, as gh does, so a block with a wrong expression gets a wrong value.
function Write-ExitCode {
    param([int]$Code)
    $global:LASTEXITCODE = $Code
}

# returns the body as JSON, or what jq makes of it under the --jq expression.
function Get-StandInApiResponse {
    param([Parameter(Mandatory = $true)]$Body, [string]$JqExpression)
    $json = $Body | ConvertTo-Json -Compress -Depth 5
    if (-not $JqExpression) { return $json }
    if ($null -eq (Get-Command -Name jq -CommandType Application -ErrorAction SilentlyContinue)) {
        throw 'jq is not installed, and the gh stand-in applies --jq with it.'
    }
    $result = $json | & jq -r $JqExpression
    if ($LASTEXITCODE -ne 0) { Write-ExitCode 1; return }
    return $result
}

function gh {
    $script:Calls.Add('gh ' + ($args -join ' '))
    $scenario = $script:Scenario
    Write-ExitCode 0
    if ($args[0] -eq 'release' -and $args[1] -eq 'download') {
        $patterns = @()
        $directory = '.'
        for ($index = 2; $index -lt $args.Count; $index++) {
            if ($args[$index] -eq '--pattern') { $patterns += [string]$args[$index + 1]; $index++ }
            elseif ($args[$index] -eq '--dir') { $directory = [string]$args[$index + 1]; $index++ }
        }
        if ($scenario.DownloadFail) { Write-ExitCode 1; return }
        foreach ($pattern in $patterns) {
            $source = Join-Path $scenario.Release $pattern
            if (-not (Test-Path -LiteralPath $source)) { Write-ExitCode 1; return }
            Copy-Item -LiteralPath $source -Destination (Join-Path $directory $pattern)
        }
        return
    }
    if ($args[0] -eq 'api') {
        $uri = [string]$args[1]
        $jqExpression = ''
        for ($index = 2; $index -lt $args.Count - 1; $index++) {
            if ($args[$index] -eq '--jq') { $jqExpression = [string]$args[$index + 1] }
        }
        if ($uri -like '*/git/ref/tags/*') {
            if ($scenario.TagFail) { Write-ExitCode 1; return }
            return (Get-StandInApiResponse -Body @{ ref = 'refs/tags/v0'; object = @{ type = $scenario.TagType; sha = $scenario.TagSha } } -JqExpression $jqExpression)
        }
        if ($uri -like '*/git/tags/*') {
            if ($scenario.AnnotatedFail) { Write-ExitCode 1; return }
            return (Get-StandInApiResponse -Body @{ tag = 'v0'; object = @{ type = 'commit'; sha = $scenario.AnnotatedSha } } -JqExpression $jqExpression)
        }
        if ($uri -like '*/compare/*') {
            if ($scenario.CompareFail) { Write-ExitCode 1; return }
            return (Get-StandInApiResponse -Body @{ status = $scenario.Compare; ahead_by = 0; behind_by = 0 } -JqExpression $jqExpression)
        }
    }
    if ($args[0] -eq 'attestation' -and $args[1] -eq 'verify') {
        if ($scenario.AttestFail) { Write-ExitCode 1 }
        return
    }
    Write-ExitCode 99
}

