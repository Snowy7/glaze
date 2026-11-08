# Round 5: which part of the Settings rules crashes Personalization > Background? Opens Settings
# on a neutral page, attaches with one rules file (glaze-styler --attach), then opens the
# Background page and sees whether Settings survives 8 s.
#   bisect.ps1 -Log file -Rules file [-Tag name]
param([Parameter(Mandatory)][string]$Log, [Parameter(Mandatory)][string]$Rules, [string]$Tag = '', [string]$Page = 'ms-settings:personalization-background')
. "$PSScriptRoot\lib.ps1"
$script:Log = $Log
Add-Type -Namespace R5b -Name Win -MemberDefinition @'
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern System.IntPtr FindWindowEx(System.IntPtr p, System.IntPtr a, string c, string n);
'@ -ErrorAction SilentlyContinue
function Frame { [R5b.Win]::FindWindowEx([IntPtr]::Zero, [IntPtr]::Zero, 'ApplicationFrameWindow', 'Settings') }
function Open-Page([string]$uri) {
  $fg = [R5.Win]::GetForegroundWindow()
  $shell.ShellExecute($uri, '', '', 'open', 7)
  for ($i = 0; $i -lt 80; $i++) {
    Start-Sleep -Milliseconds 100
    $h = Frame
    if ($h -ne [IntPtr]::Zero) {
      if ([R5.Win]::GetForegroundWindow() -ne $fg) { [void][R5.Win]::ShowWindow($h, 6) }
      [void][R5.Win]::ShowWindow($h, 4)
      [void][R5.Win]::SetWindowPos($h, [IntPtr]::Zero, -2300, 120, 1200, 860, 0x0010 -bor 0x0004)
      return
    }
  }
}
$styler = 'C:\glaze-targets\app-styler\release\glaze-styler.exe'
$tapLog = Join-Path $env:LOCALAPPDATA 'glaze\app-styler\styler.log'
Take-Lock
try {
  Open-Page 'ms-settings:sound'
  Start-Sleep -Seconds 3
  $p = Get-Process SystemSettings | Sort-Object StartTime -Descending | Select-Object -First 1
  $helper = Start-Process $styler -ArgumentList "--attach $($p.Id) --rules $Rules --seconds 30" -PassThru
  for ($i = 0; $i -lt 40; $i++) {
    if (Select-String -LiteralPath $tapLog -Pattern "\[tap $($p.Id)\] watching the visual tree" -Quiet) { break }
    Start-Sleep -Milliseconds 250
  }
  Start-Sleep -Seconds 2
  Open-Page $Page
  Start-Sleep -Seconds 8
  $alive = [bool](Get-Process -Id $p.Id -ErrorAction SilentlyContinue)
  $code = if (-not $alive) { 'gone' } else { 'alive' }
  Note ("{0}: settings {1} {2} 8 s after opening {4} (rules {3})" -f $Tag, $p.Id, $code, $Rules, $Page)
  $h = Frame
  if ($h -ne [IntPtr]::Zero) { [void][R5.Win]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
  Stop-Process -Id $helper.Id -ErrorAction SilentlyContinue
} finally { Drop-Lock }
Start-Sleep -Seconds 3
