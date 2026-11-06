# Headless, read-only sampling after a restart; -Restart uses core's normal index flag.
param([switch]$Restart, [int]$Seconds = 180, [string]$Output = 'startup.json')
$ErrorActionPreference = 'Stop'
$exe = Join-Path (Resolve-Path "$PSScriptRoot/../../..").Path 'target/release/mypaper-index.exe'
$configPath = Join-Path $env:APPDATA 'mypaper/config.json'
$lockPath = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$old = Get-Process mypaper-index -ErrorAction SilentlyContinue | Where-Object Path -eq $exe | Select-Object -First 1
$oldId = if ($old) { $old.Id } else { 0 }
$original = [bool](Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json).experimental.file_index
Add-Type @'
using System; using System.IO; using System.IO.Pipes; using System.Text;
public static class IndexStartupProbe {
  public static void Replace(string source, string destination) { File.Replace(source, destination, null); }
  public static string Query() {
    using (var pipe = new NamedPipeClientStream(".", "mypaper-index", PipeDirection.InOut)) {
      pipe.Connect(100); pipe.ReadMode = PipeTransmissionMode.Message;
      byte[] input = Encoding.UTF8.GetBytes("{\"method\":\"search\",\"query\":\"cargo\",\"limit\":1}");
      pipe.Write(input, 0, input.Length);
      byte[] buffer = new byte[65536]; int count = pipe.Read(buffer, 0, buffer.Length);
      return Encoding.UTF8.GetString(buffer, 0, count);
    }
  }
}
'@
function Set-IndexFlag([bool]$Enabled) {
  $config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
  $config.experimental | Add-Member file_index $Enabled -Force
  $temporary = $configPath + '.index-startup.tmp'
  [IO.File]::WriteAllText($temporary, ($config | ConvertTo-Json -Depth 50), [Text.UTF8Encoding]::new($false))
  [IndexStartupProbe]::Replace($temporary, $configPath)
}
try {
  if ($Restart) {
    $null = New-Item -ItemType Directory -Path $lockPath
    try {
      Set-IndexFlag $false
      $until = [DateTime]::UtcNow.AddSeconds(20)
      while (Get-Process mypaper-index -ErrorAction SilentlyContinue | Where-Object Path -eq $exe) {
        if ([DateTime]::UtcNow -ge $until) { throw 'Core did not stop its index' }
        Start-Sleep -Milliseconds 50
      }
      Set-IndexFlag $true
    } finally { [IO.Directory]::Delete($lockPath) }
  }
  $until = [DateTime]::UtcNow.AddMinutes(15)
  do {
    $index = Get-Process mypaper-index -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe -and $_.Id -ne $oldId } | Select-Object -First 1
    if ([DateTime]::UtcNow -ge $until) { throw 'No restarted index observed' }
    if (-not $index) { Start-Sleep -Milliseconds 25 }
  } while (-not $index)
  $null = $index.Handle
  $started = $index.StartTime.ToUniversalTime()
  $rows = [Collections.Generic.List[object]]::new()
  $first = $null
  do {
    $index.Refresh()
    if ($index.HasExited) { throw 'Index exited during startup sample' }
    $secondsFromStart = ([DateTime]::UtcNow - $started).TotalSeconds
    if ($null -eq $first) {
      try {
        $request = [Diagnostics.Stopwatch]::StartNew()
        $reply = [IndexStartupProbe]::Query() | ConvertFrom-Json
        if ($reply.hits.Count -gt 0) {
          $first = [pscustomobject]@{from_start_ms=([DateTime]::UtcNow-$started).TotalMilliseconds;request_ms=$request.Elapsed.TotalMilliseconds;hit=$reply.hits[0].path}
        }
      } catch { }
    }
    $rows.Add([pscustomobject]@{seconds=[Math]::Round($secondsFromStart,3);cpu_seconds=$index.TotalProcessorTime.TotalSeconds;working_set_bytes=$index.WorkingSet64;private_bytes=$index.PrivateMemorySize64})
    if ($secondsFromStart -lt $Seconds) { Start-Sleep -Milliseconds 200 }
  } while ($secondsFromStart -lt $Seconds)
  $last = $rows[$rows.Count-1]
  [pscustomobject]@{pid=$index.Id;start_utc=$started.ToString('o');exe_sha256=(Get-FileHash -LiteralPath $exe).Hash;observed_seconds=$last.seconds;cpu_seconds=$last.cpu_seconds;one_core_percent=100*$last.cpu_seconds/$last.seconds;first_search=$first;rows=$rows} |
    ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Output -Encoding utf8
} finally {
  if ($Restart -and -not $original) { Set-IndexFlag $false }
}
