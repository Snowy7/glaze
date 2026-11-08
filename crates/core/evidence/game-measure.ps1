# CPU, private memory and thread context-switch deltas for every live Glaze process.
# Context switches are a wake proxy (include external events/preemption, not just timers).
param([string]$Label, [int]$Seconds = 60)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class GameThreads {
  [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int c, IntPtr b, int n, out int r);
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern int GetThreadDescription(IntPtr thread, out IntPtr text);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
  public static string Name(long id) {
    IntPtr thread = OpenThread(0x800, false, (uint)id);
    if (thread == IntPtr.Zero) return "";
    try {
      IntPtr text;
      if (GetThreadDescription(thread, out text) != 0) return "";
      try { return Marshal.PtrToStringUni(text) ?? ""; } finally { LocalFree(text); }
    } finally { CloseHandle(thread); }
  }
  public static Dictionary<long, long[]> Read(int pid) {
    var result = new Dictionary<long, long[]>(); int n = 16 << 20; IntPtr b = Marshal.AllocHGlobal(n); int r;
    try {
      if (NtQuerySystemInformation(5, b, n, out r) != 0) throw new Exception("thread snapshot failed");
      long off = 0;
      while (true) {
        IntPtr p = new IntPtr(b.ToInt64() + off);
        int next = Marshal.ReadInt32(p, 0), count = Marshal.ReadInt32(p, 4);
        if (Marshal.ReadIntPtr(p, 0x50).ToInt64() == pid) {
          for (int i = 0; i < count; i++) {
            IntPtr t = new IntPtr(p.ToInt64() + 0x100 + i * 0x50);
            result[Marshal.ReadIntPtr(t, 0x30).ToInt64()] = new long[] { (uint)Marshal.ReadInt32(t, 0x40), Marshal.ReadInt64(t, 0) + Marshal.ReadInt64(t, 8) };
          }
          return result;
        }
        if (next == 0) break; off += next;
      }
    } finally { Marshal.FreeHGlobal(b); }
    return result;
  }
}
'@
$before = @{}
foreach ($process in Get-Process glaze-* -ErrorAction SilentlyContinue) {
  $before[$process.Id] = @{ name = $process.ProcessName; cpu = $process.TotalProcessorTime.TotalMilliseconds;
    started = $process.StartTime; private = $process.PrivateMemorySize64; threads = [GameThreads]::Read($process.Id) }
}
$signalPath = Join-Path $env:LOCALAPPDATA 'glaze/game-activity.json'
$signalBefore = if (Test-Path -LiteralPath $signalPath) { Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json }
$watch = [Diagnostics.Stopwatch]::StartNew()
Start-Sleep -Seconds $Seconds
$elapsed = $watch.Elapsed.TotalSeconds
$rows = foreach ($process in Get-Process glaze-* -ErrorAction SilentlyContinue) {
  $initial = $before[$process.Id]
  if (-not $initial -or $initial.started -ne $process.StartTime) { throw "Process changed during measurement: $($process.ProcessName)" }
  $threads = [GameThreads]::Read($process.Id)
  $deltas = foreach ($threadId in $threads.Keys) {
    $old = if ($initial.threads.ContainsKey($threadId)) { $initial.threads[$threadId] } else { @(0, 0) }
    [pscustomobject]@{ tid = $threadId; name = [GameThreads]::Name($threadId); switches = $threads[$threadId][0] - $old[0];
      cpu_core_pct = [Math]::Round(($threads[$threadId][1] - $old[1]) / ($elapsed * 100000), 4) }
  }
  [pscustomobject]@{ name = $process.ProcessName; pid = $process.Id;
    cpu_ms = $process.TotalProcessorTime.TotalMilliseconds - $initial.cpu;
    cpu_core_pct = [Math]::Round(($process.TotalProcessorTime.TotalMilliseconds - $initial.cpu) / ($elapsed * 10), 4);
    private_before_mib = [Math]::Round($initial.private / 1MB, 2); private_after_mib = [Math]::Round($process.PrivateMemorySize64 / 1MB, 2);
    switches_per_second = [Math]::Round(($deltas | Measure-Object switches -Sum).Sum / $elapsed, 3);
    threads = @($deltas | Sort-Object switches -Descending) }
}
$signalAfter = if (Test-Path -LiteralPath $signalPath) { Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json }
$result = @{ label = $Label; local_time = (Get-Date).ToString('o'); seconds = $elapsed;
  signal_before = $signalBefore; signal_after = $signalAfter; processes = @($rows) }
$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $PSScriptRoot "game-$Label.json") -Encoding utf8
$rows | Select-Object name,cpu_core_pct,private_before_mib,private_after_mib,switches_per_second | Format-Table -AutoSize
