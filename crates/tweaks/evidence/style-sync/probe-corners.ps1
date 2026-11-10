# Cross-process corner preference probe. Never changes config, registry, focus or geometry.
# Each window's preference is restored in finally, even when Windows rejects a write.
param([string]$Out = "$PSScriptRoot\corner-probe.json")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class CornerProbe {
  public delegate bool Visit(IntPtr hwnd, IntPtr data);
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct Point { public int X,Y; public Point(int x,int y) { X=x; Y=y; } }
  [DllImport("user32.dll")] public static extern bool EnumWindows(Visit visit, IntPtr data);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr hwnd, StringBuilder text, int length);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int length);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr hwnd, uint flag);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hwnd, uint attr, out uint value, uint size);
  [DllImport("dwmapi.dll", EntryPoint="DwmGetWindowAttribute")] public static extern int GetBounds(IntPtr hwnd, uint attr, out Rect value, uint size);
  [DllImport("dwmapi.dll")] public static extern int DwmSetWindowAttribute(IntPtr hwnd, uint attr, ref uint value, uint size);
  public static IntPtr[] Windows() {
    var windows = new List<IntPtr>();
    EnumWindows(delegate(IntPtr hwnd, IntPtr data) {
      if (!IsWindowVisible(hwnd)) return true;
      var cls = new StringBuilder(128); var title = new StringBuilder(512);
      GetClassName(hwnd, cls, cls.Capacity); GetWindowText(hwnd, title, title.Capacity);
      if (cls.ToString() == "CabinetWClass" || title.ToString() == "Settings") windows.Add(hwnd);
      return true;
    }, IntPtr.Zero);
    return windows.ToArray();
  }
}
'@
$lock = (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$config = Join-Path $env:APPDATA 'glaze\config.json'
$configBefore = (Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash
$foreground = [CornerProbe]::GetForegroundWindow()
$results = @()
$dpi = [IntPtr]::Zero
New-Item -ItemType Directory -Path $lock -ErrorAction Stop | Out-Null
try {
  $dpi = [CornerProbe]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
  foreach ($handle in [CornerProbe]::Windows()) {
    [uint32]$original = 0; [uint32]$pidValue = 0
    $readBefore = [CornerProbe]::DwmGetWindowAttribute($handle, 33, [ref]$original, 4)
    [CornerProbe]::GetWindowThreadProcessId($handle, [ref]$pidValue) | Out-Null
    $process = Get-Process -Id $pidValue
    $attempts = @()
    $restore = $null
    if ($readBefore -eq 0) {
      try {
        foreach ($requested in @(1, 2, 3)) {
          [uint32]$value = $requested; [uint32]$actual = 0
          $write = [CornerProbe]::DwmSetWindowAttribute($handle, 33, [ref]$value, 4)
          $read = [CornerProbe]::DwmGetWindowAttribute($handle, 33, [ref]$actual, 4)
          Start-Sleep -Milliseconds 60
          $bounds = New-Object CornerProbe+Rect
          $shot = $null
          $exposed = $false
          if ([CornerProbe]::GetBounds($handle, 9, [ref]$bounds, 16) -eq 0) {
            $hit = [CornerProbe]::WindowFromPoint([CornerProbe+Point]::new($bounds.Right - 12, $bounds.Top + 12))
            $exposed = [CornerProbe]::GetAncestor($hit, 2) -eq $handle
          }
          if ($exposed) {
            $shot = Join-Path (Split-Path -Parent $Out) "corner-$($process.ProcessName)-$($handle.ToInt64())-$requested.png"
            $bitmap = New-Object Drawing.Bitmap 100, 80
            $graphics = [Drawing.Graphics]::FromImage($bitmap)
            try {
              $graphics.CopyFromScreen($bounds.Right - 60, $bounds.Top - 10, 0, 0, [Drawing.Size]::new(100,80))
              $bitmap.Save($shot)
            } finally { $graphics.Dispose(); $bitmap.Dispose() }
          }
          $attempts += [pscustomobject]@{requested=$requested;set_hresult=('0x{0:X8}' -f $write);read_hresult=('0x{0:X8}' -f $read);actual=$actual;corner_exposed=$exposed;screenshot=if ($shot) {Split-Path -Leaf $shot} else {$null}}
        }
      } finally {
        $restoreWrite = [CornerProbe]::DwmSetWindowAttribute($handle, 33, [ref]$original, 4)
        [uint32]$restored = 0
        $restoreRead = [CornerProbe]::DwmGetWindowAttribute($handle, 33, [ref]$restored, 4)
        $restore = [pscustomobject]@{set_hresult=('0x{0:X8}' -f $restoreWrite);read_hresult=('0x{0:X8}' -f $restoreRead);actual=$restored;matches_original=($restoreRead -eq 0 -and $restored -eq $original)}
      }
    }
    $results += [pscustomobject]@{hwnd=$handle.ToInt64();process=$process.ProcessName;pid=$pidValue;maximized=[CornerProbe]::IsZoomed($handle);original=$original;read_before_hresult=('0x{0:X8}' -f $readBefore);attempts=$attempts;restore=$restore}
  }
} finally {
  if ($dpi -ne [IntPtr]::Zero) { [CornerProbe]::SetThreadDpiAwarenessContext($dpi) | Out-Null }
  # Literal, known directory; never remove someone else's lock.
  Remove-Item -LiteralPath $lock
}
$report = [pscustomobject]@{config_sha256_before=$configBefore;config_sha256_after=(Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash;foreground_before=$foreground.ToInt64();foreground_after=[CornerProbe]::GetForegroundWindow().ToInt64();windows=$results}
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $Out -Encoding utf8
$report | ConvertTo-Json -Depth 8
