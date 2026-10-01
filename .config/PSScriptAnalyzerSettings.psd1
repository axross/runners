# PSScriptAnalyzer settings for `mise run lint:powershell`. The host scripts run
# on Windows PowerShell 5.1 as well as PowerShell 7, but the linter runs on 7, so
# this rule flags syntax a 5.1 parser rejects, such as a ternary operator, `??`,
# `?.`, and `&&`. The default rules stay on.
@{
    Rules = @{
        PSUseCompatibleSyntax = @{
            Enable         = $true
            TargetVersions = @('5.1')
        }
    }
}
