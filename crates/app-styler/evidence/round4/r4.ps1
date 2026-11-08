# App styler round 4: one variant in a fresh separate Explorer process.
#   control   nothing attached
#   noadvise  probe TAP attached (holds XAML Diagnostics), never advises
#   bare      probe TAP attached and advised, no rules (round 3's condition)
#   design    the round 4 helper (--preview glass --only <pid>): styling with recycling
# Each: bursts of open/navigate/close (bursts.ps1), then the helper ends (the TAP detaches) and
# the process is sampled once more.
param([Parameter(Mandatory)][ValidateSet('control', 'noadvise', 'bare', 'design')][string]$Variant,
      [int]$Bursts = 10, [int]$Pause = 30)
$ErrorActionPreference = 'Stop'
# Appends a line (retrying while a reader has the file open) and echoes it.
function Append-Line([string]$file, [string]$line) {
  for ($i = 0; $i -lt 20; $i++) { try { [IO.File]::AppendAllText($file, $line + "`r`n"); break } catch { Start-Sleep -Milliseconds 100 } }
  Write-Host $line
}
$root = 'C:\glaze-targets\app-styler\round4'
$log = Join-Path $root "$Variant.log"
$dir = Join-Path $env:LOCALAPPDATA 'glaze\app-styler'
$probe = Join-Path $root 'probe-bin\glaze-styler.exe'
$design = 'C:\glaze-targets\app-styler\release\glaze-styler.exe'
$lock = 'C:\Users\you\AppData\Local\Temp\glaze-tools\desktop.lock'
function Note([string]$m) { $l = '{0} {1}' -f (Get-Date -Format 'HH:mm:ss'), $m; Append-Line $log $l }
function Take-Lock { for ($i = 0; $i -lt 240; $i++) { if (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue) { return }; Start-Sleep -Milliseconds 500 }; throw 'lock busy' }
function Drop-Lock { if (Test-Path -LiteralPath $lock) { Remove-Item -LiteralPath $lock -Recurse -Force } }
function Wait-Log([string]$file, [string]$pattern, [int]$seconds = 20) {
  for ($i = 0; $i -lt $seconds * 4; $i++) {
    if ((Get-Content -LiteralPath $file -Tail 200 -ErrorAction SilentlyContinue) -match $pattern) { return $true }
    Start-Sleep -Milliseconds 250
  }
  return $false
}

if (Get-Process glaze-styler -ErrorAction SilentlyContinue) { throw 'a styler helper is running; stop it first' }
# A fresh separate Explorer with one anchor window (kept open throughout).
# (`explorer /separate` hands off to a second new process: the target is the new window's owner.)
Add-Type -Namespace R4r -Name Win -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr hwnd, out uint pid);'
$shell = New-Object -ComObject Shell.Application
function Hwnds { @($shell.Windows() | ForEach-Object { try { [int64]$_.HWND } catch { } }) }
Take-Lock
try {
  $before = Hwnds
  $known = @(Get-Process explorer | ForEach-Object Id)
  Start-Process explorer.exe -ArgumentList '/separate,C:\Windows'
  $target = 0
  for ($i = 0; $i -lt 60 -and -not $target; $i++) {
    Start-Sleep -Milliseconds 250
    foreach ($h in (Hwnds | Where-Object { $before -notcontains $_ })) {
      $id = [uint32]0; [void][R4r.Win]::GetWindowThreadProcessId([IntPtr]$h, [ref]$id)
      if ($known -notcontains $id) { $target = [int]$id }
    }
  }
  if (-not $target) { throw 'no separate explorer window appeared' }
  Start-Sleep -Seconds 4
} finally { Drop-Lock }
Note "variant ${Variant}: separate explorer $target"

$helper = $null
switch ($Variant) {
  { $_ -in 'noadvise', 'bare' } {
    $rules = Join-Path $root "$Variant.json"
    [IO.File]::WriteAllText($rules, '{}')
    $helper = Start-Process $probe -ArgumentList "--attach $target --winui --rules $rules --seconds 3600" -PassThru
    $ok = Wait-Log (Join-Path $dir 'styler.log') "\[tap $target\] watching the visual tree"
    Note "probe attached: $ok (helper $($helper.Id))"
  }
  'design' {
    $env:RUST_LOG = 'glaze_styler=debug,info'
    $helper = Start-Process $design -ArgumentList "--preview glass --only $target" -PassThru
    $ok = Wait-Log (Join-Path $env:LOCALAPPDATA 'glaze\glaze.log') "styling explorer \($target, WinUI 3\)"
    Note "design helper attached: $ok (helper $($helper.Id))"
  }
}
Start-Sleep -Seconds 5
# Every variant: a hung test Explorer gets its stacks saved and is ended (never the shell's).
$watch = Start-Process powershell -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File $root\watchdog.ps1 -Target $target -Out $root\hang-$Variant.txt" -WindowStyle Hidden -PassThru
& (Join-Path $root 'bursts.ps1') -Target $target -Bursts $Bursts -Log $log -Pause $Pause -Tag $Variant
if (Test-Path "$root\hang-$Variant.txt") { Note "HUNG: see hang-$Variant.txt" }
if ($helper) {
  Stop-Process -Id $helper.Id
  Note "helper $($helper.Id) stopped"
  Start-Sleep -Seconds 12
  $q = Get-Process -Id $target -ErrorAction SilentlyContinue
  if ($q) { Note ('[after the session ended] explorer {0}: {1:N1} MB priv, {2} handles' -f $target, ($q.PrivateMemorySize64 / 1MB), $q.HandleCount) }
}
Note "variant ${Variant} done; explorer $target left open for inspection"
