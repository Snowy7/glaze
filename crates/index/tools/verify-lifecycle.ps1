# Exercise only experimental.file_index. Other config fields survive each atomic write.
param([int]$StandalonePid = 0)
$ErrorActionPreference = 'Stop'
Add-Type 'public static class IndexConfigSwap { public static void Replace(string source, string destination) { System.IO.File.Replace(source, destination, null); } }'
$lock = (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$configPath = Join-Path $env:APPDATA 'glaze\config.json'
$exe = 'D:\Coding\projects\desktop\wallpaper\target\release\glaze-index.exe'
$deadline = [DateTime]::UtcNow.AddMinutes(2)
while (-not (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)) {
  if ([DateTime]::UtcNow -ge $deadline) { throw 'Desktop lock remained busy' }
  Start-Sleep -Seconds 2
}
function Set-IndexFlag([bool]$Enabled) {
  $config = Get-Content -LiteralPath $configPath -Raw -Encoding UTF8 | ConvertFrom-Json
  if (-not $config.experimental) { $config | Add-Member experimental ([pscustomobject]@{}) -Force }
  $config.experimental | Add-Member file_index $Enabled -Force
  $temporary = $configPath + '.index.tmp'
  [IO.File]::WriteAllText($temporary, ($config | ConvertTo-Json -Depth 50), [Text.UTF8Encoding]::new($false))
  [IndexConfigSwap]::Replace($temporary, $configPath)
}
function Index-Status {
  $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', 'glaze-index', [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
  try {
    $pipe.Connect(50)
    $pipe.ReadMode = [IO.Pipes.PipeTransmissionMode]::Message
    $request = [Text.Encoding]::UTF8.GetBytes('{"method":"status"}')
    $pipe.Write($request,0,$request.Length)
    $buffer = New-Object byte[] 8192
    $read = $pipe.ReadAsync($buffer,0,$buffer.Length)
    if (-not $read.Wait(250)) { throw 'Status request timed out' }
    [Text.Encoding]::UTF8.GetString($buffer,0,$read.Result) | ConvertFrom-Json
  } finally { $pipe.Dispose() }
}
function Await-Index {
  $until = [DateTime]::UtcNow.AddSeconds(10)
  do {
    try { $status = Index-Status; if ($status.entries -gt 0) { return $status } } catch {}
    Start-Sleep -Milliseconds 25
  } while ([DateTime]::UtcNow -lt $until)
  throw 'Core did not bring up the cached index'
}
$succeeded = $false
$original = [bool](Get-Content -LiteralPath $configPath -Raw -Encoding UTF8 | ConvertFrom-Json).experimental.file_index
try {
  if ($StandalonePid) {
    $standalone = Get-Process -Id $StandalonePid -ErrorAction SilentlyContinue
    if ($standalone -and $standalone.Path -ne 'C:\glaze-targets\index\service\glaze-index.exe') { throw 'Unexpected standalone index owner' }
    if ($standalone) { Stop-Process -Id $standalone.Id; $standalone.WaitForExit(5000) | Out-Null }
  }
  Set-IndexFlag $true
  $status = Await-Index
  $started = Get-CimInstance Win32_Process -Filter "Name='glaze-index.exe'" | Where-Object ExecutablePath -eq $exe
  if (@($started).Count -ne 1) { throw 'Expected exactly one deployed index process' }
  $parent = Get-Process -Id $started.ParentProcessId -ErrorAction Stop
  if ($parent.ProcessName -ne 'glaze-core') { throw 'The index is not supervised by core' }
  $offClock = [Diagnostics.Stopwatch]::StartNew()
  Set-IndexFlag $false
  $until = [DateTime]::UtcNow.AddSeconds(10)
  while (Get-Process glaze-index -ErrorAction SilentlyContinue) {
    if ([DateTime]::UtcNow -ge $until) { throw 'Index survived disabling its flag' }
    Start-Sleep -Milliseconds 25
  }
  $offMs = $offClock.Elapsed.TotalMilliseconds
  $onClock = [Diagnostics.Stopwatch]::StartNew()
  Set-IndexFlag $true
  $ready = Await-Index
  $onMs = $onClock.Elapsed.TotalMilliseconds
  $succeeded = $true
  [pscustomobject]@{
    disabled_process_count = 0
    disable_ms = [Math]::Round($offMs,2)
    cached_ready_ms = [Math]::Round($onMs,2)
    cached_entries = $ready.entries
    supervisor = $parent.ProcessName
    final_flag = $true
  } | ConvertTo-Json
} finally {
  try { if (-not $succeeded) { Set-IndexFlag $original } }
  finally {
    if ([IO.Path]::GetFullPath($lock) -ne (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')) { throw 'Invalid desktop lock' }
    [IO.Directory]::Delete($lock)
  }
}
