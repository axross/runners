#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

# decodes at most the requested UTF-8 byte budget; malformed or excess data throws.
function Read-BoundedPrerequisiteText {
    param([string]$Path, [int]$MaximumBytes)

    $stream = $null
    try {
        $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        if ($stream.Length -gt $MaximumBytes) { throw 'text exceeds byte budget' }
        $buffer = New-Object byte[] ($MaximumBytes + 1)
        $length = 0
        while ($length -le $MaximumBytes) {
            $count = $stream.Read($buffer, $length, $buffer.Length - $length)
            if ($count -eq 0) { break }
            $length += $count
        }
        if ($length -gt $MaximumBytes) { throw 'text exceeds byte budget' }
        $encoding = New-Object Text.UTF8Encoding $false, $true
        $text = $encoding.GetString($buffer, 0, $length)
        if ($text.Length -gt 0 -and $text[0] -eq [char]0xfeff) { $text = $text.Substring(1) }
        return $text
    } finally {
        if ($null -ne $stream) { $stream.Dispose() }
    }
}

# checks Windows directory syntax without resolving a root or inspecting its storage.
function ConvertTo-PrerequisiteRoot {
    param([string]$Value, [string]$Field)

    if ($Value -cnotmatch '\A[A-Za-z]:\\' -or $Value.Length -le 3) {
        throw "$Field`: requires a non-root absolute local Windows directory"
    }
    $root = $Value.TrimEnd('\')
    if ($Value.EndsWith('\\') -or $root.Length -le 3) { throw "$Field`: invalid Windows directory components" }
    foreach ($component in $root.Substring(3).Split('\')) {
        if ($component.Length -eq 0 -or $component -in @('.', '..') -or
            $component -match '[<>:"/|?*%\x00-\x1f]' -or $component -match '[.\s]\z' -or
            $component -match '\A(?:CON|CONIN\$|CONOUT\$|PRN|AUX|NUL|COM[1-9\u00b9\u00b2\u00b3]|LPT[1-9\u00b9\u00b2\u00b3])(?:\.|\z)') {
            throw "$Field`: invalid Windows directory components"
        }
    }
    return $root
}

function Test-PrerequisiteRootIntersection {
    param([string]$Left, [string]$Right)

    return $Left.Equals($Right, [StringComparison]::OrdinalIgnoreCase) -or
        $Left.StartsWith($Right.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) -or
        $Right.StartsWith($Left.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)
}

# accepts only the two string fields; rejection messages never contain supplied keys or values.
function Read-PrerequisiteConfiguration {
    param([string]$ConfigPath, [string]$CheckoutRoot)

    $maximumConfigurationBytes = 4096
    try {
        $configurationFile = [IO.Path]::GetFullPath($ConfigPath)
        if ($CheckoutRoot -match '\A[A-Za-z]:\\' -and $configurationFile -notmatch '\A[A-Za-z]:\\') { throw 'nonlocal configuration' }
        $text = Read-BoundedPrerequisiteText -Path $configurationFile -MaximumBytes $maximumConfigurationBytes
    }
    catch { throw 'sdkRoot, avdRoot: configuration must be readable UTF-8 of at most 4096 bytes' }
    $stringToken = '"(?:[^"\\\x00-\x1f]|\\(?:["\\/bfnrt]|u[0-9a-fA-F]{4}))*"'
    $space = '[ \t\r\n]*'
    $pair = "(?<key>$stringToken)$space`:$space(?<value>$stringToken)"
    $pattern = "\A$space\{$space$pair$space,$space$pair$space\}$space\z"
    $match = [regex]::Match($text, $pattern, [Text.RegularExpressions.RegexOptions]::CultureInvariant, [TimeSpan]::FromMilliseconds(100))
    if (-not $match.Success) { throw 'sdkRoot, avdRoot: configuration requires exactly these two JSON string fields' }
    $configuration = @{}
    for ($index = 0; $index -lt 2; $index++) {
        $key = @((ConvertFrom-Json -InputObject ('[' + $match.Groups['key'].Captures[$index].Value + ']')))[0]
        $value = @((ConvertFrom-Json -InputObject ('[' + $match.Groups['value'].Captures[$index].Value + ']')))[0]
        if ($key -cnotin @('sdkRoot', 'avdRoot') -or $configuration.ContainsKey($key)) {
            throw 'sdkRoot, avdRoot: unknown or duplicate configuration fields'
        }
        if ($value.Length -gt 240) { throw "$key`: directory exceeds character budget" }
        $configuration[$key] = ConvertTo-PrerequisiteRoot -Value $value -Field $key
    }
    if (Test-PrerequisiteRootIntersection -Left $configuration.sdkRoot -Right $configuration.avdRoot) {
        throw 'sdkRoot, avdRoot: directories must not overlap'
    }
    if ($CheckoutRoot -match '\A[A-Za-z]:\\') {
        foreach ($field in @('sdkRoot', 'avdRoot')) {
            if (Test-PrerequisiteRootIntersection -Left $configuration[$field] -Right $CheckoutRoot) {
                throw "$field`: directory must not intersect the checkout"
            }
        }
    }
    return [pscustomobject]@{ sdkRoot = $configuration.sdkRoot; avdRoot = $configuration.avdRoot }
}
