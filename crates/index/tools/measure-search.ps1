# Read-only full-pipe latency and active residency, against the deployed helper.
param([string]$Exe = 'D:\Coding\projects\desktop\wallpaper\target\release\mypaper-index.exe', [switch]$Cold, [int]$Rounds = 100)
$ErrorActionPreference = 'Stop'
$index = Get-Process mypaper-index -ErrorAction Stop | Where-Object Path -eq $Exe | Select-Object -First 1
if (-not $index) { throw 'No deployed helper' }
$outputPath = Join-Path $env:TEMP ('mypaper-index-query-' + [Guid]::NewGuid().ToString('N') + '.json')
$errorPath = $outputPath + '.err'
$arguments = if ($Cold) { @('--bench-query','--cold') } else { @('--bench-query',"$Rounds") }
$cpuBefore = $index.TotalProcessorTime.TotalMilliseconds
$clock = [Diagnostics.Stopwatch]::StartNew()
$samples = [Collections.Generic.List[object]]::new()
try {
  $client = Start-Process -FilePath $Exe -ArgumentList $arguments -PassThru -WindowStyle Hidden -RedirectStandardOutput $outputPath -RedirectStandardError $errorPath
  $null = $client.Handle
  do {
    $index.Refresh()
    if ($index.HasExited) { throw 'Index restarted during queries' }
    $samples.Add([pscustomobject]@{ms=[Math]::Round($clock.Elapsed.TotalMilliseconds,2);working_set_bytes=$index.WorkingSet64;private_bytes=$index.PrivateMemorySize64;peak_working_set_bytes=$index.PeakWorkingSet64})
    Start-Sleep -Milliseconds 10
    $client.Refresh()
  } while (-not $client.HasExited)
  $client.WaitForExit()
  if ($null -ne $client.ExitCode -and $client.ExitCode -ne 0) { throw (Get-Content -LiteralPath $errorPath -Raw) }
  $benchmark = Get-Content -LiteralPath $outputPath -Raw | ConvertFrom-Json
  if (-not $benchmark.queries) { throw 'Client did not finish a complete benchmark' }
  $index.Refresh()
  [pscustomobject]@{
    pid=$index.Id
    cold=[bool]$Cold
    elapsed_ms=[Math]::Round($clock.Elapsed.TotalMilliseconds,2)
    index_cpu_ms=$index.TotalProcessorTime.TotalMilliseconds-$cpuBefore
    max_sampled_working_set_bytes=($samples | Measure-Object working_set_bytes -Maximum).Maximum
    os_peak_working_set_bytes=$index.PeakWorkingSet64
    max_private_bytes=($samples | Measure-Object private_bytes -Maximum).Maximum
    benchmark=$benchmark
    residency=$samples
  } | ConvertTo-Json -Depth 6
} finally {
  if ($client -and -not $client.HasExited) { Stop-Process -Id $client.Id }
  Remove-Item -LiteralPath $outputPath,$errorPath -ErrorAction SilentlyContinue
}
