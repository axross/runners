#Requires -Version 5.1
<#
.SYNOPSIS
    measures a standard-user CLI install without starting resident runner pools.
#>
param(
    [Parameter(Mandatory = $true)][string]$ReleaseDirectory,
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Document = (Join-Path (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))) 'docs/runner-recovery/install.md')
)

$ErrorActionPreference = 'Stop'
& (Join-Path (Split-Path -Parent $PSScriptRoot) 'tests/test-msi-standard-user.ps1') -ReleaseDirectory $ReleaseDirectory -Version $Version `
    -Document $Document -InstalledName 'marshal.exe' -LogSubdirectory 'cli-recovery-install-block-logs' -AssertInstalled {
        param([string]$Installed, [string]$Version)
        $printed = (& $Installed --version | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) { throw "the installed binary exited $LASTEXITCODE" }
        if ($printed -ne "marshal $Version") { throw "the installed binary printed '$printed'" }
        "installed binary: $printed"
    }
exit $LASTEXITCODE
