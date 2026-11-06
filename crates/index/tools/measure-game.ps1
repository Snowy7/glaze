# Owns only index_game.exe and the desktop lock; never moves another application's window.
param([int]$Seconds = 60, [string]$FixtureExe = 'C:\mypaper-targets\index\release\examples\index_game.exe', [switch]$TestCatchup)
$ErrorActionPreference = 'Stop'
$lockDir = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$signalPath = Join-Path $env:LOCALAPPDATA 'mypaper\game-activity.json'
$evidence = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\evidence'))
$deployed = 'D:\Coding\projects\desktop\wallpaper\target\release\mypaper-index.exe'
$service = @(Get-CimInstance Win32_Process -Filter "Name='mypaper-index.exe'" | Where-Object { $_.ExecutablePath -eq $deployed -and $_.CommandLine -match '--parent' })
if ($service.Count -ne 1) { throw 'Expected one core-owned deployed index' }
$index = Get-Process -Id $service[0].ProcessId
if ([IntPtr]::Size -ne 8) { throw '64-bit PowerShell required for native thread counters' }
Add-Type @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class IndexGameThreads {
 [StructLayout(LayoutKind.Sequential)] public struct Point { public int X,Y; }
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point p);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] static extern bool GetCursorPos(out Point p);
 [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 public static uint ForegroundPid() {uint pid;GetWindowThreadProcessId(GetForegroundWindow(),out pid);return pid;}
 public static void ActivateOwn(IntPtr hwnd,int pid) {
  SetProcessDpiAwarenessContext(new IntPtr(-4)); Rect r; Point before; uint owner;
  if(!GetWindowRect(hwnd,out r))throw new Exception("fixture has no window");
  var p=new Point{X=r.Left+(r.Right-r.Left)/2,Y=r.Top+(r.Bottom-r.Top)/2};
  GetWindowThreadProcessId(WindowFromPoint(p),out owner);
  if(owner!=pid)throw new Exception("fixture activation point is not owned; click skipped");
  if(!GetCursorPos(out before))throw new Exception("cursor unavailable");
  try {
   SetCursorPos(p.X,p.Y);GetWindowThreadProcessId(WindowFromPoint(p),out owner);
   if(owner!=pid)throw new Exception("fixture activation target changed; click skipped");
   mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);
  }finally{SetCursorPos(before.X,before.Y);}
 }
 [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int c,IntPtr b,int n,out int r);
 [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access,bool inherit,uint id);
 [DllImport("kernel32.dll")] static extern int GetThreadDescription(IntPtr thread,out IntPtr description);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr p);
 public static string Name(long tid) {
  var h=OpenThread(0x0800,false,(uint)tid);if(h==IntPtr.Zero)return "";
  try{IntPtr p;if(GetThreadDescription(h,out p)<0)return "";try{return Marshal.PtrToStringUni(p);}finally{LocalFree(p);}}
  finally{CloseHandle(h);}
 }
 public static Dictionary<long,long> Read(int pid) {
  var result=new Dictionary<long,long>(); int n=16<<20,r; IntPtr b=Marshal.AllocHGlobal(n);
  try {
   if(NtQuerySystemInformation(5,b,n,out r)!=0)throw new Exception("thread snapshot failed");
   long off=0;
   while(true) {
    IntPtr p=new IntPtr(b.ToInt64()+off); int next=Marshal.ReadInt32(p,0),count=Marshal.ReadInt32(p,4);
    if(Marshal.ReadIntPtr(p,0x50).ToInt64()==pid) {
     for(int i=0;i<count;i++) {IntPtr t=new IntPtr(p.ToInt64()+0x100+i*0x50);result[Marshal.ReadIntPtr(t,0x30).ToInt64()]=(uint)Marshal.ReadInt32(t,0x40);}
     return result;
    }
    if(next==0)break; off+=next;
   }
  }finally{Marshal.FreeHGlobal(b);}return result;
 }
}
'@
$until = [DateTime]::UtcNow.AddMinutes(2)
while (-not (New-Item -ItemType Directory -Path $lockDir -ErrorAction SilentlyContinue)) {
  if ([DateTime]::UtcNow -ge $until) { throw 'Desktop lock remained busy' }
  Start-Sleep -Seconds 2
}
try { # Outer scope also cleans the optional filesystem fixture if activation fails.
try {
  # Start from a loaded, exercised full index, rather than certifying a tiny startup stub.
  $status = & $deployed --status | ConvertFrom-Json
  if ($LASTEXITCODE -ne 0 -or $status.entries -lt 1000000) { throw 'Full cached index is not ready for the game test' }
  if ($TestCatchup) {
    if ($status.scanning) { throw 'Wait for initial reconciliation before the subtree catch-up test' }
    $gameRoot = Join-Path $env:USERPROFILE ('mypaper-index-fixtures\game-' + [Guid]::NewGuid().ToString('N'))
    $gameName = 'mypaper-game-resume-' + [Guid]::NewGuid().ToString('N') + '.txt'
    $gameFile = Join-Path $gameRoot $gameName
    [IO.Directory]::CreateDirectory($gameRoot) | Out-Null
    $until = [DateTime]::UtcNow.AddSeconds(10)
    do {
      $directory = & $deployed --sizes $gameRoot | ConvertFrom-Json
      if (@($directory.hits | Where-Object path -eq $gameRoot).Count -eq 1) { break }
      if ([DateTime]::UtcNow -ge $until) { throw 'Filesystem fixture did not become indexed' }
      Start-Sleep -Milliseconds 100
    } while ($true)
  }
  $warm = & $deployed --bench-query 1 | ConvertFrom-Json
  if ($LASTEXITCODE -ne 0 -or $warm.queries -ne 11) { throw 'Game test query warmup did not complete' }
  $index.Refresh()
  $beforePrivate = $index.PrivateMemorySize64
  $beforeWorkingSet = $index.WorkingSet64
  $fixture = Start-Process -FilePath $FixtureExe -ArgumentList '180' -WindowStyle Normal -PassThru
  $null = $fixture.Handle
  Start-Sleep -Seconds 1
  $fixture.Refresh()
  [IndexGameThreads]::ActivateOwn($fixture.MainWindowHandle,$fixture.Id)
  Start-Sleep -Seconds 10
  & 'C:\Users\you\AppData\Local\Temp\mypaper-tools\shotall.ps1' -Out (Join-Path $evidence 'round2-game-fullscreen.png') | Out-Null
  $signal = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
  if (-not $signal.activity.pause_background -or $signal.activity.foreground_pid -ne $fixture.Id -or [IndexGameThreads]::ForegroundPid() -ne $fixture.Id) {
    [IndexGameThreads]::ActivateOwn($fixture.MainWindowHandle,$fixture.Id)
    Start-Sleep -Seconds 2
    $signal = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
  }
  [pscustomobject]@{fixture_pid=$fixture.Id;foreground_pid=[IndexGameThreads]::ForegroundPid();signal=$signal} | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $evidence 'round2-game-activation.json')
  if (-not $signal.activity.pause_background -or $signal.activity.foreground_pid -ne $fixture.Id -or [IndexGameThreads]::ForegroundPid() -ne $fixture.Id) { throw 'Owned fullscreen fixture did not engage the real game hint' }
  Start-Sleep -Seconds 2
  $index.Refresh()
  $started = $index.StartTime
  $cpuBefore = $index.TotalProcessorTime.TotalMilliseconds
  $threadsBefore = [IndexGameThreads]::Read($index.Id)
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $samples = @()
  for ($i=0; $i -lt $Seconds; $i++) {
    Start-Sleep -Seconds 1
    $signal = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
    if (-not $signal.activity.pause_background -or $signal.activity.foreground_pid -ne $fixture.Id) { throw 'Game hint changed during measurement' }
    if ($TestCatchup -and $i -eq 10) {
      [IO.File]::WriteAllBytes($gameFile,[byte[]]::new(32768))
      $createdAt = Get-Date -Format o
    }
    $index.Refresh()
    if ($index.HasExited -or $index.StartTime -ne $started) { throw 'Index restarted during game measurement' }
    $samples += [pscustomobject]@{second=$i+1;private_bytes=$index.PrivateMemorySize64;working_set_bytes=$index.WorkingSet64;pause_background=$signal.activity.pause_background;foreground_pid=$signal.activity.foreground_pid}
  }
  $elapsed = $clock.Elapsed.TotalSeconds
  $index.Refresh()
  $threadsAfter = [IndexGameThreads]::Read($index.Id)
  $switches = 0L
  $threadSwitches = foreach ($tid in $threadsAfter.Keys) {
    $old = if ($threadsBefore.ContainsKey($tid)) { $threadsBefore[$tid] } else { 0 }
    $count = $threadsAfter[$tid]-$old
    $switches += $count
    [pscustomobject]@{tid=$tid;name=[IndexGameThreads]::Name($tid);context_switches=$count;switches_per_second=$count/$elapsed}
  }
  [pscustomobject]@{
    index_pid=$index.Id;fixture_pid=$fixture.Id;seconds=$elapsed
    entries_before_game=$status.entries;warmup_queries=$warm.queries
    before_private_bytes=$beforePrivate;before_working_set_bytes=$beforeWorkingSet
    cpu_core_pct=($index.TotalProcessorTime.TotalMilliseconds-$cpuBefore)/($elapsed*10)
    context_switches_per_second=$switches/$elapsed
    max_private_bytes=($samples|Measure-Object private_bytes -Maximum).Maximum
    max_working_set_bytes=($samples|Measure-Object working_set_bytes -Maximum).Maximum
    final_private_bytes=$index.PrivateMemorySize64;final_working_set_bytes=$index.WorkingSet64
    threads=$threadSwitches
    samples=$samples
  } | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $evidence 'round2-game.json')
  $validGame = $true
} finally {
  try {
    if ($fixture -and -not $fixture.HasExited) {
      if ($fixture.Path -ne $FixtureExe) { throw 'Fixture process identity changed' }
      Stop-Process -InputObject $fixture
    }
    if ($TestCatchup -and $validGame) {
      $catchupClock = [Diagnostics.Stopwatch]::StartNew()
      do {
        $answer = & $deployed --query $gameName | ConvertFrom-Json
        if (@($answer.hits | Where-Object path -eq $gameFile).Count -eq 1) { break }
        if ($catchupClock.Elapsed.TotalSeconds -gt 30) { throw 'Subtree change did not catch up within 30 seconds' }
        Start-Sleep -Milliseconds 100
      } while ($true)
      $catchupMs = $catchupClock.Elapsed.TotalMilliseconds
      $sizes = & $deployed --sizes $gameRoot | ConvertFrom-Json
      $aggregate = @($sizes.hits | Where-Object path -eq $gameRoot)
      if ($aggregate.Count -ne 1 -or $aggregate[0].size -ne 32768 -or $aggregate[0].files -ne 1 -or $aggregate[0].size_unknown) { throw 'Resumed fixture size was incorrect' }
      [pscustomobject]@{created_at=$createdAt;file=$gameFile;catchup_ms=$catchupMs;search=$answer;sizes=$sizes} | ConvertTo-Json -Depth 7 | Set-Content -Encoding utf8 (Join-Path $evidence 'round2-backed-game-catchup.json')
    }
    Start-Sleep -Seconds 2
    & 'C:\Users\you\AppData\Local\Temp\mypaper-tools\shotall.ps1' -Out (Join-Path $evidence 'round2-game-restored.png') | Out-Null
  } finally { [IO.Directory]::Delete($lockDir) }
}
} finally {
  if ($gameRoot) {
    if (-not $gameRoot.StartsWith((Join-Path $env:USERPROFILE 'mypaper-index-fixtures\game-'),[StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe game fixture cleanup path' }
    if (Test-Path -LiteralPath $gameFile) { Remove-Item -LiteralPath $gameFile }
    [IO.Directory]::Delete($gameRoot)
  }
}
