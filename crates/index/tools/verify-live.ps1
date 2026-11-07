# Verify the real watch -> store -> pipe path using files owned by this fixture only.
param([string]$Exe = 'C:\mypaper-targets\index\release\mypaper-index.exe', [int]$Files = 2000)
$ErrorActionPreference = 'Stop'
$scratchBase = Join-Path $env:USERPROFILE 'mypaper-index-fixtures'
$tag = 'mypaper-index-live-' + [Guid]::NewGuid().ToString('N').Substring(0,12)
$scratch = [IO.Path]::GetFullPath((Join-Path $scratchBase $tag))
if (-not $scratch.StartsWith($scratchBase + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid fixture path' }
function Request-Index([string]$Command, [string]$Value) {
  $text = if ($Value) { & $Exe $Command $Value } else { & $Exe $Command }
  if ($LASTEXITCODE -ne 0) { throw 'Index request failed' }
  $text | ConvertFrom-Json
}
function Await-Size([string]$Path, [long]$Bytes, [int]$Count) {
  $deadline = [DateTime]::UtcNow.AddSeconds(60)
  do {
    $reply = Request-Index '--sizes' $Path
    if ($reply.hits.Count -gt 0 -and $reply.hits[0].size -eq $Bytes -and $reply.hits[0].files -eq $Count) { return $reply }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Sizes did not converge for $Path (expected $Bytes bytes, $Count files)"
}
if ((Request-Index '--status' '').scanning) { throw 'Wait for initial scan before running the live change fixture' }
$indexProcess = Get-Process mypaper-index -ErrorAction Stop | Select-Object -First 1
$cpuBefore = $indexProcess.TotalProcessorTime.TotalMilliseconds
$clock = [Diagnostics.Stopwatch]::StartNew()
try {
  [IO.Directory]::CreateDirectory((Join-Path $scratch 'before')) | Out-Null
  $payload = New-Object byte[] 128
  for ($i=0; $i -lt $Files; $i++) { [IO.File]::WriteAllBytes((Join-Path $scratch "before\$tag-$i.txt"), $payload) }
  $created = Await-Size $scratch ($Files * 128) $Files
  $burstMs = $clock.Elapsed.TotalMilliseconds
  $indexProcess.Refresh()
  $burstCpuMs = $indexProcess.TotalProcessorTime.TotalMilliseconds - $cpuBefore
  $query = Request-Index '--query' ("ext:txt path:`"$scratch`" $tag")
  if ($query.hits.Count -eq 0) { throw 'Created files not returned by search' }
  $clock.Restart()
  Move-Item -LiteralPath (Join-Path $scratch 'before') -Destination (Join-Path $scratch 'after')
  [IO.File]::WriteAllBytes((Join-Path $scratch "after\$tag-0.txt"), (New-Object byte[] 1024))
  Move-Item -LiteralPath (Join-Path $scratch "after\$tag-0.txt") -Destination (Join-Path $scratch "after\$tag-renamed.md")
  Remove-Item -LiteralPath (Join-Path $scratch "after\$tag-1.txt")
  $changed = Await-Size $scratch (($Files - 2) * 128 + 1024) ($Files - 1)
  $movedMs = $clock.Elapsed.TotalMilliseconds
  $renamed = Request-Index '--query' ("ext:md path:`"$scratch`" renamed")
  if ($renamed.hits.Count -ne 1) { throw 'Rename/extension query did not converge' }
  $old = Request-Index '--query' ("path:`"$scratch\before`" $tag")
  if ($old.hits.Count -ne 0) { throw 'Old directory still returned after move' }
  [pscustomobject]@{
    files_created = $Files
    payload_bytes = $Files * 128
    burst_wall_ms = [Math]::Round($burstMs,2)
    burst_index_cpu_ms = [Math]::Round($burstCpuMs,2)
    move_resize_rename_delete_ms = [Math]::Round($movedMs,2)
    final_files = $changed.hits[0].files
    final_bytes = $changed.hits[0].size
    final_search_server_us = $renamed.elapsed_us
    index_private_mb = [Math]::Round($indexProcess.PrivateMemorySize64 / 1MB,2)
    indexed_entries = $renamed.entries
  } | ConvertTo-Json
} finally {
  if ([IO.Path]::GetFullPath($scratch).StartsWith($scratchBase + '\', [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $scratch)) {
    Remove-Item -LiteralPath $scratch -Recurse -Force
  }
}
