# Captures the actual Win32 client surface and compares RGB with --quadro output.
# Build first: execution deadlines below deliberately exclude Cargo/rustc.
param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [ValidateRange(1,1000000000)][int]$Slices = 320,
    [switch]$DuringExecution,
    [ValidateRange(5,600)][int]$StartupTimeoutSeconds = 120
)
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows graphical session required' }
$exe = (Resolve-Path -LiteralPath $Executable).Path
$folder = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $folder) { throw "Output directory must be new: $folder" }
[IO.Directory]::CreateDirectory($folder) | Out-Null
$frame = Join-Path $folder 'guest.bgra'
$errorLog = Join-Path $folder 'stderr.log'
$outputLog = Join-Path $folder 'stdout.log'
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
public static class AlphaWindowCheck {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l,t,r,b; }
    public class Stats { public int Width,Height,Pixels,Rows,Red,Green,Blue,Mismatches; }
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr window);
    [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr window, IntPtr dc);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr window,uint message,UIntPtr w,IntPtr l);
    [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr dst,int x,int y,int width,int height,IntPtr src,int sx,int sy,uint rop);
    public static Stats CaptureAndCompare(IntPtr window,string expectedPath,string pngPath) {
        RECT rect;
        if(!GetClientRect(window,out rect)) throw new InvalidOperationException("GetClientRect failed");
        int width=rect.r-rect.l, height=rect.b-rect.t;
        if(width!=640 || height!=480) throw new InvalidOperationException("Client must be 640x480, got "+width+"x"+height);
        byte[] expected=File.ReadAllBytes(expectedPath), actual=new byte[width*height*4];
        if(expected.Length!=actual.Length) throw new InvalidOperationException("Invalid guest BGRA length");
        using(var bitmap=new Bitmap(width,height,PixelFormat.Format32bppRgb)) {
            using(var graphics=Graphics.FromImage(bitmap)) {
                IntPtr dst=graphics.GetHdc(), src=GetDC(window);
                try {
                    if(src==IntPtr.Zero || !BitBlt(dst,0,0,width,height,src,0,0,0x00CC0020))
                        throw new InvalidOperationException("Cannot capture client surface with BitBlt");
                } finally { if(src!=IntPtr.Zero) ReleaseDC(window,src); graphics.ReleaseHdc(dst); }
            }
            bitmap.Save(pngPath,ImageFormat.Png);
            var data=bitmap.LockBits(new Rectangle(0,0,width,height),ImageLockMode.ReadOnly,PixelFormat.Format32bppRgb);
            try {
                for(int y=0;y<height;y++) Marshal.Copy(IntPtr.Add(data.Scan0,y*data.Stride),actual,y*width*4,width*4);
            } finally { bitmap.UnlockBits(data); }
        }
        var result=new Stats {Width=width,Height=height};
        for(int y=0;y<height;y++) {
            bool colored=false;
            for(int x=0;x<width;x++) {
                int i=(y*width+x)*4;
                int blue=actual[i],green=actual[i+1],red=actual[i+2];
                if(blue!=expected[i] || green!=expected[i+1] || red!=expected[i+2]) result.Mismatches++;
                if(red!=0 || green!=0 || blue!=0) {result.Pixels++;colored=true;}
                if(red>=192 && green<=32 && blue<=32) result.Red++;
                if(green>=192 && red<=32 && blue<=32) result.Green++;
                if(blue>=192 && red<=32 && green<=32) result.Blue++;
            }
            if(colored) result.Rows++;
        }
        if(result.Mismatches!=0) throw new InvalidOperationException("RGB mismatch pixels: "+result.Mismatches+"; capture preserved at "+pngPath);
        return result;
    }
}
'@
function Read-RunLog {
    # RedirectStandardError is still writing: permit the existing writer while polling.
    $stream = [IO.File]::Open($errorLog,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
    try {
        $reader = [IO.StreamReader]::new($stream,[Text.Encoding]::UTF8)
        try { return $reader.ReadToEnd() } finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
}
$arguments = '--janela --fatias {0} --quadro "{1}"' -f $Slices,$frame
# This is an interactive application under test. Hidden startup also hides its
# first ShowWindow, so explicitly show it; helper processes stay out of this path.
$app = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru -WindowStyle Normal -RedirectStandardOutput $outputLog -RedirectStandardError $errorLog
# Keep a process handle while it is alive: Windows PowerShell's Process wrapper
# otherwise can lose ExitCode after Refresh/exit. A missing code must never pass.
$null = $app.Handle
$stats = $null
try {
    $startup = [Diagnostics.Stopwatch]::StartNew()
    do {
        Start-Sleep -Milliseconds 50
        $app.Refresh()
        if ($app.HasExited) { throw "Process exited before window: $(Read-RunLog)" }
    } until ($app.MainWindowHandle -ne 0 -or $startup.Elapsed.TotalSeconds -ge $StartupTimeoutSeconds)
    if ($app.MainWindowHandle -eq 0) { throw 'Window startup deadline exceeded' }
    if ($DuringExecution) {
        if ((Read-RunLog) -match 'janela mantida aberta') { throw 'Guest already stopped before close test' }
    } else {
        do {
            Start-Sleep -Milliseconds 50
            $app.Refresh()
            if ($app.HasExited) { throw "Process exited before final frame: $(Read-RunLog)" }
        } until ((Read-RunLog) -match 'janela mantida aberta' -or $startup.Elapsed.TotalSeconds -ge $StartupTimeoutSeconds)
        if ((Read-RunLog) -notmatch 'janela mantida aberta') { throw 'Final-frame deadline exceeded' }
        # Main keeps presenting the frozen final state. Read back the window DC,
        # not a bitmap constructed from guest.bgra; alpha is intentionally ignored.
        $stats = [AlphaWindowCheck]::CaptureAndCompare($app.MainWindowHandle,$frame,(Join-Path $folder 'client.png'))
    }
    $closing = [Diagnostics.Stopwatch]::StartNew()
    if (-not [AlphaWindowCheck]::PostMessageW($app.MainWindowHandle,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)) { throw 'WM_CLOSE failed' }
    if (-not $app.WaitForExit(5000)) { throw 'WM_CLOSE exceeded five seconds' }
    $closing.Stop()
    if ($closing.Elapsed.TotalMilliseconds -gt 5000) { throw 'WM_CLOSE exceeded five seconds' }
    $exitCode = $app.ExitCode
    if ($null -eq $exitCode) { throw 'Process exit code unavailable' }
    $text = Read-RunLog
    if ($DuringExecution) {
        if ($exitCode -ne 0 -or $text -notmatch 'pelo host\.' -or $text -match 'janela mantida aberta') {
            throw "Not a controlled host stop: $text"
        }
    } elseif ($exitCode -ne 1 -or $text -notmatch 'Error:.*interrompida ap.s [0-9]+ blocos;') {
        throw "Expected budget stop after closing frozen frame: $text"
    }
    if (-not (Test-Path -LiteralPath $frame)) { throw 'Guest frame was not exported' }
    $report = [ordered]@{ executable=$exe; during_execution=[bool]$DuringExecution; slices=$Slices;
        exit_code=$exitCode; close_ms=$closing.Elapsed.TotalMilliseconds; surface=$stats;
        guest_sha256=(Get-FileHash -LiteralPath $frame -Algorithm SHA256).Hash }
    $report | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $folder 'result.json')
    $report | ConvertTo-Json -Depth 4
} finally {
    $app.Refresh()
    # Timeout cleanup is restricted to the process created above; never other apps.
    if (-not $app.HasExited) { $app.Kill(); $app.WaitForExit() }
}
