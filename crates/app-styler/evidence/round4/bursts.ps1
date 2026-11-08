# App styler round 4: open/navigate/close bursts in ONE Explorer process (a separate one made with
# `explorer /separate`, or the shell's), sampling that process's private bytes and handles after
# each burst. No synthetic input: Shell COM, and UI Automation Invoke on the anchor window's tab
# strip for tabs (-Windows: new windows, which Shell COM opens in the shell Explorer). The desktop
# lock is held per burst only.
#
#   bursts.ps1 -Target <explorer pid> -Bursts 10 -Log file [-Pause 30] [-Tag name]
#   [-BeforeBurst {script}] [-AfterBurst {script}]   hooks (e.g. attach / end a session)
param([Parameter(Mandatory)][int]$Target, [int]$Bursts = 10, [Parameter(Mandatory)][string]$Log,
      [int]$Pause = 30, [string]$Tag = '', [scriptblock]$BeforeBurst, [scriptblock]$AfterBurst,
      [int]$Settle = 6, [switch]$Windows)
$ErrorActionPreference = 'Stop'
# Appends a line (retrying while a reader has the file open) and echoes it.
function Append-Line([string]$file, [string]$line) {
  for ($i = 0; $i -lt 20; $i++) { try { [IO.File]::AppendAllText($file, $line + "`r`n"); break } catch { Start-Sleep -Milliseconds 100 } }
  Write-Host $line
}
$lock = 'C:\Users\you\AppData\Local\Temp\glaze-tools\desktop.lock'

Add-Type -Namespace R4 -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr hwnd, out uint pid);
'@ -ErrorAction SilentlyContinue

function Note([string]$m) {
  $line = '{0} {1}' -f (Get-Date -Format 'HH:mm:ss'), $m
  Append-Line $Log $line
}
function Sample([string]$what) {
  $p = Get-Process -Id $Target -ErrorAction SilentlyContinue
  if (-not $p) { Note "[$what] explorer $Target is gone"; return }
  $s = Get-Process glaze-styler -ErrorAction SilentlyContinue | Select-Object -First 1
  $styler = if ($s) { ' | styler {0}: {1:N1} MB priv' -f $s.Id, ($s.PrivateMemorySize64 / 1MB) } else { '' }
  Note ('[{0}] explorer {1}: {2:N1} MB priv, {3:N1} MB ws, {4} handles, {5:N2} s cpu{6}' -f $what, $Target,
    ($p.PrivateMemorySize64 / 1MB), ($p.WorkingSet64 / 1MB), $p.HandleCount, $p.TotalProcessorTime.TotalSeconds, $styler)
}
function Take-Lock {
  for ($i = 0; $i -lt 240; $i++) {
    if (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue) { return }
    Start-Sleep -Milliseconds 500
  }
  throw 'desktop lock busy for 2 minutes'
}
function Drop-Lock { if (Test-Path -LiteralPath $lock) { Remove-Item -LiteralPath $lock -Recurse -Force } }

