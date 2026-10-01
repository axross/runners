#Requires -Version 5.1
<#
.SYNOPSIS
    reads and validates a runner host configuration file, dot-sourced by the
    host scripts and the tests.

.DESCRIPTION
    Read-HostConfiguration turns the json file into a plan: the image name, and
    per target repository the registration labels, the container name prefix and
    the volume names. a configuration is rejected as a whole, with one message
    line per problem naming the offending field, before any container starts.
    nothing here calls Docker or GitHub, or reads a token file.
#>

$script:DefaultLabels = @('self-hosted', 'linux', 'x64')
$script:MaxSlots = 16
# GitHub documents no limit for a runner's name. the derived runner name is the
# container prefix plus a slot number and a 17-digit timestamp, so capping the
# prefix keeps every name short, on an assumed limit rather than a known one.
$script:MaxPrefixLength = 64

$script:HostFields = @('hostPrefix', 'imageName', 'repositories')
$script:RepositoryFields = @('owner', 'repository', 'slots', 'tokenPath', 'labels', 'volumes', 'prefix')
$script:VolumeFields = @('suffix', 'mountPath')

$script:PrefixPattern = '^[a-z0-9][a-z0-9-]*$'
$script:OwnerPattern = '^[A-Za-z0-9][A-Za-z0-9-]*$'
$script:RepositoryPattern = '^(?!\.{1,2}$)[A-Za-z0-9_.-]+$'
$script:DerivedPrefixPattern = '^[a-z0-9][a-z0-9_.-]*$'
$script:ImageNamePattern = '^[A-Za-z0-9][A-Za-z0-9_.:/@-]*$'
$script:LabelPattern = '^[A-Za-z0-9][A-Za-z0-9._:/-]*$'
$script:MountPathPattern = '^/[A-Za-z0-9_./-]+$'
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

# returns the custom labels, or $null after recording why the list is unusable.
function Get-CustomLabel {
    param([Parameter(Mandatory)]$Node, [Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Errors)

    $raw = Get-Field -Node $Node -Name 'labels' -Path "$Path.labels" -Errors $Errors
    if ($null -eq $raw) {
        return $null
    }
    if ($raw -isnot [array]) {
        $Errors.Add("${Path}.labels: must be a list of custom labels")
        return $null
    }
    $items = @($raw)
    if ($items.Count -eq 0) {
        $Errors.Add("${Path}.labels: at least one custom label is required")
        return $null
    }

    $valid = $true
    $seen = @{}
    foreach ($item in $items) {
        if ($item -isnot [string] -or $item -cnotmatch $script:LabelPattern) {
            $Errors.Add("${Path}.labels: each label must be a string of letters, digits and . _ : / - starting with a letter or digit")
            $valid = $false
        } elseif ($script:DefaultLabels -contains $item.ToLowerInvariant()) {
            $Errors.Add("${Path}.labels: '$item' is always added, so list only custom labels (at least one is required)")
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
            -Pattern $script:PrefixPattern -Expectation 'lowercase letters, digits and hyphens, starting with a letter or digit'
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

# returns one repository's plan entry, or $null after recording every problem.
function Get-RepositoryPlan {
    param(
        [Parameter(Mandatory)]$Node,
        [Parameter(Mandatory)][string]$Path,
        [string]$HostPrefix,
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

    $prefixProperty = $Node.PSObject.Properties['prefix']
    $prefix = $null
    if ($null -ne $prefixProperty) {
        $prefix = Get-StringField -Node $Node -Name 'prefix' -Path "$Path.prefix" -Errors $Errors `
            -Pattern $script:DerivedPrefixPattern -Expectation 'lowercase letters, digits and . _ -, starting with a letter or digit'
    } elseif ($null -ne $owner -and $null -ne $repository) {
        $prefix = "$HostPrefix-$owner-$repository".ToLowerInvariant()
    }
    if ($null -ne $prefix -and $prefix.Length -gt $script:MaxPrefixLength) {
        $Errors.Add("$Path.prefix: container prefix is $($prefix.Length) characters, at most $($script:MaxPrefixLength) are allowed because it starts every runner name; set a shorter prefix on this entry")
        $prefix = $null
    }

    if ($null -eq $owner -or $null -eq $repository -or $null -eq $tokenPath -or $null -eq $labels `
            -or $null -eq $volumes -or $null -eq $slots -or $null -eq $prefix) {
        return $null
    }

    $volumePlan = @($volumes | ForEach-Object {
            [pscustomobject]@{ Name = "$prefix-$($_.Suffix)"; MountPath = $_.MountPath }
        })
    return [pscustomobject]@{
        Path            = $Path
        Owner           = $owner
        Repository      = $repository
        Slots           = [int]$slots
        TokenPath       = $tokenPath
        Labels          = [string[]](@($script:DefaultLabels) + @($labels))
        ContainerPrefix = $prefix
        Volumes         = $volumePlan
    }
}

function Test-PrefixCollision {
    param([string]$First, [string]$Second)

    return $First -eq $Second -or $First.StartsWith("$Second-") -or $Second.StartsWith("$First-")
}

# records a duplicate repository, a colliding container prefix or a shared token
# file between any two entries, against the later entry. volume names are the
# prefix plus a suffix, so they cannot collide while the prefixes do not. paths
# compare case-insensitively because windows file names do.
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
            if (Test-PrefixCollision -First $a.ContainerPrefix -Second $b.ContainerPrefix) {
                $Errors.Add("$($b.Path).prefix: container prefix '$($b.ContainerPrefix)' collides with '$($a.ContainerPrefix)' at $($a.Path); set a distinct prefix on one entry")
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

    $hostPrefix = Get-StringField -Node $root -Name 'hostPrefix' -Path 'hostPrefix' -Errors $errors `
        -Pattern '^[a-z0-9][a-z0-9-]{0,31}$' -Expectation 'up to 32 lowercase letters, digits and hyphens, starting with a letter or digit'
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
            $entry = Get-RepositoryPlan -Node $node -Path "repositories[$index]" -HostPrefix "$hostPrefix" -Errors $errors
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
    return [pscustomobject]@{
        HostPrefix   = $hostPrefix
        ImageName    = $imageName
        Repositories = $entries.ToArray()
    }
}

# returns the lines the supervisor prints for -ValidateOnly.
function Get-PlanSummary {
    param([Parameter(Mandatory)]$Plan)

    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add("Host configuration is valid: $(@($Plan.Repositories).Count) repositories, image $($Plan.ImageName)")
    foreach ($entry in $Plan.Repositories) {
        $lines.Add('')
        $lines.Add("Repository $($entry.Owner)/$($entry.Repository)")
        $lines.Add("  slots:             $($entry.Slots)")
        $lines.Add("  labels:            $($entry.Labels -join ', ')")
        $lines.Add("  token file:        $($entry.TokenPath)")
        $lines.Add("  container prefix:  $($entry.ContainerPrefix)")
        $lines.Add("  containers:        $($entry.ContainerPrefix)-<slot>-<timestamp>")
        foreach ($volume in $entry.Volumes) {
            $lines.Add("  volume:            $($volume.Name) -> $($volume.MountPath)")
        }
    }
    return $lines.ToArray()
}
