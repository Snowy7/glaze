param([Parameter(Mandatory)][string]$Backup)
$ErrorActionPreference='Stop'
$lock=(Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$local=Join-Path $env:LOCALAPPDATA 'Glaze'
$config=Join-Path $env:APPDATA 'glaze\config.json'
$out=$PSScriptRoot
$original=[IO.File]::ReadAllText((Join-Path $backup 'config.json'))
$changed=$original -replace '"theme": "rain-city-amber"','"theme": "aurora"'
if($changed -eq $original){throw 'Original wallpaper differs from the recorded selection'}
function Take-Lock {
  $until=(Get-Date).AddMinutes(2)
  while(-not (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)) {
    if((Get-Date) -ge $until){throw 'Desktop lock is busy'}
    Start-Sleep 2
  }
}
function Files {
  Get-ChildItem (Join-Path $local 'backdrop'),(Join-Path $local 'snapshot') -File |
    Where-Object {$_.Extension -in '.bmp','.jpg'} |
    ForEach-Object {[pscustomobject]@{name=$_.Name; utc=$_.LastWriteTimeUtc; bytes=$_.Length; sha256=(Get-FileHash -LiteralPath $_.FullName).Hash}}
}
$start=[DateTime]::UtcNow
$logOffset=@(Get-Content (Join-Path $local 'glaze.log')).Count
$originalProcess=Get-Process glaze-wallpaper
$before=[pscustomobject]@{pid=$originalProcess.Id;privateMiB=$originalProcess.PrivateMemorySize64/1MB;cpu=$originalProcess.TotalProcessorTime.TotalMilliseconds}
$ownsLock=$false
try {
  Take-Lock; $ownsLock=$true
  Files | ConvertTo-Json | Set-Content (Join-Path $out 'files-before.json')
  [IO.File]::WriteAllText($config,$changed)
  Start-Sleep 5
  & powershell -NoProfile -File (Join-Path $out 'capture-studio.ps1') -LockHeld -Out (Join-Path $out 'studio-aurora.png')
  Files | ConvertTo-Json | Set-Content (Join-Path $out 'files-aurora.json')
  Get-ChildItem (Join-Path $local 'snapshot') -Filter 'aurora*.jpg' | Copy-Item -Destination $out
  $peak=Get-Process glaze-wallpaper
  $afterFrame=[pscustomobject]@{pid=$peak.Id;privateMiB=$peak.PrivateMemorySize64/1MB;cpu=$peak.TotalProcessorTime.TotalMilliseconds}
  [IO.Directory]::Delete($lock); $ownsLock=$false
  Start-Sleep 30
  $rest=Get-Process glaze-wallpaper
  $restCpu=$rest.TotalProcessorTime.TotalMilliseconds-$afterFrame.cpu
  [pscustomobject]@{start=$start;before=$before;afterFrame=$afterFrame;rest=@{pid=$rest.Id;privateMiB=$rest.PrivateMemorySize64/1MB;cpuMsOver30s=$restCpu;oneCorePercent=$restCpu/300};sameProcess=($rest.Id -eq $before.pid)} |
    ConvertTo-Json -Depth 5 | Set-Content (Join-Path $out 'cost.json')
} finally {
  if($ownsLock){[IO.Directory]::Delete($lock);$ownsLock=$false;Start-Sleep 30}
  Take-Lock
  try {
    Copy-Item -LiteralPath (Join-Path $backup 'config.json') -Destination $config -Force
    Copy-Item -LiteralPath (Join-Path $backup 'widgets.json') -Destination (Join-Path $env:APPDATA 'glaze\widgets.json') -Force
    Start-Sleep 4
    Copy-Item -LiteralPath (Join-Path $backup 'wallpaper.json') -Destination (Join-Path $local 'wallpaper.json') -Force
    & powershell -NoProfile -File (Join-Path $out 'capture-studio.ps1') -LockHeld -Out (Join-Path $out 'studio-restored.png')
    $restored=@(foreach($item in @(@('config.json',$config),@('widgets.json',(Join-Path $env:APPDATA 'glaze\widgets.json')),@('wallpaper.json',(Join-Path $local 'wallpaper.json')))) {
      [pscustomobject]@{file=$item[0];original=(Get-FileHash -LiteralPath (Join-Path $backup $item[0])).Hash;restored=(Get-FileHash -LiteralPath $item[1]).Hash}
    })
    $restored | ConvertTo-Json | Set-Content (Join-Path $out 'restored.json')
  } finally {[IO.Directory]::Delete($lock)}
  Get-Content (Join-Path $local 'glaze.log') | Select-Object -Skip $logOffset | Where-Object {$_ -match 'glaze_wallpaper'} | Set-Content (Join-Path $out 'live.log')
}
