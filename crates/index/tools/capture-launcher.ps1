# Capture the deployed launcher. Input is sent only to verified mypaper windows under the
# shared desktop lock; no user window is minimized, moved, closed, or typed into.
param([string]$Query = 'folder:wallpaper', [string]$Out = 'D:\Coding\projects\desktop\wallpaper\crates\index\evidence\launcher-index.png')
$ErrorActionPreference = 'Stop'
$lock = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
New-Item -ItemType Directory -Path $lock -ErrorAction Stop | Out-Null
try {
  Add-Type -AssemblyName System.Drawing, System.Windows.Forms, UIAutomationClient, UIAutomationTypes
  Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Diagnostics;
using System.Text;
public static class IndexCapture {
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x,y; }
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left,top,right,bottom; }
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT point);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder s,int max);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 public static bool Ours(IntPtr h) {uint pid; GetWindowThreadProcessId(h,out pid); try{return Process.GetProcessById((int)pid).ProcessName.StartsWith("mypaper-");}catch{return false;}}
 public static IntPtr Flyout(int pid) {IntPtr found=IntPtr.Zero; EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);var title=new StringBuilder(256);GetWindowText(h,title,256);if(owner==pid&&IsWindowVisible(h)&&title.ToString()=="Flyout"){found=h;return false;}return true;},IntPtr.Zero);return found;}
}
'@
  [IndexCapture]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null
  $shell = Get-Process mypaper-shell -ErrorAction Stop | Select-Object -First 1
  $condition = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty, $shell.Id)
  $starts = @()
  foreach ($window in [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, $condition)) {
    foreach ($element in $window.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)) {
      if ($element.Current.Name -eq 'Start' -and -not $element.Current.IsOffscreen) { $starts += $element }
    }
  }
  if ($starts.Count -eq 0) { throw 'No visible mypaper Start control' }
  $start = $starts[0]
  $bounds = $start.Current.BoundingRectangle
  $point = New-Object IndexCapture+POINT
  $point.x = [int]($bounds.X + $bounds.Width/2); $point.y = [int]($bounds.Y + $bounds.Height/2)
  if (-not [IndexCapture]::Ours([IndexCapture]::WindowFromPoint($point))) { throw 'Start point is not a mypaper window; input skipped' }
  # UIA Invoke targets the returned mypaper control directly, without global keyboard input.
  $invoke = $start.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)
  $invoke.Invoke()
  Start-Sleep -Milliseconds 650
  $flyout = [IndexCapture]::Flyout($shell.Id)
  if ($flyout -eq [IntPtr]::Zero -or -not [IndexCapture]::Ours($flyout)) { throw 'Launcher did not open' }
  if ([IndexCapture]::GetForegroundWindow() -ne $flyout) { throw 'Launcher is not foreground; typing skipped' }
  # Windowless RichEdit receives its owning surface's WM_CHAR. Every character is targeted
  # to the verified launcher HWND; recheck foreground before each one.
  foreach ($character in $Query.ToCharArray()) {
    if ([IndexCapture]::GetForegroundWindow() -ne $flyout) { throw 'Foreground changed; typing stopped' }
    [IndexCapture]::SendMessage($flyout, 0x102, [IntPtr]::new([int]$character), [IntPtr]::Zero) | Out-Null
  }
  Start-Sleep -Milliseconds 750
  $rect = New-Object IndexCapture+RECT
  [IndexCapture]::GetWindowRect($flyout, [ref]$rect) | Out-Null
  $image = New-Object Drawing.Bitmap ($rect.right-$rect.left), ($rect.bottom-$rect.top)
  try {
    $graphics = [Drawing.Graphics]::FromImage($image)
    try { $graphics.CopyFromScreen($rect.left,$rect.top,0,0,$image.Size) } finally { $graphics.Dispose() }
    $image.Save($Out,[Drawing.Imaging.ImageFormat]::Png)
  } finally { $image.Dispose() }
  $root = [Windows.Automation.AutomationElement]::FromHandle($flyout)
  $rows = @($root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) | ForEach-Object { $_.Current.Name })
  [pscustomobject]@{ query=$Query; screenshot=$Out; visible_text=$rows } | ConvertTo-Json -Depth 4
  if ([IndexCapture]::GetForegroundWindow() -eq $flyout) { [IndexCapture]::SendMessage($flyout,0x100,[IntPtr]::new(27),[IntPtr]::Zero) | Out-Null }
} finally {
  if ([IO.Path]::GetFullPath($lock) -ne 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock') { throw 'Invalid desktop lock' }
  [IO.Directory]::Delete($lock)
}
