# builds the per-user MSI for Marshal from marshal.wxs with the WiX that
# fetch-wix.sh installed, and writes it to <Output>. <App> is the app executable,
# installed unchanged as marshal-app.exe, and <Notices> the app's licence notices
# for Windows, installed as THIRD-PARTY-NOTICES.txt. <Version> is the installer's
# own X.Y.Z as installer-version.sh prints it, which is where the MSI limits are
# enforced. docs/operations/cli.md describes the installer.
param(
  [Parameter(Mandatory = $true)][string]$App,
  [Parameter(Mandatory = $true)][string]$Notices,
  [Parameter(Mandatory = $true)][string]$Version,
  [Parameter(Mandatory = $true)][string]$Output,
  [Parameter(Mandatory = $true)][string]$WixDirectory
)

$ErrorActionPreference = 'Stop'

if ($Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
  throw "The installer version '$Version' is not X.Y.Z; map a release version with installer-version.sh first."
}
$wix = Join-Path $WixDirectory 'wix.exe'
foreach ($required in @($App, $Notices, $wix)) {
  if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "$required does not exist." }
}
if (Test-Path -LiteralPath $Output) { throw "$Output already exists." }

$source = Join-Path $PSScriptRoot 'marshal.wxs'
# -acceptEula names the major version of the OSMF EULA that the maintainer accepted.
# -pdbtype none stops wix build from writing a .wixpdb beside the output.
& $wix build -acceptEula wix7 -arch x64 -pdbtype none -d "MarshalApp=$App" -d "MarshalNotices=$Notices" -d "MarshalVersion=$Version" -o $Output $source
if ($LASTEXITCODE -ne 0) { throw "wix build exited $LASTEXITCODE." }
if (-not (Test-Path -LiteralPath $Output -PathType Leaf)) { throw "wix build wrote no $Output." }
Write-Output "Built $Output at version $Version."
