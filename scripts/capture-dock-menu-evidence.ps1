param([Parameter(Mandatory=$true)][string]$Folder)
$ErrorActionPreference = 'Stop'
# Read-only: PrintWindow and UIA are restricted to our no-activate fixture's process.
Add-Type -AssemblyName System.Drawing, UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class GlazeMenuCapture {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtr(IntPtr h, int index);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point p);
}
'@
$state = Get-Content -LiteralPath (Join-Path $Folder 'ready.json') -Raw | ConvertFrom-Json
$fixtureProcess = Get-Process -Id $state.pid
if ($fixtureProcess.ProcessName -ne 'dock_menu_evidence') { throw 'Not our evidence process' }
$canvas = New-Object Drawing.Bitmap 960,600
$graphics = [Drawing.Graphics]::FromImage($canvas)
$graphics.Clear([Drawing.Color]::Gray)
$windows = @($state.backdrop,$state.frost,$state.menu,$state.submenu) | Where-Object { $_ }
$report = @()
foreach ($handle in $windows) {
  $hwnd = [IntPtr][long]$handle
  $owner = [uint32]0
  [void][GlazeMenuCapture]::GetWindowThreadProcessId($hwnd,[ref]$owner)
  if ($owner -ne $state.pid) { throw 'Capture refused: another process owns this window' }
  $rect = New-Object GlazeMenuCapture+Rect
  [void][GlazeMenuCapture]::GetWindowRect($hwnd,[ref]$rect)
  $bitmap = New-Object Drawing.Bitmap ($rect.Right-$rect.Left),($rect.Bottom-$rect.Top)
  $g = [Drawing.Graphics]::FromImage($bitmap)
  $dc = $g.GetHdc()
  try { $printed = [GlazeMenuCapture]::PrintWindow($hwnd,$dc,2) } finally { $g.ReleaseHdc($dc); $g.Dispose() }
  $graphics.DrawImageUnscaled($bitmap,($rect.Left-$state.rect[0]),($rect.Top-$state.rect[1]))
  $bitmap.Save((Join-Path $Folder "window-$handle.png"),[Drawing.Imaging.ImageFormat]::Png)
  $bitmap.Dispose()
  $element = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
  $noActivate = ([GlazeMenuCapture]::GetWindowLongPtr($hwnd,-20).ToInt64() -band 0x08000000) -ne 0
  if (!$noActivate) { throw 'Fixture must remain no-activate' }
  $report += @{ process=$owner; printed=$printed; noActivate=$noActivate; role=$element.Current.ControlType.ProgrammaticName; fixture=$element.Current.Name }
}
$graphics.Dispose()
$canvas.Save((Join-Path $Folder 'dock-menu-printwindow.png'),[Drawing.Imaging.ImageFormat]::Png)
$canvas.Dispose()
# PrintWindow cannot read DirectComposition pixels. The visual proof crops only the opaque
# fixture rectangle. Refuse a capture if any point belongs to a different process; never save
# another app's content. UIA/PrintWindow above provide the independent ownership/role checks.
$proof = New-Object Drawing.Bitmap 960,600
$proofGraphics = [Drawing.Graphics]::FromImage($proof)
$proofGraphics.Clear([Drawing.Color]::Gray)
try {
  for ($y=0; $y -lt 600; $y+=8) {
    for ($x=0; $x -lt 960; $x+=8) {
      $point = New-Object GlazeMenuCapture+Point
      $point.X = $state.rect[0]+$x; $point.Y = $state.rect[1]+$y
      $owner = [uint32]0
      [void][GlazeMenuCapture]::GetWindowThreadProcessId([GlazeMenuCapture]::WindowFromPoint($point),[ref]$owner)
      if ($owner -ne $state.pid) { throw 'Visual proof refused: another window covers our fixture' }
    }
  }
  $proofGraphics.CopyFromScreen([int]$state.rect[0],[int]$state.rect[1],0,0,$proof.Size)
  $proof.Save((Join-Path $Folder 'dock-menu-live.png'),[Drawing.Imaging.ImageFormat]::Png)
} finally { $proofGraphics.Dispose(); $proof.Dispose() }
$report | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $Folder 'printwindow-verification.json')
$report | ConvertTo-Json -Depth 4
