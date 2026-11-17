# fails unless the product version in the version resource of <Executable> is
# <Version>, the version the release build stamped. release-build.yaml runs it on
# the built marshal-app.exe, in dry runs too, and once more with a version that
# differs, which must fail. docs/operations/cli.md describes it.
param(
  [Parameter(Mandatory = $true)][string]$Executable,
  [Parameter(Mandatory = $true)][string]$Version
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) { throw "$Executable does not exist." }
$found = [string][Diagnostics.FileVersionInfo]::GetVersionInfo((Resolve-Path -LiteralPath $Executable).Path).ProductVersion
if ($found -cne $Version) {
  throw "The product version of $Executable is '$found', not the stamped '$Version'."
}
Write-Output "The product version of $Executable is $found."
