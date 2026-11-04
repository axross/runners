# PSScriptAnalyzer settings for `mise run lint:powershell`. PowerShell in this
# repository must parse under Windows PowerShell 5.1, but the linter runs on
# PowerShell 7, so this rule flags syntax a 5.1 parser rejects, such as a ternary
# operator, `??`, `?.`, and `&&`. the default rules stay on.
@{
    Rules = @{
        PSUseCompatibleSyntax = @{
            Enable         = $true
            TargetVersions = @('5.1')
        }
    }
}
