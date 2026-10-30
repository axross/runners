#Requires -Version 5.1
<#
.SYNOPSIS
    builds the runner image from the repository checkout this script runs from,
    under the image name the host configuration gives.

.DESCRIPTION
    builds images/actions-runner as the checkout holds it, ignoring the layer
    cache, and afterwards removes only the images this script built earlier and
    left untagged; see docs/operations/windows-runner-host.md. a failed build
    leaves the previous image under its name.
#>
param(
    [Parameter(Mandatory)][string]$ConfigPath
)

$ErrorActionPreference = 'Stop'
$InformationPreference = 'Continue'

. (Join-Path $PSScriptRoot 'host-configuration.ps1')

# marks the images this script builds, so the prune below touches only those.
$BuildLabel = 'com.github.axross.runners.image=actions-runner'

$plan = Read-HostConfiguration -Path $ConfigPath
$imageDirectory = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../images/actions-runner')).Path

Write-Information "Building $($plan.ImageName) from $imageDirectory without the layer cache..."
& docker build --pull --no-cache --label $BuildLabel --tag $plan.ImageName $imageDirectory
if ($LASTEXITCODE -ne 0) {
    throw "docker build failed with exit code $LASTEXITCODE."
}

Write-Information "Built $($plan.ImageName)."

# the build is done and the new image is in place, so a prune that fails is
# reported without failing the task.
& docker image prune --force --filter "label=$BuildLabel"
if ($LASTEXITCODE -ne 0) {
    Write-Warning "docker image prune exited with code $LASTEXITCODE; earlier runner images may remain on disk."
}
