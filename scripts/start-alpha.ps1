# Backend de lançamento Windows do run-alpha.sh; runtime e AOT continuam Rust.
param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$RunDirectory,
    [ValidateRange(1,999999999)][int]$Slices,
    [switch]$Headless
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Executable).Path
$folder = (Resolve-Path -LiteralPath $RunDirectory).Path
foreach ($name in @('stdout.log','stderr.log','pid.txt','guest.bgra')) {
    if (Test-Path -LiteralPath (Join-Path $folder $name)) { throw "Run artifact already exists: $name" }
}
$frame = Join-Path $folder 'guest.bgra'
$arguments = '--fatias {0} --quadro "{1}"' -f $Slices,$frame
$style = 'Hidden'
if (-not $Headless) { $arguments += ' --janela'; $style = 'Normal' }
# A visible interactive application needs Normal startup (ADR-0074).
$app = $null
try {
    $app = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru -WindowStyle $style -RedirectStandardOutput (Join-Path $folder 'stdout.log') -RedirectStandardError (Join-Path $folder 'stderr.log')
    $null = $app.Handle
    [IO.File]::WriteAllText((Join-Path $folder 'pid.txt'),[string]$app.Id)
    $app.WaitForExit()
    $code = $app.ExitCode
} finally {
    # If the launcher is interrupted, clean only the process it started.
    if ($null -ne $app) {
        $app.Refresh()
        if (-not $app.HasExited) { $app.Kill(); $app.WaitForExit() }
    }
}
if ($null -eq $code) { throw 'Process exit code unavailable' }
exit $code
