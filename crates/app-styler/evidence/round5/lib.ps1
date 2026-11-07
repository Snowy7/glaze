# App styler round 5: shared helpers for the live proofs. Dot-source it.
# No synthetic input: windows open through ShellExecute with SW_SHOWNOACTIVATE (no focus change),
# move to the secondary screen without activation, navigate with Shell COM and close with Quit()
# or WM_CLOSE. The desktop lock is held per action only.
$ErrorActionPreference = 'Stop'
$lock = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
Add-Type -Namespace R5 -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, out uint pid);
[DllImport("user32.dll")] public static extern bool SetWindowPos(System.IntPtr h, System.IntPtr after, int x, int y, int cx, int cy, uint flags);
[DllImport("user32.dll")] public static extern bool PostMessage(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l);
[DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool IsWindow(System.IntPtr h);
[DllImport("user32.dll")] public static extern System.IntPtr GetShellWindow();
[DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr h, int cmd);
'@ -ErrorAction SilentlyContinue

function Append-Line([string]$file, [string]$line) {
  for ($i = 0; $i -lt 20; $i++) { try { [IO.File]::AppendAllText($file, $line + "`r`n"); break } catch { Start-Sleep -Milliseconds 100 } }
  Write-Host $line
}
function Note([string]$m) { Append-Line $script:Log ('{0} {1}' -f (Get-Date -Format 'HH:mm:ss'), $m) }
Add-Type -Namespace R5i -Name Win -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct LII { public uint cbSize; public uint dwTime; }
[DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LII p);
public static double IdleSeconds() { var l = new LII(); l.cbSize = 8; GetLastInputInfo(ref l); return unchecked((uint)System.Environment.TickCount - l.dwTime) / 1000.0; }
'@ -ErrorAction SilentlyContinue
# Our windows can take the focus as they open: only open them while the user isn't typing or
# moving the mouse (45 s without input), so nothing they type lands in one.
function Wait-Idle {
  $waited = 0
  while ([R5i.Win]::IdleSeconds() -lt 45) { Start-Sleep -Seconds 3; $waited += 3 }
  if ($waited) { Note "  (waited $waited s for the user to be idle)" }
}
function Take-Lock {
  Wait-Idle
  for ($i = 0; $i -lt 240; $i++) {
    if (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue) { return }
    Start-Sleep -Milliseconds 500
  }
  throw 'desktop lock busy for 2 minutes'
}
function Drop-Lock { if (Test-Path -LiteralPath $lock) { Remove-Item -LiteralPath $lock -Recurse -Force } }
function Owner([int64]$hwnd) { $id = [uint32]0; [void][R5.Win]::GetWindowThreadProcessId([IntPtr]$hwnd, [ref]$id); [int]$id }
function Fg-Pid { Owner ([int64][R5.Win]::GetForegroundWindow()) }
function Shell-Pid { Owner ([int64][R5.Win]::GetShellWindow()) }
function Mb($p) { '{0:N1}' -f ($p.PrivateMemorySize64 / 1MB) }
function Sample-Explorers([string]$what) {
  $parts = foreach ($p in (Get-Process explorer -ErrorAction SilentlyContinue | Sort-Object Id)) {
    $tag = if ($p.Id -eq (Shell-Pid)) { 'shell' } else { 'folder' }
    '{0} {1}: {2} MB priv, {3} handles' -f $tag, $p.Id, (Mb $p), $p.HandleCount
  }
  Note "[$what] $($parts -join ' | ')"
}

$shell = New-Object -ComObject Shell.Application
function Shell-Windows { @($shell.Windows() | Where-Object { $_ }) }
function Key($w) { [int64]$w.HWND }
# Opens `path` in a new folder window without activating it, moves it to the secondary screen,
# and returns its Shell COM object (or $null).
function Open-Folder([string]$path, [int]$slot = 0) {
  $before = @(Shell-Windows | ForEach-Object { Key $_ })
  $fg = [R5.Win]::GetForegroundWindow()
  $shell.ShellExecute('explorer.exe', $path, '', 'open', 7)  # SW_SHOWMINNOACTIVE
  for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Milliseconds 100
    $new = Shell-Windows | Where-Object { $before -notcontains (Key $_) } | Select-Object -First 1
    if ($new) {
      $h = [IntPtr](Key $new)
      # Explorer may activate its new window anyway: minimizing it activates the next window
      # down, which is the one that had the focus.
      if ([R5.Win]::GetForegroundWindow() -eq $h) { [void][R5.Win]::ShowWindow($h, 6) }  # SW_MINIMIZE
      [void][R5.Win]::ShowWindow($h, 4)  # SW_SHOWNOACTIVATE: restored, not activated
      # Secondary screen (x -2560..0), no activation, no z-order change.
      [void][R5.Win]::SetWindowPos($h, [IntPtr]::Zero, -2400 + 60 * $slot, 80 + 60 * $slot, 1300, 850, 0x0010 -bor 0x0004)
      $script:FocusKept = ([R5.Win]::GetForegroundWindow() -eq $fg)
      return $new
    }
  }
  return $null
}
$folders = 'C:\Windows', 'C:\Windows\System32', 'C:\Program Files', 'C:\Users\Public',
  'shell:::{f874310e-b6b7-47dc-bc84-b9e6b38f5903}', 'shell:::{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}',
  'C:\Windows\Fonts', 'C:\mypaper-targets\app-styler'
$rand = New-Object Random 7
