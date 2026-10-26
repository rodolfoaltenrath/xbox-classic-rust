# Exercise the actual polling function with partial streamed log records.
$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '../verify-session.ps1'
$tokens = $null; $parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path).Path,[ref]$tokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw 'Cannot parse session helper' }
$definition = $ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-Log'},$true)
if ($null -eq $definition) { throw 'Missing polling function' }
Invoke-Expression $definition.Extent.Text
$TimeoutSeconds = 2
$script:app = [pscustomobject]@{HasExited=$false}
$script:app | Add-Member -MemberType ScriptMethod -Name Refresh -Value {}
function Read-Log {
    $script:reads++
    return $script:chunks[[Math]::Min($script:reads-1,$script:chunks.Count-1)]
}
function Check-Case([string[]]$chunks,[string]$pattern,[int]$count,[string]$expected) {
    $script:chunks = $chunks; $script:reads = 0
    $match = Wait-Log $pattern $count
    if ($script:reads -lt 2 -or $match.Groups[1].Value -ne $expected) { throw "Accepted partial log record: $($match.Value)" }
}
Check-Case @('restored EIP=0x0001',"restored EIP=0x00013346`n") 'restored EIP=0x([0-9A-F]+)' 1 '00013346'
Check-Case @("prior line`nrestored cycles=651", "prior line`nrestored cycles=651219046442`n") 'restored cycles=(\d+)' 1 '651219046442'
Check-Case @("saved value=1`nsaved value=2", "saved value=1`nsaved value=234`n") 'saved value=(\d+)' 2 '234'
Write-Output 'SESSION LOG OK: 3 cases'
