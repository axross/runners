#Requires -Version 5.1
<#
.SYNOPSIS
    runs the Docker command line client and reports its exit code, and builds
    the job container's arguments, its name and the pattern that recognizes it.
    dot-sourced by the supervisor and by each slot's background job, so it
    depends on nothing from host-configuration.ps1.

.DESCRIPTION
    Windows PowerShell 5.1 turns a native command's stderr output into a
    terminating error under $ErrorActionPreference = 'Stop' once it is
    redirected, so both functions that run Docker relax the preference while it
    runs and leave the caller to check the returned exit code.
#>

# runs Docker, returns its exit code and its output with stderr merged in.
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

# runs Docker and writes each output line to the information stream as it
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

# returns the arguments of the one `docker run` that starts a job container.
# -Cpus is the text Format-CpuCount returns. memory and swap are both
# -MemoryGb, so the container gets no swap beyond its memory limit. `-e NAME`
# without a value makes the Docker client read the registration from its own
# environment, so it is never on a command line.
function Get-JobContainerArgument {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Cpus,
        [Parameter(Mandatory)][int]$MemoryGb,
        [string[]]$Mounts = @(),
        [Parameter(Mandatory)][string]$JitConfigVariable,
        [Parameter(Mandatory)][string]$ImageName,
        [Parameter(Mandatory)][string]$RunCommand,
        [bool]$Diagnostics = $false,
        [bool]$DiagnosticRawRecords = $false
    )

    if ($Diagnostics) {
        $mode = ''
        if ($DiagnosticRawRecords) { $mode = ' raw-run' }
        $launcher = 'if [ -x /usr/local/bin/runner-diagnostics ]; then exec /usr/local/bin/runner-diagnostics' + $mode +
            '; else printf "Diagnostic gap: collector missing.\n" >&2; exec /home/runner/run.sh; fi'
        return @('run', '--pull', 'never', '--name', $Name,
            '--cpus', $Cpus, '--memory', "${MemoryGb}g", '--memory-swap', "${MemoryGb}g") + @($Mounts) +
            @('-e', $JitConfigVariable, $ImageName, 'bash', '-c', $launcher)
    }
    return @('run', '--rm', '--pull', 'never', '--name', $Name,
        '--cpus', $Cpus, '--memory', "${MemoryGb}g", '--memory-swap', "${MemoryGb}g") + @($Mounts) +
        @('-e', $JitConfigVariable, $ImageName, $RunCommand)
}

# returns the name of a slot's next job container, which is also its GitHub
# runner name: the entry's name, the 1-based slot index and a 17-digit
# timestamp. the invariant culture keeps the digits ASCII under any calendar.
function Get-JobContainerName {
    param(
        [Parameter(Mandatory)][string]$EntryName,
        [Parameter(Mandatory)][int]$Slot,
        [datetime]$Now = (Get-Date)
    )

    return "$EntryName-$Slot-$($Now.ToString('yyyyMMddHHmmssfff', [System.Globalization.CultureInfo]::InvariantCulture))"
}

# returns the pattern that matches only the names Get-JobContainerName gives
# this entry's containers, so a container of another entry whose name starts
# with this one is not matched. \z rather than $ keeps a trailing newline out.
function Get-JobContainerNamePattern {
    param([Parameter(Mandatory)][string]$Name)

    return '^' + [regex]::Escape($Name) + '-\d+-\d{17}\z'
}
