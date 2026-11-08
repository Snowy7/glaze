# Round 5: the stowed exception that ends Settings on Personalization > Background while the TAP
# follows its tree (no rules). Opens Settings, attaches the TAP with no rules, attaches cdb, then
# opens the Background page; cdb writes the stowed error and stacks when it fails fast.
#   diag.ps1 -Out file
param([Parameter(Mandatory)][string]$Out, [string]$Rules = "$PSScriptRoot\rules-none.json", [string]$Page = 'ms-settings:personalization-background')
. "$PSScriptRoot\lib.ps1"
$script:Log = [IO.Path]::ChangeExtension($Out, '.log')
Add-Type -Namespace R5d -Name Win -MemberDefinition @'
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern System.IntPtr FindWindowEx(System.IntPtr p, System.IntPtr a, string c, string n);
'@ -ErrorAction SilentlyContinue
function Frame { [R5d.Win]::FindWindowEx([IntPtr]::Zero, [IntPtr]::Zero, 'ApplicationFrameWindow', 'Settings') }
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
$cdb = 'C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe'
$styler = 'C:\glaze-targets\app-styler\release\glaze-styler.exe'
$tapLog = Join-Path $env:LOCALAPPDATA 'glaze\app-styler\styler.log'
$sym = 'srv*C:\symbols*https://msdl.microsoft.com/download/symbols;C:\glaze-targets\app-styler\release'
Take-Lock
try {
  Open-Page 'ms-settings:sound'
  Start-Sleep -Seconds 3
  $p = Get-Process SystemSettings | Sort-Object StartTime -Descending | Select-Object -First 1
  if ($Rules -ne 'none') {
    $helper = Start-Process $styler -ArgumentList "--attach $($p.Id) --rules $Rules --seconds 240" -PassThru
    for ($i = 0; $i -lt 40; $i++) {
      if (Select-String -LiteralPath $tapLog -Pattern "\[tap $($p.Id)\] watching the visual tree" -Quiet) { break }
      Start-Sleep -Milliseconds 250
    }
  }
  # Break on the stowed exception (first chance), print it, the stowed errors and the stack.
  $cmds = 'sxe -c2 \".exr -1;kb 80;!analyze -v;q\" c000027b; sxd av; g'
  $dbg = Start-Process $cdb -ArgumentList "-p $($p.Id) -y $sym -logo $Out -c `"$cmds`"" -WindowStyle Hidden -PassThru
  Start-Sleep -Seconds 6
  Open-Page $Page
  Start-Sleep -Seconds 5
} finally { Drop-Lock }
$dbg.WaitForExit(300000) | Out-Null
Note "settings $($p.Id): cdb done ($Out)"
if ($helper) { Stop-Process -Id $helper.Id -ErrorAction SilentlyContinue }
$h = Frame
if ($h -ne [IntPtr]::Zero) { [void][R5.Win]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
