#Requires -Version 5.1
<#
.SYNOPSIS
    reads and validates a runner host configuration file, dot-sourced by the
    host scripts and the tests.

.DESCRIPTION
    Read-HostConfiguration turns the JSON file into a plan: the image name, and
    per target repository the registration labels, the name that starts every
    container, runner and volume name, the container CPU and memory limits, and
    the volume names. a configuration is rejected as a whole, with one message
    line per problem naming the offending field, before any container starts.
    Get-SlotWorkerArgument turns a plan entry into a slot job's arguments.
    nothing here calls Docker or GitHub, or reads a token file.
#>

$script:DefaultLabels = @('self-hosted', 'linux', 'x64', 'axpc')
$script:MaxSlots = 16
# GitHub documents no limit for a runner's name. the runner name is the entry's
# name plus a slot number and a 17-digit timestamp, so capping the entry name
# keeps every name short, on an assumed limit rather than a known one.
$script:MaxNameLength = 64
$script:DefaultCpus = 2
$script:MaxCpus = 64
$script:DefaultMemoryGb = 8
$script:MaxMemoryGb = 256

$script:HostFields = @('imageName', 'repositories')
$script:RepositoryFields = @('owner', 'repository', 'name', 'slots', 'tokenPath', 'labels', 'volumes', 'cpus', 'memoryGb', 'diagnostics', 'diagnosticDirectory', 'diagnosticRawRecords')
$script:VolumeFields = @('suffix', 'mountPath')

$script:NamePattern = '^[a-z0-9][a-z0-9-]*\z'
$script:OwnerPattern = '^[A-Za-z0-9][A-Za-z0-9-]*\z'
$script:RepositoryPattern = '^(?!\.{1,2}\z)[A-Za-z0-9_.-]+\z'
$script:ImageNamePattern = '^[A-Za-z0-9][A-Za-z0-9_.:/@-]*\z'
$script:LabelPattern = '^[A-Za-z0-9][A-Za-z0-9._:/-]*\z'
$script:MountPathPattern = '^/[A-Za-z0-9_./-]+\z'
# a drive letter and a backslash, or a UNC path. Path.IsPathRooted would also
# accept C:name and \name, which resolve against a working directory or drive
# the scheduled task does not control.
$script:TokenPathPattern = '^(?:[A-Za-z]:\\|\\\\)\S'

# returns the property's value, or $null after recording why it is unusable.
function Get-Field {
    param(
        [Parameter(Mandatory)]$Node,
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)]$Errors
    )

    $property = $Node.PSObject.Properties[$Name]
    if ($null -eq $property -or $null -eq $property.Value) {
        $Errors.Add("${Path}: required field is missing")
        return $null
    }
    return , $property.Value
}

# returns the string field if it matches -Pattern, or $null after recording why.
function Get-StringField {
    param(
        [Parameter(Mandatory)]$Node,
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)]$Errors,
        [Parameter(Mandatory)][string]$Pattern,
        [Parameter(Mandatory)][string]$Expectation
    )

    $value = Get-Field -Node $Node -Name $Name -Path $Path -Errors $Errors
    if ($null -eq $value) {
        return $null
    }
    if ($value -isnot [string] -or $value -cnotmatch $Pattern) {
        $Errors.Add("${Path}: must be $Expectation")
        return $null
    }
    return $value
}

# records an error for every property of the node that -Allowed does not list.
function Test-UnknownField {
    param(
        [Parameter(Mandatory)]$Node,
        [Parameter(Mandatory)][string[]]$Allowed,
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)]$Errors
    )

    foreach ($property in $Node.PSObject.Properties) {
        if ($Allowed -cnotcontains $property.Name) {
            $Errors.Add("$Path.$($property.Name): unknown field")
        }
    }
}

# returns whether the node is an object, recording an error when it is not.
function Test-IsObject {
    param($Node, [string]$Path, $Errors)

    if ($Node -is [System.Management.Automation.PSCustomObject]) {
        return $true
    }
    $Errors.Add("${Path}: must be an object")
    return $false
}

