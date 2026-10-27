# Real Win32 pause/save/advance/restore proof. Build is deliberately separate.
param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [ValidateSet('hello','triangle','gamma-fade','file-read','file-enum')][string]$Sample,
    [ValidateRange(10,600)][int]$TimeoutSeconds = 120
)
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows graphical session required' }
$exe = (Resolve-Path -LiteralPath $Executable).Path
$folder = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $folder) { throw "Output directory must be new: $folder" }
[IO.Directory]::CreateDirectory($folder) | Out-Null
$slices = switch ($Sample) { 'triangle' { 320 } 'gamma-fade' { 128 } 'file-read' { 512 } 'file-enum' { 128 } default { 64 } }
$states = Join-Path $folder 'states'
$final = Join-Path $folder 'final.obsstate'
$log = Join-Path $folder 'stderr.log'
$app = $null
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class AlphaSessionCheck {
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr window,uint message,UIntPtr w,IntPtr l);
}
'@
function Read-Log {
    if (-not (Test-Path -LiteralPath $script:log)) { return '' }
    $stream = [IO.File]::Open($script:log,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
    try {
        $reader = [IO.StreamReader]::new($stream,[Text.Encoding]::UTF8)
        try { return $reader.ReadToEnd() } finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
}
function Wait-Log([string]$pattern,[int]$count=1) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        $script:app.Refresh()
        if ($script:app.HasExited) { throw "Process exited before $pattern : $(Read-Log)" }
        # stderr formatting is streamed: a numeric suffix can already match
        # while its final digits are still being written. Only consume lines
        # terminated by newline, including for prefix-only patterns.
        $completeLog = Read-Log
        $lineEnd = $completeLog.LastIndexOf("`n")
        if ($lineEnd -lt 0) { $completeLog = '' }
        else { $completeLog = $completeLog.Substring(0,$lineEnd+1) }
        $matches = [regex]::Matches($completeLog,$pattern)
        if ($matches.Count -ge $count) { return $matches[$count-1] }
        Start-Sleep -Milliseconds 30
    }
    throw "Deadline waiting for $pattern ($count): $(Read-Log)"
}
function Key([int]$code) {
    $script:app.Refresh()
    if ($script:app.MainWindowHandle -eq 0) { throw 'Missing window handle' }
    if (-not [AlphaSessionCheck]::PostMessageW($script:app.MainWindowHandle,0x100,[UIntPtr][uint32]$code,[IntPtr]::Zero)) { throw 'WM_KEYDOWN failed' }
}
function Hash([string]$path) { return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash }
function Close-App {
    $script:app.Refresh()
    $clock = [Diagnostics.Stopwatch]::StartNew()
    if (-not [AlphaSessionCheck]::PostMessageW($script:app.MainWindowHandle,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)) { throw 'WM_CLOSE failed' }
    if (-not $script:app.WaitForExit(5000)) { throw 'WM_CLOSE exceeded five seconds' }
    $script:app.WaitForExit() # Flush redirected asynchronous streams after bounded exit.
    $clock.Stop()
    if ($null -eq $script:app.ExitCode -or $script:app.ExitCode -ne 0) { throw "Not a controlled host exit: $(Read-Log)" }
    if ((Read-Log) -notmatch 'pelo host\.' -or (Read-Log) -match 'janela mantida aberta|^Error:') { throw "Unexpected stop: $(Read-Log)" }
    return $clock.Elapsed.TotalMilliseconds
}
function Assert-NoRefusals {
    if ((Read-Log) -notmatch 'Kernel: \d+ chamadas atendidas, 0 recusadas;') { throw "Missing successful kernel summary: $(Read-Log)" }
}
try {
    $arguments = '--janela --continuo --fatias {0} --estados "{1}" --salvar "{2}" --quadro "{3}" --quadro-apresentado "{4}"' -f $slices,$states,$final,(Join-Path $folder 'guest.bgra'),(Join-Path $folder 'display.bgra')
    $app = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru -WindowStyle Normal -RedirectStandardOutput (Join-Path $folder 'stdout.log') -RedirectStandardError $log
    $null = $app.Handle
    # Two complete batches exceed the former default finite budget.
    Wait-Log 'Trecho conclu.do:' 2 | Out-Null
    Key 0x20
    $pause = Wait-Log 'Sess.o pausada: instru..es=(\d+), ciclos=(\d+), EIP=0x([0-9A-F]+)'
    Start-Sleep -Milliseconds 100
    $app.Refresh()
    if ($app.MainWindowTitle -notmatch 'PAUSADO') { throw 'Paused title is missing' }
    Key 0x74; Wait-Log 'Estado salvo:' 1 | Out-Null
    $first = Join-Path $states 'checkpoint-000001.obsstate'
    $expected = Hash $first
    Start-Sleep -Milliseconds 350
    Key 0x74; Wait-Log 'Estado salvo:' 2 | Out-Null
    $second = Join-Path $states 'checkpoint-000002.obsstate'
    if ((Hash $second) -ne $expected) { throw 'Guest state advanced while paused' }
    $batches = [regex]::Matches((Read-Log),'Trecho conclu.do:').Count
    Key 0x20; Wait-Log 'Sess.o retomada:' | Out-Null
    Wait-Log 'Trecho conclu.do:' ($batches+1) | Out-Null
    Key 0x20
    $advanced = Wait-Log 'Sess.o pausada: instru..es=(\d+), ciclos=(\d+), EIP=0x([0-9A-F]+)' 2
    if ([uint64]$advanced.Groups[1].Value -le [uint64]$pause.Groups[1].Value -or [uint64]$advanced.Groups[2].Value -le [uint64]$pause.Groups[2].Value) { throw 'Resume did not advance CPU/Clock' }
    Key 0x78
    $restored = Wait-Log 'Estado restaurado: instru..es=(\d+), ciclos=(\d+), EIP=0x([0-9A-F]+)'
    for ($i=1; $i -le 3; $i++) { if ($restored.Groups[$i].Value -ne $pause.Groups[$i].Value) { throw 'Restored CPU/Clock differs' } }
    Key 0x74; Wait-Log 'Estado salvo:' 3 | Out-Null
    $third = Join-Path $states 'checkpoint-000003.obsstate'
    if ((Hash $third) -ne $expected) { throw 'Restore differs in CPU/RAM/GPU/kernel/thread snapshot' }
    # Deliberately corrupt only the new test generation, then prove failure keeps
    # the active session and the earlier generations intact.
    [IO.File]::WriteAllBytes($third,[byte[]](0,1,2,3))
    Key 0x78; Wait-Log 'Falha ao restaurar estado por F9:' | Out-Null
    Key 0x74; Wait-Log 'Estado salvo:' 4 | Out-Null
    $fourth = Join-Path $states 'checkpoint-000004.obsstate'
    if ((Hash $fourth) -ne $expected -or (Hash $first) -ne $expected -or (Hash $second) -ne $expected) { throw 'Failed restore changed session or prior generations' }
    $pausedClose = Close-App
    Assert-NoRefusals
    if ((Hash $final) -ne $expected) { throw 'Paused close changed session before final save' }
    $rawHash = Hash (Join-Path $folder 'guest.bgra')
    $dacHash = Hash (Join-Path $folder 'display.bgra')
    if ($Sample -eq 'gamma-fade' -and ($rawHash -eq $dacHash -or (Read-Log) -notmatch 'DAC gamma: perfil=PublicReferenceV1 .*entradas_nao_identidade=Some\([1-9]\d*\)')) {
        throw 'Restored gamma-fade must preserve a nonidentity DAC and transformed output'
    }
    $loadedLog = Join-Path $folder 'resumed-stderr.log'
    $log = $loadedLog
    $resumedFinal = Join-Path $folder 'resumed-final.obsstate'
    $arguments = '--janela --continuo --fatias {0} --carregar "{1}" --salvar "{2}"' -f $slices,$first,$resumedFinal
    $app = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru -WindowStyle Normal -RedirectStandardOutput (Join-Path $folder 'resumed-stdout.log') -RedirectStandardError $log
    $null = $app.Handle
    $loaded = Wait-Log 'Estado carregado: .+ \(instru..es=(\d+), ciclos=(\d+), EIP=0x([0-9A-F]+)\)'
    for ($i=1; $i -le 3; $i++) { if ($loaded.Groups[$i].Value -ne $pause.Groups[$i].Value) { throw 'New process did not load saved CPU/Clock' } }
    Wait-Log 'Trecho conclu.do:' | Out-Null
    $runningClose = Close-App
    Assert-NoRefusals
    if ((Hash $resumedFinal) -eq $expected) { throw 'New process did not advance restored guest' }
    $report = [ordered]@{ sample=$Sample; executable=$exe; slices_per_batch=$slices; default_budget_exceeded=$true;
        paused_retired=$pause.Groups[1].Value; paused_cycles=$pause.Groups[2].Value;
        advanced_retired=$advanced.Groups[1].Value; advanced_cycles=$advanced.Groups[2].Value;
        paused_state_sha256=$expected; pause_unchanged=$true; restore_equal=$true;
        failed_restore_preserved=$true; resumed_in_new_process=$true;
        paused_close_ms=$pausedClose; running_close_ms=$runningClose; refused=0;
        raw_sha256=$rawHash; display_sha256=$dacHash; dac_differs_from_raw=($rawHash -ne $dacHash) }
    $report | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $folder 'result.json')
    $report | ConvertTo-Json
} finally {
    if ($null -ne $app) { $app.Refresh(); if (-not $app.HasExited) { $app.Kill(); $app.WaitForExit() } }
}
