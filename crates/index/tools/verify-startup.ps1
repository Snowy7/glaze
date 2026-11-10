# Isolated service fixture. Production index stops/starts only through core's config flag.
param([string]$Exe = 'C:\glaze-targets\index\release\glaze-index.exe')
$ErrorActionPreference = 'Stop'
$base = [IO.Path]::GetFullPath('C:\glaze-targets\index\startup-fixture-' + [Guid]::NewGuid().ToString('N'))
$root = Join-Path $base 'root'
$cache = Join-Path $base 'cache'
$appData = Join-Path $base 'appdata'
$configPath = Join-Path $env:APPDATA 'glaze/config.json'
$lockPath = (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$original = [bool](Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json).experimental.file_index
$fixture = $null
$ownsLock = $false
$lastRelease = $null
Add-Type @'
using System; using System.IO; using System.IO.Pipes; using System.Text;
public static class IndexFixturePipe {
  public static void Replace(string s, string d) { File.Replace(s,d,null); }
  public static string Request(string json) {
    using (var pipe = new NamedPipeClientStream(".", "glaze-index", PipeDirection.InOut)) {
      pipe.Connect(100); pipe.ReadMode = PipeTransmissionMode.Message;
      byte[] input = Encoding.UTF8.GetBytes(json); pipe.Write(input,0,input.Length);
      using (var output = new MemoryStream()) {
        byte[] buffer = new byte[65536];
        do { int count = pipe.Read(buffer,0,buffer.Length); output.Write(buffer,0,count); } while (!pipe.IsMessageComplete);
        return Encoding.UTF8.GetString(output.ToArray());
      }
    }
  }
}
'@
function Take-Lock {
  if ($script:lastRelease) {
    $remaining = 30 - ([DateTime]::UtcNow - $script:lastRelease).TotalSeconds
    if ($remaining -gt 0) { Start-Sleep -Milliseconds ([int]($remaining * 1000)) }
  }
  $until = [DateTime]::UtcNow.AddSeconds(60)
  while (-not (New-Item -ItemType Directory -Path $lockPath -ErrorAction SilentlyContinue)) {
    if ([DateTime]::UtcNow -gt $until) { throw 'Desktop lock busy' }
    Start-Sleep -Milliseconds 500
  }
  $script:ownsLock = $true
}
function Release-Lock {
  if ($script:ownsLock) { [IO.Directory]::Delete($lockPath); $script:ownsLock = $false; $script:lastRelease = [DateTime]::UtcNow }
}
function Set-Flag([bool]$Enabled) {
  $config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
  $config.experimental | Add-Member file_index $Enabled -Force
  $temporary = $configPath + '.index-fixture.tmp'
  [IO.File]::WriteAllText($temporary, ($config | ConvertTo-Json -Depth 50), [Text.UTF8Encoding]::new($false))
  [IndexFixturePipe]::Replace($temporary,$configPath)
}
function Request($Value) { [IndexFixturePipe]::Request(($Value | ConvertTo-Json -Compress)) | ConvertFrom-Json }
function Search([string]$Name) { Request @{method='search';query=$Name;limit=50} }
function Wait-Until([scriptblock]$Test, [string]$Message) {
  $until = [DateTime]::UtcNow.AddSeconds(45)
  do {
    try { if (& $Test) { return } } catch { }
    if ($script:fixture -and $script:fixture.HasExited) { throw "Fixture exited: $(Get-Content (Join-Path $base 'stderr.log') -Raw)" }
    Start-Sleep -Milliseconds 50
  } while ([DateTime]::UtcNow -lt $until)
  throw $Message
}
function Start-Fixture([int]$After, [int]$Delay) {
  $config = @{index=@{include=@(($root.Replace('\','/')+'/**'));startup_rescan_after=$After;startup_rescan_delay=$Delay}}
  [IO.File]::WriteAllText((Join-Path $appData 'glaze/config.json'), ($config | ConvertTo-Json -Depth 6))
  $savedAppData = $env:APPDATA
  try {
    $env:APPDATA = $appData
    $script:fixture = Start-Process -FilePath $Exe -ArgumentList @('--root',('"'+$root+'"'),('"'+$cache+'"')) -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $base 'stderr.log')
    $null = $script:fixture.Handle
  } finally { $env:APPDATA = $savedAppData }
  Wait-Until { (Request @{method='status'}).entries -ge 0 } 'Fixture pipe did not become ready'
}
function Stop-Fixture {
  if ($script:fixture -and -not $script:fixture.HasExited) { Stop-Process -Id $script:fixture.Id; $script:fixture.WaitForExit() }
  $script:fixture = $null
}
try {
  $null = [IO.Directory]::CreateDirectory((Join-Path $root 'top/deep'))
  $null = [IO.Directory]::CreateDirectory($cache)
  $null = [IO.Directory]::CreateDirectory((Join-Path $appData 'Glaze'))
  [IO.File]::WriteAllText((Join-Path $root 'top/deep/seed.txt'),'old')
  [IO.File]::WriteAllText((Join-Path $root 'top/deep/delete-offline.txt'),'gone')
  Take-Lock
  try {
    Set-Flag $false
    $until = [DateTime]::UtcNow.AddSeconds(20)
    while (Get-Process glaze-index -ErrorAction SilentlyContinue) {
      if ([DateTime]::UtcNow -gt $until) { throw 'Production helper did not stop' }
      Start-Sleep -Milliseconds 50
    }
    Start-Fixture 0 1
  } finally { Release-Lock }
  Wait-Until { (Test-Path (Join-Path $cache 'freshness.json')) -and (Search 'seed.txt').hits.Count -eq 1 } 'Initial fixture scan failed'
  Stop-Fixture
  $topTime = [IO.Directory]::GetLastWriteTimeUtc((Join-Path $root 'top'))
  [IO.File]::WriteAllText((Join-Path $root 'top/deep/created-offline.txt'),'offline')
  [IO.File]::WriteAllText((Join-Path $root 'top/deep/seed.txt'),'resized-while-stopped')
  [IO.File]::Delete((Join-Path $root 'top/deep/delete-offline.txt'))
  [IO.Directory]::SetLastWriteTimeUtc((Join-Path $root 'top'),$topTime)
  $start = [Diagnostics.Stopwatch]::StartNew()
  Start-Fixture 0 5
  if ((Search 'created-offline.txt').hits.Count -ne 0) { throw 'Offline file unexpectedly present before catch-up' }
  $cachedMs = $start.Elapsed.TotalMilliseconds
  [IO.File]::WriteAllText((Join-Path $root 'live-during-delay.txt'),'live')
  Wait-Until { (Search 'live-during-delay.txt').hits.Count -eq 1 } 'Live update was blocked by startup delay'
  $liveMs = $start.Elapsed.TotalMilliseconds
  Wait-Until { (Search 'created-offline.txt').hits.Count -eq 1 -and -not (Request @{method='status'}).scanning } 'Offline drift did not converge'
  $offlineMs = $start.Elapsed.TotalMilliseconds
  if ((Search 'delete-offline.txt').hits.Count -ne 0) { throw 'Offline deletion remained indexed' }
  $resized = (Search 'seed.txt').hits[0].size
  if ($resized -ne 21) { throw "Offline resize incorrect: $resized" }
  $fallback = (Get-Content (Join-Path $cache 'service.log') | Where-Object { $_ -match '0 changed top-level markers.*full fallback' }).Count -gt 0
  if (-not $fallback) { throw 'Fixture did not exercise unchanged ancestor markers' }
  Stop-Fixture
  $scanLinesBefore = (Get-Content (Join-Path $cache 'service.log') | Where-Object { $_ -match ' scan ' }).Count
  Start-Fixture 21600 1
  Start-Sleep -Seconds 3
  $scanLinesRecent = (Get-Content (Join-Path $cache 'service.log') | Where-Object { $_ -match ' scan ' }).Count
  if ($scanLinesRecent -ne $scanLinesBefore) { throw 'Recent volume rescanned at startup' }
  $recentCpu = $fixture.TotalProcessorTime.TotalMilliseconds
  $manualAt = [Diagnostics.Stopwatch]::StartNew()
  $null = Request @{method='rescan'}
  Wait-Until { ((Get-Content (Join-Path $cache 'service.log') | Where-Object { $_ -match ' scan ' }).Count -gt $scanLinesRecent) -and -not (Request @{method='status'}).scanning } 'Manual rescan did not complete'
  [pscustomobject]@{cached_ready_ms=$cachedMs;live_during_delay_ms=$liveMs;offline_catchup_ms=$offlineMs;offline_creation=$true;offline_deletion=$true;offline_resize_bytes=$resized;unchanged_ancestor_fallback=$fallback;recent_volume_skipped=$true;recent_run_cpu_ms=$recentCpu;manual_rescan_ms=$manualAt.Elapsed.TotalMilliseconds;log=@(Get-Content (Join-Path $cache 'service.log') | ForEach-Object { [string]$_ });exe_sha256=(Get-FileHash $Exe).Hash} |
    ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $PSScriptRoot '../evidence/startup-fixture.json')
} finally {
  Stop-Fixture
  Release-Lock
  if ([bool](Get-Content $configPath -Raw | ConvertFrom-Json).experimental.file_index -ne $original) {
    Take-Lock
    try { Set-Flag $original } finally { Release-Lock }
  }
  if (-not $base.StartsWith('C:\glaze-targets\index\startup-fixture-', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid fixture cleanup path' }
  if (Test-Path -LiteralPath $base) { Remove-Item -LiteralPath $base -Recurse -Force }
}
