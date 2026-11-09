# Finding 4: flip the rules files fast (each flip queues restyles on every UI thread), then turn
# experimental.app_styler off right away, while those restyles are still landing. Every session
# must put everything back (its clean-up runs after the in-flight work) and detach; then the flag
# goes back on. Holds the desktop lock for the test (about 20 s).
param([string]$Out = "C:\glaze-targets\app-styler\round3\flag-off.txt")
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'glaze\app-styler'
$config = Join-Path $env:APPDATA 'glaze\config.json'
$tapLog = Join-Path $dir 'styler.log'
$lock = (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$utf8 = New-Object Text.UTF8Encoding($false)
function Note([string]$m) { $l = '{0} {1}' -f (Get-Date -Format 'HH:mm:ss.fff'), $m; Add-Content -LiteralPath $Out -Value $l; Write-Host $l }
function Write-Atomic([string]$path, [string]$text) { $t = "$path.f4.tmp"; [IO.File]::WriteAllText($t, $text, $utf8); Move-Item -LiteralPath $t -Destination $path -Force }
function Set-Flag([bool]$on) {
  $text = [IO.File]::ReadAllText($config)
  $new = if ($on) { $text -replace '"app_styler":\s*false', '"app_styler": true' } else { $text -replace '"app_styler":\s*true', '"app_styler": false' }
  if ($new -eq $text) { throw "flag already $on" }
  Write-Atomic $config $new
}

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class F4Shot {
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
}
'@
[F4Shot]::SetProcessDPIAware() | Out-Null
# PrintWindow (full content) of a folder window: works while covered, never activates it.
function Shot([int64]$hwnd, [string]$png) {
  $h = [IntPtr]$hwnd; $r = New-Object F4Shot+R; [F4Shot]::GetWindowRect($h, [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap ([Math]::Max(1, $r.Rt - $r.L)), ([Math]::Max(1, [Math]::Min(260, $r.B - $r.T)))
  $g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
  [F4Shot]::PrintWindow($h, $hdc, 2) | Out-Null; $g.ReleaseHdc($hdc); $bmp.Save($png); $g.Dispose(); $bmp.Dispose()
}
$original = @{}
foreach ($app in 'explorer', 'settings') { $original[$app] = [IO.File]::ReadAllText((Join-Path $dir "$app.json")) }
for ($i = 0; -not (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue); $i++) {
  if ($i -gt 240) { throw 'desktop lock busy' }; Start-Sleep -Milliseconds 500
}
$win = $null
try {
  $shell = New-Object -ComObject Shell.Application
  $before = @($shell.Windows() | ForEach-Object { [int64]$_.HWND })
  $shell.Open('C:\Windows'); Start-Sleep -Seconds 2
  $win = $shell.Windows() | Where-Object { $before -notcontains [int64]$_.HWND } | Select-Object -First 1
  $styler = Get-Process glaze-styler
  Note "styler $($styler.Id); explorer + settings styled; our window $([int64]$win.HWND)"
  if ($win) { Shot ([int64]$win.HWND) 'C:\glaze-targets\app-styler\round3\f4-styled.png' }
  # Show Settings so its UI thread is awake (a suspended app runs nothing).
  Start-Process 'ms-settings:display'; Start-Sleep -Seconds 2
  $mark = (Get-Content -LiteralPath $tapLog).Count
  $variant = { param($t) $t.Replace('"16"', '"6"').Replace('"10"', '"7"').Replace('Transparent', '#08808080') }
  foreach ($k in 1..11) {
    foreach ($app in 'explorer', 'settings') {
      Write-Atomic (Join-Path $dir "$app.json") $(if ($k % 2) { & $variant $original[$app] } elseif ($k % 3) { '{}' } else { $original[$app] })
    }
    Start-Sleep -Milliseconds 60
  }
  Note '11 rules flips in ~1 s (the last one: other values); flag off now'
  Set-Flag $false
  $t0 = Get-Date
  while ((Get-Process -Id $styler.Id -ErrorAction SilentlyContinue) -and ((Get-Date) - $t0).TotalSeconds -lt 15) { Start-Sleep -Milliseconds 50 }
  Note ("styler gone after {0:N0} ms: {1}" -f ((Get-Date) - $t0).TotalMilliseconds, -not (Get-Process -Id $styler.Id -ErrorAction SilentlyContinue))
  Start-Sleep -Seconds 4
  if ($win) { Shot ([int64]$win.HWND) 'C:\glaze-targets\app-styler\round3\f4-off.png' }
  Note '--- TAP log since the flips'
  Get-Content -LiteralPath $tapLog | Select-Object -Skip $mark | Where-Object { $_ -match 'restoring|put back|detached|clean|forgot|gone|abandon|not restored|leaving' } | ForEach-Object { Note "  $_" }
} finally {
  # The helper wrote nothing while off; put the rules files back as they were, then the flag.
  foreach ($app in 'explorer', 'settings') { Write-Atomic (Join-Path $dir "$app.json") $original[$app] }
  try { Set-Flag $true } catch { Note "flag: $_" }
  Start-Sleep -Seconds 4
  $new = Get-Process glaze-styler -ErrorAction SilentlyContinue
  Note "flag back on; styler $(if ($new) { $new.Id } else { 'NOT RUNNING' })"
  if ($win) { Shot ([int64]$win.HWND) 'C:\glaze-targets\app-styler\round3\f4-on-again.png'; $win.Quit() }
  if (Test-Path -LiteralPath $lock) { Remove-Item -LiteralPath $lock -Recurse -Force }
}
