# PSScriptAnalyzer settings for `mise run lint:powershell`. the host scripts
# target Windows PowerShell 5.1, but the linter runs on PowerShell 7, so this
# rule flags syntax a 5.1 parser rejects, such as a ternary operator, `??`, `?.`,
# and `&&`. the default rules stay on, and the task fails if the rule stops
# flagging a PowerShell 7 construct.
@{
    Rules = @{
        PSUseCompatibleSyntax = @{
            Enable         = $true
            TargetVersions = @('5.1')
        }
    }
}