# returns the custom labels, an empty list when the field is absent or empty,
# or $null after recording why a present list is unusable.
function Get-CustomLabel {
    param([Parameter(Mandatory)]$Node, [Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Errors)

    $property = $Node.PSObject.Properties['labels']
    if ($null -eq $property) {
        return , [string[]]@()
    }
    $raw = $property.Value
    if ($raw -isnot [array]) {
        $Errors.Add("${Path}.labels: must be a list of custom labels")
        return $null
    }
    $items = @($raw)

    $valid = $true
    $seen = @{}
    foreach ($item in $items) {
        if ($item -isnot [string] -or $item -cnotmatch $script:LabelPattern) {
            $Errors.Add("${Path}.labels: each label must be a string of letters, digits and . _ : / - starting with a letter or digit")
            $valid = $false
        } elseif ($script:DefaultLabels -contains $item.ToLowerInvariant()) {
            $Errors.Add("${Path}.labels: '$item' is always added, so list only custom labels")
            $valid = $false
        } elseif ($seen.ContainsKey($item.ToLowerInvariant())) {
            $Errors.Add("${Path}.labels: duplicate label '$item'")
            $valid = $false
        }
        if ($item -is [string]) {
            $seen[$item.ToLowerInvariant()] = $true
        }
    }
    if (-not $valid) {
        return $null
    }
    return , [string[]]$items
}

# returns the cache volume definitions as suffix and mount path pairs, or $null.
function Get-VolumeDefinition {
    param([Parameter(Mandatory)]$Node, [Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Errors)

    $raw = Get-Field -Node $Node -Name 'volumes' -Path "$Path.volumes" -Errors $Errors
    if ($null -eq $raw) {
        return $null
    }

    if ($raw -isnot [array]) {
        $Errors.Add("${Path}.volumes: must be a list")
        return $null
    }
    $definitions = New-Object System.Collections.Generic.List[object]
    $suffixes = @{}
    $mountPaths = @{}
    $valid = $true
    $index = 0
    foreach ($item in @($raw)) {
        $itemPath = "$Path.volumes[$index]"
        $index++
        if (-not (Test-IsObject -Node $item -Path $itemPath -Errors $Errors)) {
            $valid = $false
            continue
        }
        Test-UnknownField -Node $item -Allowed $script:VolumeFields -Path $itemPath -Errors $Errors
        $before = $Errors.Count
        $suffix = Get-StringField -Node $item -Name 'suffix' -Path "$itemPath.suffix" -Errors $Errors `
            -Pattern $script:NamePattern -Expectation 'lowercase letters, digits and hyphens, starting with a letter or digit'
        $mountPath = Get-StringField -Node $item -Name 'mountPath' -Path "$itemPath.mountPath" -Errors $Errors `
            -Pattern $script:MountPathPattern -Expectation 'an absolute container path of letters, digits and . _ / -'
        if ($null -ne $mountPath -and $mountPath.Contains('..')) {
            $Errors.Add("$itemPath.mountPath: must not contain '..'")
            $mountPath = $null
        }
        if ($Errors.Count -gt $before -or $null -eq $suffix -or $null -eq $mountPath) {
            $valid = $false
            continue
        }
        if ($suffixes.ContainsKey($suffix)) {
            $Errors.Add("$itemPath.suffix: duplicate suffix '$suffix'")
            $valid = $false
        }
        if ($mountPaths.ContainsKey($mountPath)) {
            $Errors.Add("$itemPath.mountPath: duplicate mount path '$mountPath'")
            $valid = $false
        }
        $suffixes[$suffix] = $true
        $mountPaths[$mountPath] = $true
        $definitions.Add([pscustomobject]@{ Suffix = $suffix; MountPath = $mountPath })
    }
    if (-not $valid) {
        return $null
    }
    return , $definitions.ToArray()
}

# returns the entry's CPU limit as a double, the default when the field is
# absent, or $null after recording why the value is unusable. ConvertFrom-Json
# returns a fraction as a different numeric type depending on the PowerShell
# version, so every numeric type is accepted.
function Get-CpuLimit {
    param([Parameter(Mandatory)]$Node, [Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Errors)

    $property = $Node.PSObject.Properties['cpus']
    if ($null -eq $property) {
        return [double]$script:DefaultCpus
    }
    $value = $property.Value
    $isNumber = $value -is [int] -or $value -is [long] -or $value -is [double] -or $value -is [decimal]
    if (-not $isNumber -or $value -le 0 -or $value -gt $script:MaxCpus) {
        $Errors.Add("$Path.cpus: must be a number greater than 0 and at most $($script:MaxCpus)")
        return $null
    }
    if ((Format-CpuCount -Cpus ([double]$value)) -ceq '0') {
        $Errors.Add("$Path.cpus: must be a number greater than 0 and at most $($script:MaxCpus), and large enough not to be written as 0, which Docker reads as no limit")
        return $null
    }
    return [double]$value
}

# returns the entry's memory limit in whole gigabytes, the default when the
# field is absent, or $null after recording why the value is unusable.
function Get-MemoryLimit {
    param([Parameter(Mandatory)]$Node, [Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Errors)

    $property = $Node.PSObject.Properties['memoryGb']
    if ($null -eq $property) {
        return $script:DefaultMemoryGb
    }
    $value = $property.Value
    if (($value -isnot [int] -and $value -isnot [long]) -or $value -lt 1 -or $value -gt $script:MaxMemoryGb) {
        $Errors.Add("$Path.memoryGb: must be an integer from 1 to $($script:MaxMemoryGb)")
        return $null
    }
    return [int]$value
}

# returns the entry's name, or $null after recording why it is unusable.
function Get-EntryName {
    param([Parameter(Mandatory)]$Node, [Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Errors)

    $name = Get-StringField -Node $Node -Name 'name' -Path "$Path.name" -Errors $Errors `
        -Pattern $script:NamePattern -Expectation 'lowercase letters, digits and hyphens, starting with a letter or digit'
    if ($null -ne $name -and $name.Length -gt $script:MaxNameLength) {
        $Errors.Add("$Path.name: name is $($name.Length) characters, at most $($script:MaxNameLength) are allowed because it starts every runner name")
        return $null
    }
    return $name
}

# returns one repository's plan entry, or $null after recording every problem.
function Get-RepositoryPlan {
    param(
        [Parameter(Mandatory)]$Node,
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)]$Errors
    )

    if (-not (Test-IsObject -Node $Node -Path $Path -Errors $Errors)) {
        return $null
    }
    Test-UnknownField -Node $Node -Allowed $script:RepositoryFields -Path $Path -Errors $Errors

    $owner = Get-StringField -Node $Node -Name 'owner' -Path "$Path.owner" -Errors $Errors `
        -Pattern $script:OwnerPattern -Expectation 'a GitHub owner name (letters, digits and hyphens)'
    $repository = Get-StringField -Node $Node -Name 'repository' -Path "$Path.repository" -Errors $Errors `
        -Pattern $script:RepositoryPattern -Expectation 'a GitHub repository name (letters, digits and . _ -)'
    $tokenPath = Get-StringField -Node $Node -Name 'tokenPath' -Path "$Path.tokenPath" -Errors $Errors `
        -Pattern $script:TokenPathPattern -Expectation 'an absolute Windows path, such as C:\path\to\file or \\server\share\file'
    $labels = Get-CustomLabel -Node $Node -Path $Path -Errors $Errors
    $volumes = Get-VolumeDefinition -Node $Node -Path $Path -Errors $Errors

    $slots = Get-Field -Node $Node -Name 'slots' -Path "$Path.slots" -Errors $Errors
    if ($null -ne $slots -and (($slots -isnot [int] -and $slots -isnot [long]) -or $slots -lt 1 -or $slots -gt $script:MaxSlots)) {
        $Errors.Add("$Path.slots: must be an integer from 1 to $($script:MaxSlots)")
        $slots = $null
    }

    $name = Get-EntryName -Node $Node -Path $Path -Errors $Errors
    $cpus = Get-CpuLimit -Node $Node -Path $Path -Errors $Errors
    $memoryGb = Get-MemoryLimit -Node $Node -Path $Path -Errors $Errors
    $diagnostics = $false
    $rawRecords = $false
    foreach ($setting in @('diagnostics', 'diagnosticRawRecords')) {
        $property = $Node.PSObject.Properties[$setting]
        if ($null -ne $property) {
            if ($property.Value -isnot [bool]) { $Errors.Add("$Path.${setting}: must be a boolean") }
            elseif ($setting -eq 'diagnostics') { $diagnostics = $property.Value }
            else { $rawRecords = $property.Value }
        }
    }
    $diagnosticDirectory = ''
    $directoryProperty = $Node.PSObject.Properties['diagnosticDirectory']
    if ($diagnostics) {
        $diagnosticDirectory = Get-StringField -Node $Node -Name 'diagnosticDirectory' -Path "$Path.diagnosticDirectory" -Errors $Errors `
            -Pattern '\A[A-Za-z]:\\[^\x00-\x1f"<>|?*:/]+\z' -Expectation 'an absolute local Windows directory outside the checkout'
        if ($null -ne $diagnosticDirectory -and $diagnosticDirectory -match '(^|\\)\.\.?($|\\)') {
            $Errors.Add("$Path.diagnosticDirectory: must not contain dot path components")
        }
        if ($null -ne $diagnosticDirectory -and [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
            try {
                $diagnosticDirectory = [IO.Path]::GetFullPath($diagnosticDirectory)
                $checkout = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd([char]92)
                if ($diagnosticDirectory.TrimEnd([char]92).Equals($checkout, [StringComparison]::OrdinalIgnoreCase) -or
                    $diagnosticDirectory.StartsWith($checkout + '\', [StringComparison]::OrdinalIgnoreCase)) {
                    $Errors.Add("$Path.diagnosticDirectory: must be outside the checkout")
                }
            } catch {
                $Errors.Add("$Path.diagnosticDirectory: cannot normalize local Windows directory")
            }
        }
    } elseif ($null -ne $directoryProperty) {
        $Errors.Add("$Path.diagnosticDirectory: requires diagnostics")
    }
    if ($rawRecords -and -not $diagnostics) { $Errors.Add("$Path.diagnosticRawRecords: requires diagnostics") }

    if ($null -eq $owner -or $null -eq $repository -or $null -eq $tokenPath -or $null -eq $labels `
            -or $null -eq $volumes -or $null -eq $slots -or $null -eq $name -or $null -eq $cpus -or $null -eq $memoryGb) {
        return $null
    }

    $volumePlan = @($volumes | ForEach-Object {
            [pscustomobject]@{ Name = "$name-$($_.Suffix)"; MountPath = $_.MountPath }
        })
    return [pscustomobject]@{
        Path       = $Path
        Owner      = $owner
        Repository = $repository
        Name       = $name
        Slots      = [int]$slots
        TokenPath  = $tokenPath
        Labels     = [string[]](@($script:DefaultLabels) + @($labels))
        Cpus       = $cpus
        MemoryGb   = $memoryGb
        Volumes    = $volumePlan
        Diagnostics = $diagnostics
        DiagnosticDirectory = [string]$diagnosticDirectory
        DiagnosticRawRecords = $rawRecords
    }
}

function Test-NameCollision {
    param([string]$First, [string]$Second)

    return $First -eq $Second -or $First.StartsWith("$Second-") -or $Second.StartsWith("$First-")
}

# records a duplicate repository, a colliding name or a shared token file
# between any two entries, against the later entry. volume names are the name
# plus a suffix, so they cannot collide while the names do not. paths
# compare case-insensitively because Windows file names do.
function Test-EntryUniqueness {
    param([Parameter(Mandatory)][object[]]$Entries, [Parameter(Mandatory)]$Errors)

    for ($later = 1; $later -lt $Entries.Count; $later++) {
        for ($earlier = 0; $earlier -lt $later; $earlier++) {
            $a = $Entries[$earlier]
            $b = $Entries[$later]
            if ("$($a.Owner)/$($a.Repository)".ToLowerInvariant() -eq "$($b.Owner)/$($b.Repository)".ToLowerInvariant()) {
                $Errors.Add("$($b.Path).repository: duplicate repository '$($b.Owner)/$($b.Repository)', already listed at $($a.Path)")
                continue
            }
            if (Test-NameCollision -First $a.Name -Second $b.Name) {
                $Errors.Add("$($b.Path).name: name '$($b.Name)' collides with '$($a.Name)' at $($a.Path); set a distinct name on one entry")
            }
            if ($a.TokenPath.ToLowerInvariant() -eq $b.TokenPath.ToLowerInvariant()) {
                $Errors.Add("$($b.Path).tokenPath: token file '$($b.TokenPath)' is already used at $($a.Path); each repository needs its own token")
            }
        }
    }
}

<#
.SYNOPSIS
    reads the host configuration at -Path and returns its plan, or throws one
    message naming every offending field.
#>
function Read-HostConfiguration {
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Host configuration file not found: $Path"
    }
    try {
        $root = Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
    } catch {
        throw "Host configuration is not valid JSON: $($_.Exception.Message)"
    }

    $errors = New-Object System.Collections.Generic.List[string]
    if (-not (Test-IsObject -Node $root -Path 'configuration' -Errors $errors)) {
        throw "Invalid host configuration:`n  - $($errors -join "`n  - ")"
    }
    Test-UnknownField -Node $root -Allowed $script:HostFields -Path 'configuration' -Errors $errors

    $imageName = Get-StringField -Node $root -Name 'imageName' -Path 'imageName' -Errors $errors `
        -Pattern $script:ImageNamePattern -Expectation 'a local Docker image name with a tag, such as name:tag'

    $entries = New-Object System.Collections.Generic.List[object]
    $repositories = Get-Field -Node $root -Name 'repositories' -Path 'repositories' -Errors $errors
    if ($null -ne $repositories -and $repositories -isnot [array]) {
        $errors.Add('repositories: must be a list')
    } elseif ($null -ne $repositories) {
        if (@($repositories).Count -eq 0) {
            $errors.Add('repositories: must list at least one repository')
        }
        $index = 0
        foreach ($node in @($repositories)) {
            $entry = Get-RepositoryPlan -Node $node -Path "repositories[$index]" -Errors $errors
            $index++
            if ($null -ne $entry) {
                $entries.Add($entry)
            }
        }
    }
    if ($entries.Count -gt 1) {
        Test-EntryUniqueness -Entries $entries.ToArray() -Errors $errors
    }

    if ($errors.Count -gt 0) {
        throw "Invalid host configuration ($Path):`n  - $($errors -join "`n  - ")"
    }
    $affinityOffset = 0L
    foreach ($entry in $entries) {
        $count = [int][Math]::Ceiling($entry.Cpus)
        $entry | Add-Member -NotePropertyName CpuAffinityCount -NotePropertyValue $count
        $entry | Add-Member -NotePropertyName CpuAffinityOffset -NotePropertyValue $affinityOffset
        $affinityOffset += [long]$entry.Slots * $count
    }
    return [pscustomobject]@{
        ImageName    = $imageName
        Repositories = $entries.ToArray()
    }
}

# returns the CPU count as text for Docker and for the summary: the invariant
# culture keeps the decimal point whatever the machine's regional settings, and
# the custom format never switches to an exponent.
function Format-CpuCount {
    param([Parameter(Mandatory)][double]$Cpus)

    return $Cpus.ToString('0.#########', [System.Globalization.CultureInfo]::InvariantCulture)
}

# returns the arguments of a slot's background job, keyed by the worker script
# block's parameter names and in their order. Start-Job binds them to those
# parameters by position, so a value out of order reaches the wrong parameter.
function Get-SlotWorkerArgument {
    param(
        [Parameter(Mandatory)]$Entry,
        [Parameter(Mandatory)][int]$Slot,
        [Parameter(Mandatory)][string]$ScriptRoot,
        [Parameter(Mandatory)][string]$ImageName,
        [string[]]$Mounts = @(),
        [Parameter(Mandatory)][string]$RunCommand,
        [Parameter(Mandatory)][string]$JitConfigVariable,
        [Parameter(Mandatory)][int]$InitialBackoffSeconds,
        [Parameter(Mandatory)][int]$MaxBackoffSeconds,
        [int]$BackoffSeconds = $InitialBackoffSeconds
    )

    return [ordered]@{
        ScriptRoot            = $ScriptRoot
        Owner                 = $Entry.Owner
        Repository            = $Entry.Repository
        TokenPath             = $Entry.TokenPath
        Slot                  = $Slot
        ImageName             = $ImageName
        EntryName             = $Entry.Name
        Labels                = $Entry.Labels
        Cpus                  = Format-CpuCount -Cpus $Entry.Cpus
        CpuAffinityCount      = $Entry.CpuAffinityCount
        CpuAffinityOffset     = [long]($Entry.CpuAffinityOffset + [long]($Slot - 1) * $Entry.CpuAffinityCount)
        MemoryGb              = $Entry.MemoryGb
        Mounts                = $Mounts
        RunCommand            = $RunCommand
        JitConfigVariable     = $JitConfigVariable
        InitialBackoffSeconds = $InitialBackoffSeconds
        MaxBackoffSeconds     = $MaxBackoffSeconds
        Diagnostics           = [bool]$Entry.Diagnostics
        DiagnosticRawRecords  = [bool]$Entry.DiagnosticRawRecords
        BackoffSeconds        = $BackoffSeconds
    }
}

# returns the lines the supervisor prints for -ValidateOnly.
function Get-PlanSummary {
    param([Parameter(Mandatory)]$Plan)

    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add("Host configuration is valid: $(@($Plan.Repositories).Count) repositories, image $($Plan.ImageName)")
    $lines.Add('CPU policy: cpus is CPU-time quota; affinity count is ceiling(cpus), not reserved CPUs.')
    foreach ($entry in $Plan.Repositories) {
        $lines.Add('')
        $lines.Add("Repository $($entry.Owner)/$($entry.Repository)")
        $lines.Add("  slots:             $($entry.Slots)")
        $lines.Add("  labels:            $($entry.Labels -join ', ')")
        $lines.Add("  token file:        $($entry.TokenPath)")
        $lines.Add("  name:              $($entry.Name)")
        $lines.Add("  containers:        $($entry.Name)-<index>-<timestamp>")
        $lines.Add("  cpus:              $(Format-CpuCount -Cpus $entry.Cpus)")
        $lines.Add("  affinity per slot: $($entry.CpuAffinityCount) CPUs; IDs discovered at launch")
        $lines.Add("  affinity position: $($entry.CpuAffinityOffset)")
        $lines.Add("  memory:            $($entry.MemoryGb) GB")
        $lines.Add("  diagnostics:       $($entry.Diagnostics)")
        $lines.Add("  raw records:       $($entry.DiagnosticRawRecords)")
        foreach ($volume in $entry.Volumes) {
            $lines.Add("  volume:            $($volume.Name) -> $($volume.MountPath)")
        }
    }
    return $lines.ToArray()
}
