param([Parameter(Mandatory=$true)][string]$Out,[switch]$LockHeld)
$ErrorActionPreference = 'Stop'
$lock = 'C:\Users\you\AppData\Local\Temp\glaze-tools\desktop.lock'
if (-not $LockHeld) {
  if (Test-Path -LiteralPath $lock) { throw 'Desktop lock is busy' }
  New-Item -ItemType Directory -Path $lock | Out-Null
}
try {
  Add-Type -AssemblyName System.Drawing
  Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class CoveredCapture {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr arg);
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int L,T,R,B; }
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr arg);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
}
'@
  $studio = @(Get-Process glaze-studio -ErrorAction Stop)
  if ($studio.Count -ne 1) { throw 'Expected one Studio process' }
  $script:studioPid = [uint32]$studio[0].Id
  $script:windows = @()
  [CoveredCapture]::EnumWindows({param($hwnd,$arg)
    $owner = [uint32]0
    [CoveredCapture]::GetWindowThreadProcessId($hwnd,[ref]$owner) | Out-Null
    if ($owner -eq $script:studioPid -and [CoveredCapture]::IsWindowVisible($hwnd)) { $script:windows += $hwnd }
    return $true
  },[IntPtr]::Zero) | Out-Null
  if ($script:windows.Count -ne 1) { throw "Expected one visible Studio window, found $($script:windows.Count)" }
  $oldDpi = [CoveredCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))
  try {
    $rect = New-Object CoveredCapture+Rect
    [CoveredCapture]::GetWindowRect($script:windows[0],[ref]$rect) | Out-Null
    $bitmap = New-Object System.Drawing.Bitmap(($rect.R-$rect.L),($rect.B-$rect.T))
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $dc = $graphics.GetHdc()
    try { if (-not [CoveredCapture]::PrintWindow($script:windows[0],$dc,2)) { throw 'PrintWindow failed' } }
    finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
    try { $bitmap.Save([IO.Path]::GetFullPath($Out),[System.Drawing.Imaging.ImageFormat]::Png) }
    finally { $bitmap.Dispose() }
    [pscustomobject]@{ pid=$script:studioPid; hwnd=$script:windows[0].ToInt64(); width=$rect.R-$rect.L; height=$rect.B-$rect.T; utc=[DateTime]::UtcNow; path=[IO.Path]::GetFullPath($Out) } | ConvertTo-Json
  } finally { [CoveredCapture]::SetThreadDpiAwarenessContext($oldDpi) | Out-Null }
} finally { if (-not $LockHeld) { [IO.Directory]::Delete($lock) } }
