param([string]$From='C:\mypaper-targets\main\release\examples\mypaper-look-fixture.exe')
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
$evidence=Join-Path $repo 'crates/shell/evidence/look-settings'
$testRoot='C:\Users\you\AppData\Local\Temp\mypaper-tools\look-fixture'
$configRoot=Join-Path $testRoot 'appdata\mypaper'
$runRoot='C:\mypaper-targets\look-fixture'
$lock='C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$realConfig=Join-Path $env:APPDATA 'mypaper\config.json'
$beforeHash=(Get-FileHash -LiteralPath $realConfig -Algorithm SHA256).Hash
New-Item -ItemType Directory -Force $evidence,$configRoot,$runRoot | Out-Null
if (Test-Path -LiteralPath (Join-Path $configRoot 'revisions.jsonl')) {Remove-Item -LiteralPath (Join-Path $configRoot 'revisions.jsonl')}
if (Test-Path -LiteralPath (Join-Path $configRoot 'stop')) {Remove-Item -LiteralPath (Join-Path $configRoot 'stop')}
Copy-Item -LiteralPath $From -Destination (Join-Path $runRoot 'mypaper-look-fixture.exe') -Force
$baseline=@{theme='aurora';look=@{style='solid';shape=@{corners=0;edge=$false};text=@{font='Segoe UI';size=1;shadow=$false;tone='light';icons='lucide'}}}
function Save-Config { [IO.File]::WriteAllText((Join-Path $configRoot 'config.json'),($baseline | ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false)) }
Save-Config
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class LookCapture {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr w,out Rect r);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr w,out uint pid);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
}
'@
$oldDpi=[LookCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))
$foreground=[LookCapture]::GetForegroundWindow().ToInt64()
function Foreground-Owner {
 [uint32]$foregroundPid=0
 [LookCapture]::GetWindowThreadProcessId([LookCapture]::GetForegroundWindow(),[ref]$foregroundPid) | Out-Null
 return (Get-Process -Id $foregroundPid -ErrorAction SilentlyContinue).ProcessName
}
$foregroundOwner=Foreground-Owner
$previous=@{APPDATA=$env:APPDATA;LOCALAPPDATA=$env:LOCALAPPDATA;MYPAPER_LOOK_FIXTURE=$env:MYPAPER_LOOK_FIXTURE}
$fixture=$null
$held=$false
$observations=@()
function Revision { if(Test-Path -LiteralPath (Join-Path $configRoot 'revisions.jsonl')) {return Get-Content -LiteralPath (Join-Path $configRoot 'revisions.jsonl') | Select-Object -Last 1 | ConvertFrom-Json}; return $null }
function Await-Revision([long]$after) {
 $until=(Get-Date).AddSeconds(10)
 do { $r=Revision; if($r -and $r.revision -gt $after){return $r}; if($fixture.HasExited){throw "fixture exited: $(Get-Content (Join-Path $evidence 'stderr.log') -Raw)"}; Start-Sleep -Milliseconds 100 } while((Get-Date) -lt $until)
 throw "config watcher did not apply revision after $after"
}
function Capture([string]$name,$revision) {
 foreach($surface in $revision.surfaces) {
  [uint32]$owner=0
  $hwnd=[IntPtr]([long]$surface.hwnd)
  [LookCapture]::GetWindowThreadProcessId($hwnd,[ref]$owner) | Out-Null
  if($owner -ne $fixture.Id){throw 'Refusing screenshot of a window outside the fixture'}
  $rect=New-Object LookCapture+Rect
  if(-not [LookCapture]::GetWindowRect($hwnd,[ref]$rect)){throw 'GetWindowRect failed'}
  $image=[Drawing.Bitmap]::new(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top))
  $graphics=[Drawing.Graphics]::FromImage($image)
  try {
   $graphics.CopyFromScreen($rect.Left,$rect.Top,0,0,$image.Size)
   $image.Save((Join-Path $evidence "$name-$($surface.name).png"),[Drawing.Imaging.ImageFormat]::Png)
  } finally {$graphics.Dispose();$image.Dispose()}
 }
}
try {
 $held=$null -ne (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)
 if(-not $held){throw 'Desktop lock is busy; retry after the other action finishes'}
 $env:APPDATA=Join-Path $testRoot 'appdata'
 $env:LOCALAPPDATA=Join-Path $testRoot 'localappdata'
 $env:MYPAPER_LOOK_FIXTURE=$configRoot
 try {
  $fixture=Start-Process -FilePath (Join-Path $runRoot 'mypaper-look-fixture.exe') -WorkingDirectory $repo -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $evidence 'stdout.log') -RedirectStandardError (Join-Path $evidence 'stderr.log')
 } finally {foreach($key in $previous.Keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}
 $revision=Await-Revision 0
 Start-Sleep -Milliseconds 600
 Capture '01-square' $revision
 $observations+=@{name='square';revision=$revision}
 foreach($step in @(@('corners',24),@('edge',$true),@('font','Consolas'),@('size',1.5),@('tone','dark'),@('shadow',$true),@('icons','fluent'),@('style','liquid'),@('bars_style','solid'),@('menus_style','solid'))) {
  $key=$step[0];$value=$step[1]
  if($key -in 'corners','edge'){$baseline.look.shape[$key]=$value}
  elseif($key -eq 'style'){$baseline.look.style=$value}
  elseif($key -eq 'bars_style'){$baseline.look.bars=@{style=$value}}
  elseif($key -eq 'menus_style'){$baseline.look.menus=@{style=$value}}
  else{$baseline.look.text[$key]=$value}
  Save-Config
  $revision=Await-Revision $revision.revision
  Start-Sleep -Milliseconds 600
  $label='{0:D2}-{1}' -f ($observations.Count+1),$key
  Capture $label $revision
  $observations+=@{name=$key;revision=$revision}
 }
 $foregroundAfter=[LookCapture]::GetForegroundWindow().ToInt64()
 [IO.Directory]::Delete($lock);$held=$false
 # Give other agents a turn; measure.ps1 owns the lock itself.
 Start-Sleep -Seconds 30
 & powershell -NoProfile -File 'C:\Users\you\AppData\Local\Temp\mypaper-tools\measure.ps1' -Label 'look-settings' -Seconds 20
 if($LASTEXITCODE -ne 0){throw 'measure.ps1 failed'}
 Copy-Item -LiteralPath 'C:\Users\you\AppData\Local\Temp\mypaper-tools\measure-look-settings.json' -Destination (Join-Path $evidence 'idle.json') -Force
 Copy-Item -LiteralPath (Join-Path $configRoot 'revisions.jsonl') -Destination (Join-Path $evidence 'revisions.jsonl') -Force
 $result=@{fixture_pid=$fixture.Id;config_unchanged=((Get-FileHash -LiteralPath $realConfig -Algorithm SHA256).Hash -eq $beforeHash);foreground_before=$foreground;foreground_after=$foregroundAfter;foreground_owner_before=$foregroundOwner;foreground_owner_after=(Foreground-Owner);synthetic_input=$false;observations=$observations}
 $result | ConvertTo-Json -Depth 18 | Set-Content -LiteralPath (Join-Path $evidence 'verification.json') -Encoding UTF8
 if(-not $result.config_unchanged){throw 'User config hash changed during verification'}
 [pscustomobject]$result | Select-Object fixture_pid,config_unchanged,foreground_before,foreground_after | Format-List
} finally {
 if($fixture -and -not $fixture.HasExited) {
  [IO.File]::WriteAllText((Join-Path $configRoot 'stop'),'stop')
  if(-not $fixture.WaitForExit(5000)){Stop-Process -Id $fixture.Id}
 }
 if($held){[IO.Directory]::Delete($lock)}
 [LookCapture]::SetThreadDpiAwarenessContext($oldDpi) | Out-Null
 foreach($key in $previous.Keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}
}