$shell = New-Object -ComObject Shell.Application
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
function Owner($w) { try { $id = [uint32]0; [void][R4.Win]::GetWindowThreadProcessId([IntPtr][int64]$w.HWND, [ref]$id); $id } catch { 0 } }
function Mine { @($shell.Windows() | Where-Object { (Owner $_) -eq $Target }) }
# COM identity of a tab or window object (the same remote object gives the same pointer).
function Key($w) { $p = [Runtime.InteropServices.Marshal]::GetIUnknownForObject($w); [void][Runtime.InteropServices.Marshal]::Release($p); [int64]$p }
function Id-Is([string]$id) { New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::AutomationIdProperty, $id) }
# Within the tab strip only: a search from the window would walk (and make Explorer build
# automation peers for) the whole window, file list included.
function By-Id($tabs, [string]$id) { $tabs.FindAll([Windows.Automation.TreeScope]::Descendants, (Id-Is $id)) }
function Invoke-Element($e) { $e.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke() }
# The anchor window's tab strip (TabView, in the title bar), found once.
$script:tabStrip = $null
function Anchor {
  if (-not $script:tabStrip) {
    $window = [Windows.Automation.AutomationElement]::FromHandle([IntPtr][int64]((Mine | Select-Object -First 1).HWND))
    $script:tabStrip = $window.FindFirst([Windows.Automation.TreeScope]::Descendants, (Id-Is 'TabView'))
  }
  $script:tabStrip
}
# A new tab in the anchor window (its tab strip's "Add New Tab"): the tab's browser object.
function Open-Tab {
  $before = @(Mine | ForEach-Object { Key $_ })
  $add = By-Id (Anchor) 'AddButton' | Select-Object -First 1
  if (-not $add) { return $null }
  Invoke-Element $add
  for ($i = 0; $i -lt 40; $i++) {
    Start-Sleep -Milliseconds 150
    $new = Mine | Where-Object { $before -notcontains (Key $_) } | Select-Object -First 1
    if ($new) { return $new }
  }
  return $null
}
# Closes every tab of the anchor window but the first (its "Close Tab" buttons).
function Close-Extra-Tabs {
  $close = @(By-Id (Anchor) 'CloseButton')
  foreach ($c in ($close | Select-Object -Skip 1)) { try { Invoke-Element $c; Start-Sleep -Milliseconds 300 } catch { } }
  return [Math]::Max(0, $close.Count - 1)
}
$folders = 'C:\Windows', 'C:\Windows\System32', 'C:\Program Files', 'C:\Users\Public',
  'shell:::{f874310e-b6b7-47dc-bc84-b9e6b38f5903}', 'shell:::{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}',
  'C:\Windows\Fonts', 'C:\glaze-targets\app-styler\round4'
$r = New-Object Random 7

# A new folder window, by Shell COM: it opens in the shell Explorer (so -Windows targets that).
function Open-Window([string]$path) {
  $before = @($shell.Windows() | ForEach-Object { Key $_ })
  $shell.Open($path)
  for ($i = 0; $i -lt 40; $i++) {
    Start-Sleep -Milliseconds 150
    $new = Mine | Where-Object { $before -notcontains (Key $_) } | Select-Object -First 1
    if ($new) { return $new }
  }
  return $null
}

# Two tabs (or, with -Windows, two windows) opened, each navigated 6 times, then closed.
function Burst([int]$n) {
  Take-Lock
  $opened = @()
  $closed = 0
  try {
    $t0 = Get-Date
    foreach ($k in 1..2) {
      $w = if ($Windows) { Open-Window $folders[$r.Next($folders.Count)] } else { Open-Tab }
      if ($w) { $opened += $w } else { Note '  nothing opened' }
    }
    foreach ($k in 1..6) {
      foreach ($w in $opened) { try { $w.Navigate2($folders[$r.Next($folders.Count)]) } catch { } }
      Start-Sleep -Milliseconds (400 + $r.Next(600))
    }
    if ($Windows) { foreach ($w in $opened) { try { $w.Quit(); $closed++ } catch { } } }
    else { $closed = Close-Extra-Tabs }
    Note ('  burst {0}: {1} opened, {2} closed, {3:N0} s with the lock' -f $n, $opened.Count, $closed, ((Get-Date) - $t0).TotalSeconds)
  } finally {
    if ($Windows) { foreach ($w in $opened) { try { $w.Quit() } catch { } } } else { try { [void](Close-Extra-Tabs) } catch { } }
    Drop-Lock
  }
}

Note "bursts start $Tag (explorer $Target, $Bursts bursts)"
Sample 'start'
foreach ($n in 1..$Bursts) {
  if ($BeforeBurst) { & $BeforeBurst $n }
  Burst $n
  if ($AfterBurst) { & $AfterBurst $n }
  Start-Sleep -Seconds $Settle
  Sample "after burst $n"
  if ($n -lt $Bursts) { Start-Sleep -Seconds $Pause }
}
Note "bursts done $Tag"
