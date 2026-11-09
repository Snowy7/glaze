# Read-only capture of the deployed calendar widget. Never activates a window or injects input.
param([string]$Out = "$env:TEMP\glaze-calendar-live.png")
$ErrorActionPreference = 'Stop'
$calendarCaptureLock = (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$calendarCaptureOwned = $false
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
public static class CalendarCapture {
  [StructLayout(LayoutKind.Sequential)] struct Rect { public int L,T,R,B; }
  delegate bool WindowProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumWindows(WindowProc fn,IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent,WindowProc fn,IntPtr p);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder text,int length);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h,out Rect r);
  [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
  [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  public static string Save(int[] processes,string path) {
    SetThreadDpiAwarenessContext(new IntPtr(-4));
    var candidates=new List<IntPtr>();
    WindowProc collect=(h,p)=>{uint id;GetWindowThreadProcessId(h,out id);if(Array.IndexOf(processes,(int)id)<0)return true;var title=new StringBuilder(512);GetWindowText(h,title,512);if(title.ToString().StartsWith("Glaze widget calendar"))candidates.Add(h);return true;};
    EnumWindows((h,p)=>{collect(h,p);EnumChildWindows(h,collect,IntPtr.Zero);return true;},IntPtr.Zero);
    foreach(var h in candidates){Rect r;if(!GetWindowRect(h,out r)||r.R-r.L<40||r.B-r.T<40)continue;
      using(var bmp=new Bitmap(r.R-r.L,r.B-r.T))using(var graphics=Graphics.FromImage(bmp)){
        var dc=graphics.GetHdc();bool captured=PrintWindow(h,dc,2);graphics.ReleaseHdc(dc);
        if(!captured)continue;bmp.Save(path,ImageFormat.Png);return (r.R-r.L)+"x"+(r.B-r.T)+" "+path;
      }
    }
    throw new InvalidOperationException("No deployed calendar widget is available to capture; user windows were left untouched.");
  }
}
'@
try {
    New-Item -ItemType Directory -Path $calendarCaptureLock -ErrorAction Stop | Out-Null
    $calendarCaptureOwned = $true
    $calendarCaptureIds = @(Get-Process glaze-shell -ErrorAction Stop | ForEach-Object { $_.Id })
    [CalendarCapture]::Save([int[]]$calendarCaptureIds, [IO.Path]::GetFullPath($Out))
} finally {
    if ($calendarCaptureOwned) { [IO.Directory]::Delete($calendarCaptureLock) }
}
