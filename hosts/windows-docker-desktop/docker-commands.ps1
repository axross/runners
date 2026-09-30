#Requires -Version 5.1
<#
.SYNOPSIS
    runs the docker command line client and reports its exit code, dot-sourced
    by the supervisor and by each slot's background job.

.DESCRIPTION
    Windows PowerShell 5.1 turns a native command's stderr output into a
    terminating error under $ErrorActionPreference = 'Stop' once it is
    redirected, so both functions relax the preference while docker runs and
    leave the caller to check the returned exit code.
#>

# runs docker, returns its exit code and its output with stderr merged in.
function Invoke-Docker {
    param([Parameter(Mandatory)][string[]]$Arguments)

    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = @(& docker @Arguments 2>&1 | ForEach-Object { "$_" })
        return [pscustomobject]@{ ExitCode = $LASTEXITCODE; Output = $output }
    } finally {
        $ErrorActionPreference = $previous
    }
}

# runs docker and writes each output line to the information stream as it
# arrives, so a job's log never reaches the output stream the supervisor parses.
function Invoke-DockerLogged {
    param([Parameter(Mandatory)][string[]]$Arguments)

    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & docker @Arguments 2>&1 | ForEach-Object { Write-Information "$_" }
        return $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
}
