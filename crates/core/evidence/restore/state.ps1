# Read-only snapshot of the system state Glaze changes. Prints one JSON object.
param([string]$Label = "state", [int]$ProbePid = 0)
$ErrorActionPreference = "Stop"
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class GP {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string w);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
  [DllImport("user32.dll")] static extern bool GetLayeredWindowAttributes(IntPtr h, out uint k, out byte a, out uint f);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b; public override string ToString() { return l+","+t+","+r+","+b; } }
  [StructLayout(LayoutKind.Sequential)] struct MI { public int cb; public RECT mon; public RECT work; public uint flags; }
  delegate bool MonProc(IntPtr m, IntPtr dc, ref RECT r, IntPtr d);
  [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr dc, IntPtr clip, MonProc p, IntPtr d);
  [DllImport("user32.dll")] static extern bool GetMonitorInfoW(IntPtr m, ref MI mi);
  [StructLayout(LayoutKind.Sequential)] struct ABD { public int cb; public IntPtr h; public uint msg; public uint edge; public RECT rc; public IntPtr lp; }
  [DllImport("shell32.dll")] static extern IntPtr SHAppBarMessage(uint m, ref ABD d);
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int n);
  public static List<string> Taskbars() {
    var o = new List<string>();
    foreach (var cls in new[]{"Shell_TrayWnd","Shell_SecondaryTrayWnd"}) {
      IntPtr h = IntPtr.Zero;
      while ((h = FindWindowExW(IntPtr.Zero, h, cls, null)) != IntPtr.Zero) {
        uint pid; GetWindowThreadProcessId(h, out pid);
        string name; try { name = System.Diagnostics.Process.GetProcessById((int)pid).ProcessName; } catch { name = "?"; }
        long ex = GetWindowLongPtrW(h, -20).ToInt64();
        uint k, f; byte a = 255; bool layered = GetLayeredWindowAttributes(h, out k, out a, out f);
        RECT r; GetWindowRect(h, out r);
        o.Add(cls+" "+name+" visible="+IsWindowVisible(h)+" ex=0x"+ex.ToString("x")+" alpha="+(layered && (f&2)!=0 ? a.ToString() : "none")+" rect="+r);
      }
    }
    return o;
  }
  public static List<string> WorkAreas() {
    var o = new List<string>();
    EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, delegate(IntPtr m, IntPtr dc, ref RECT r, IntPtr d) {
      var mi = new MI(); mi.cb = Marshal.SizeOf(typeof(MI)); GetMonitorInfoW(m, ref mi);
      o.Add("monitor="+mi.mon+" work="+mi.work); return true; }, IntPtr.Zero);
    return o;
  }
  public static uint AppBarState() { var d = new ABD(); d.cb = Marshal.SizeOf(typeof(ABD)); return (uint)SHAppBarMessage(4, ref d).ToInt64(); }
  public static string Dwm(uint pid) {
    var sb = new StringBuilder();
    foreach (var p in System.Diagnostics.Process.GetProcessById((int)pid).Threads) {}
    IntPtr h = System.Diagnostics.Process.GetProcessById((int)pid).MainWindowHandle;
    foreach (var a in new[]{33,34,35,36,38,20}) { int v; int hr = DwmGetWindowAttribute(h, a, out v, 4); sb.Append(a+"="+(hr==0? "0x"+v.ToString("x") : "hr0x"+hr.ToString("x"))+" "); }
    return sb.ToString().Trim();
  }
}
"@
function Reg($path, $name) { try { (Get-ItemProperty -Path $path -Name $name -ErrorAction Stop).$name } catch { "<absent>" } }
$local = Join-Path $env:LOCALAPPDATA "glaze"
$journals = @{}
foreach ($j in "taskbar.json","undo.json","undo-system.json","undo-shell.json","undo-folders.json","wallpaper.json") {
  $p = Join-Path $local $j
  $journals[$j] = if (Test-Path $p) { (Get-FileHash $p -Algorithm SHA256).Hash.Substring(0,12) } else { "absent" }
}
$state = [ordered]@{
  label = $Label
  time = (Get-Date).ToString("HH:mm:ss.fff")
  glaze = @(Get-CimInstance Win32_Process -Filter "Name like 'glaze-%'" | Where-Object { $_.ExecutablePath -like "*target\release*" -or $_.ExecutablePath -like "*glaze-targets\safety*" } | ForEach-Object { "$($_.Name)#$($_.ProcessId)" })
  taskbars = [GP]::Taskbars()
  appbar_state = [GP]::AppBarState()
  taskbar_al = Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" "TaskbarAl"
  work_areas = [GP]::WorkAreas()
  cursor_scheme = Reg "HKCU:\Control Panel\Cursors" "(default)"
  cursor_arrow = Reg "HKCU:\Control Panel\Cursors" "Arrow"
  accent = Reg "HKCU:\Software\Microsoft\Windows\DWM" "AccentColor"
  apps_light = Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" "AppsUseLightTheme"
  hide_file_ext = Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" "HideFileExt"
  journals = $journals
}
if ($ProbePid) { $state.probe_dwm = [GP]::Dwm([uint32]$ProbePid) }
$state | ConvertTo-Json -Depth 4 -Compress
