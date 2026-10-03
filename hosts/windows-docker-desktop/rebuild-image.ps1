#Requires -Version 5.1
<#
.SYNOPSIS
    builds the runner image from the repository checkout this script runs from,
    under the image name the host configuration gives.

.DESCRIPTION
    builds images/actions-runner exactly as the checkout holds it, so the runner
    version and base image digest come from its Dockerfile. it fetches nothing
    else and never updates the checkout; pull a newer checkout by hand first to
    move to a newer runner. the build ignores the layer cache, so the operating
    system packages are installed again and pick up their current updates. a
    failed build leaves the previous image under its name.

    after a successful build it removes the final images this script built
    earlier and that the new build left untagged. build caches and tagged
    recovery images remain. only images carrying this script's own build label
    are removed; it does not prune other dangling images, and an image built by
    hand or before this label existed is left alone. the separate build-cache
    reclamation procedure is in docs/operations/windows-runner-host.md.

    a container already running finishes its job on the image it started with;
    the next container a slot starts uses the rebuilt one.

.PARAMETER ConfigPath
    path to the host configuration JSON file. see runner-host.example.json.
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
