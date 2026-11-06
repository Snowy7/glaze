# Measure a truly quiet watched tree, then verify off/on and restore the core-owned helper.
param([string]$Exe = 'D:\Coding\projects\desktop\wallpaper\target\release\mypaper-index.exe', [int]$Seconds = 60)
$ErrorActionPreference = 'Stop'
$toolsDir = 'C:\Users\you\AppData\Local\Temp\mypaper-tools'
$lockDir = Join-Path $toolsDir 'desktop.lock'
$configPath = Join-Path $env:APPDATA 'mypaper\config.json'
$tag = 'idle-' + [Guid]::NewGuid().ToString('N')
$fixtureBase = Join-Path $env:USERPROFILE 'mypaper-index-fixtures'
$fixtureRoot = [IO.Path]::GetFullPath((Join-Path $fixtureBase $tag))
$cacheBase = 'C:\mypaper-targets\index'
$fixtureCache = [IO.Path]::GetFullPath((Join-Path $cacheBase $tag))
$ownExe = Join-Path $cacheBase 'service\mypaper-index.exe'
$original = [bool](Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json).experimental.file_index
$ownsLock = $false
Add-Type 'public static class IndexIdleConfig { public static void Replace(string s, string d) { System.IO.File.Replace(s,d,null); } }'
function Take-Lock {
  $until = [DateTime]::UtcNow.AddMinutes(2)
  while (-not (New-Item -ItemType Directory -Path $lockDir -ErrorAction SilentlyContinue)) {
    if ([DateTime]::UtcNow -ge $until) { throw 'Desktop lock remained busy' }
    Start-Sleep -Seconds 2
  }
  $script:ownsLock = $true
}
function Release-Lock {
  if ($script:ownsLock) { [IO.Directory]::Delete($lockDir); $script:ownsLock = $false }
}
function Set-Flag([bool]$Enabled) {
  $config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
  $config.experimental | Add-Member file_index $Enabled -Force
  $temporary = $configPath + '.index-idle.tmp'
  [IO.File]::WriteAllText($temporary, ($config | ConvertTo-Json -Depth 50), [Text.UTF8Encoding]::new($false))
  [IndexIdleConfig]::Replace($temporary, $configPath)
}
try {
  [IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
  [IO.Directory]::CreateDirectory($fixtureCache) | Out-Null
  [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($ownExe)) | Out-Null
  [IO.File]::WriteAllText((Join-Path $fixtureRoot 'quiet.txt'),'quiet')
  Take-Lock
  try {
    Set-Flag $false
    $until = [DateTime]::UtcNow.AddSeconds(15)
    while (Get-Process mypaper-index -ErrorAction SilentlyContinue) {
      if ([DateTime]::UtcNow -ge $until) { throw 'Core-owned helper did not stop' }
      Start-Sleep -Milliseconds 50
    }
    Copy-Item -LiteralPath $Exe -Destination $ownExe -Force
    $fixture = Start-Process -FilePath $ownExe -ArgumentList @('--root',('"'+$fixtureRoot+'"'),('"'+$fixtureCache+'"')) -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $fixtureCache 'service.log')
    $until = [DateTime]::UtcNow.AddSeconds(15)
    do {
      Start-Sleep -Milliseconds 100
      $status = & $Exe --status | ConvertFrom-Json
      if ($status.entries -gt 0 -and -not $status.scanning) { break }
      if ([DateTime]::UtcNow -ge $until) { throw 'Fixture did not settle' }
    } while ($true)
  } finally { Release-Lock }
  Start-Sleep -Seconds 30
  & (Join-Path $toolsDir 'measure.ps1') -Label 'index-round2-idle' -Seconds $Seconds | Set-Content -Encoding utf8 (Join-Path $PSScriptRoot '..\evidence\round2-idle-measure.txt')
  Copy-Item -LiteralPath (Join-Path $toolsDir 'measure-index-round2-idle.json') -Destination (Join-Path $PSScriptRoot '..\evidence\round2-idle.json') -Force
  Start-Sleep -Seconds 30
  & (Join-Path $PSScriptRoot 'verify-lifecycle.ps1') -StandalonePid $fixture.Id | Set-Content -Encoding utf8 (Join-Path $PSScriptRoot '..\evidence\round2-lifecycle.json')
} finally {
  if ($fixture -and -not $fixture.HasExited) { Stop-Process -Id $fixture.Id -ErrorAction SilentlyContinue }
  Release-Lock
  $current = [bool](Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json).experimental.file_index
  if ($current -ne $original) {
    Start-Sleep -Seconds 30
    Take-Lock
    try { Set-Flag $original } finally { Release-Lock }
  }
  if (-not $fixtureRoot.StartsWith([IO.Path]::GetFullPath($fixtureBase)+'\idle-', [StringComparison]::OrdinalIgnoreCase) -or
      -not $fixtureCache.StartsWith([IO.Path]::GetFullPath($cacheBase)+'\idle-', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid fixture cleanup path' }
  if (Test-Path -LiteralPath $fixtureRoot) { Remove-Item -LiteralPath $fixtureRoot -Recurse -Force }
  if (Test-Path -LiteralPath $fixtureCache) { Remove-Item -LiteralPath $fixtureCache -Recurse -Force }
}
