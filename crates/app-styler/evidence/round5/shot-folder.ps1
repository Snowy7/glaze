# Round 5: one styled folder window (in the folder process), captured with PrintWindow (never
# activates it), then closed.   shot-folder.ps1 -Out png [-Path folder]
param([Parameter(Mandatory)][string]$Out, [string]$Path = 'C:\Windows')
. "$PSScriptRoot\lib.ps1"
$script:Log = [IO.Path]::ChangeExtension($Out, '.log')
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class R5Shot {
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
}
'@
[R5Shot]::SetProcessDPIAware() | Out-Null
Take-Lock
try {
  $w = Open-Folder $Path
  Start-Sleep -Seconds 5
  $h = [IntPtr](Key $w)
  $r = New-Object R5Shot+R; [R5Shot]::GetWindowRect($h, [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap ($r.Rt - $r.L), ($r.B - $r.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
  [R5Shot]::PrintWindow($h, $hdc, 2) | Out-Null; $g.ReleaseHdc($hdc)
  $bmp.Save($Out)
  Note "shot of window $h in explorer $(Owner (Key $w)) (shell $(Shell-Pid)): $Out"
  $w.Quit()
} finally { Drop-Lock }
