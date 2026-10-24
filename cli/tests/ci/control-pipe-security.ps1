# Reads back the security descriptor of a live control pipe and fails unless it
# grants the current user alone. The debug helper opens the channel in a
# debug-only root and prints the pipe's name; this script connects as a client and
# reads the descriptor from the pipe handle, which the CLI cannot do without
# unsafe code. CI runs it under PowerShell 7 on the Windows hosted runner.
param(
    [Parameter(Mandatory = $true)]
    [string] $Helper
)

$ErrorActionPreference = 'Stop'

$root = Join-Path ([System.IO.Path]::GetTempPath()) "runners-pipe-security-$PID"
New-Item -ItemType Directory -Path $root | Out-Null

$start = [System.Diagnostics.ProcessStartInfo]::new((Resolve-Path -LiteralPath $Helper).Path)
$start.ArgumentList.Add('serve')
$start.ArgumentList.Add('--root')
$start.ArgumentList.Add($root)
$start.UseShellExecute = $false
$start.RedirectStandardInput = $true
$start.RedirectStandardOutput = $true
$process = [System.Diagnostics.Process]::Start($start)
$client = $null
try {
    $name = $process.StandardOutput.ReadLine()
    $prefix = '\\.\pipe\'
    if (-not $name -or -not $name.StartsWith($prefix)) {
        throw "The helper did not open the control pipe; it printed: $name"
    }
    $user = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
    if (-not $name.StartsWith("${prefix}runners-control-$($user.Value)")) {
        throw "The control pipe $name is not named for the current user $($user.Value)."
    }
    Write-Output "Control pipe: $name"

    $client = [System.IO.Pipes.NamedPipeClientStream]::new(
        '.', $name.Substring($prefix.Length), [System.IO.Pipes.PipeDirection]::InOut)
    $client.Connect(5000)
    $security = [System.IO.Pipes.PipesAclExtensions]::GetAccessControl($client)
    Write-Output "Access control list: $($security.GetSecurityDescriptorSddlForm('Access'))"

    if (-not $security.AreAccessRulesProtected) {
        throw 'The access control list inherits entries, so others may be granted access.'
    }
    $rules = @($security.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
    if ($rules.Count -ne 1) {
        throw "The access control list has $($rules.Count) entries, not one for the current user."
    }
    $rule = $rules[0]
    if ($rule.AccessControlType -ne [System.Security.AccessControl.AccessControlType]::Allow) {
        throw 'The only entry does not allow access.'
    }
    if ($rule.IdentityReference.Value -ne $user.Value) {
        throw "The only entry is for $($rule.IdentityReference.Value), not the current user $($user.Value)."
    }
    # creating the pipe maps the generic GA right to the pipe's full access, so
    # the entry holds either
    $rights = [int64] $rule.PipeAccessRights
    $full = [int64] [System.IO.Pipes.PipeAccessRights]::FullControl
    $genericAll = [int64] 0x10000000
    if ((($rights -band $full) -ne $full) -and ($rights -ne $genericAll)) {
        throw ('The current user is granted 0x{0:X}, not full access.' -f $rights)
    }
    Write-Output 'The control pipe grants the current user alone.'
}
finally {
    if ($client) {
        $client.Dispose()
    }
    $process.StandardInput.Close()
    $ended = $process.WaitForExit(10000)
    if (-not $ended) {
        $process.Kill()
        $process.WaitForExit()
    }
    # the helper has exited either way, so its root can go before any check throws
    Remove-Item -LiteralPath $root -Recurse -Force
    if (-not $ended) {
        throw 'The helper did not end when its input closed.'
    }
    if ($process.ExitCode -ne 0) {
        throw "The helper exited with code $($process.ExitCode), not 0."
    }
}
