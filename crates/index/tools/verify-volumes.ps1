# Exercise fixed-drive discovery without touching a physical device: a temporary SUBST alias
# targets only this fixture's directory. Broadcasts address only the index's hidden window.
param([string]$Exe = 'D:\Coding\projects\desktop\wallpaper\target\release\mypaper-index.exe')
$ErrorActionPreference = 'Stop'
$base = Join-Path $env:USERPROFILE 'mypaper-index-fixtures'
$scratch = [IO.Path]::GetFullPath((Join-Path $base ('volume-' + [Guid]::NewGuid().ToString('N'))))
if (-not $scratch.StartsWith($base + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe volume fixture path' }
$service = @(Get-CimInstance Win32_Process -Filter "Name='mypaper-index.exe'" | Where-Object {$_.ExecutablePath -eq $Exe -and $_.CommandLine -match '--parent'})
if ($service.Count -ne 1) { throw 'Expected one core-owned index' }
$index = Get-Process -Id $service[0].ProcessId
$indexStart = $index.StartTime
Add-Type @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class IndexVolumeFixture {
 delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] public static extern uint GetDriveType(string root);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern uint QueryDosDevice(string name,StringBuilder target,int n);
 public static string Mapping(string drive) {var s=new StringBuilder(4096);return QueryDosDevice(drive,s,s.Capacity)==0?null:s.ToString();}
 public static void Notify(int process,int change,int letter) {
  IntPtr found=IntPtr.Zero;
  EnumWindows((h,p)=>{uint id;GetWindowThreadProcessId(h,out id);if(id==(uint)process){var s=new StringBuilder(128);GetClassName(h,s,128);if(s.ToString()=="mypaper.index.volumes")found=h;}return true;},IntPtr.Zero);
  if(found==IntPtr.Zero)throw new Exception("index volume notifier unavailable");
  var data=new int[]{20,2,0,1<<letter,0};var block=Marshal.AllocHGlobal(20);
  try{Marshal.Copy(data,0,block,5);SendMessage(found,0x219,new IntPtr(change),block);}finally{Marshal.FreeHGlobal(block);}
 }
}
'@
function Request-Index([hashtable]$Request) {
  $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', 'mypaper-index', [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
  try {
    $pipe.Connect(2000)
    $pipe.ReadMode = [IO.Pipes.PipeTransmissionMode]::Message
    $message = [Text.Encoding]::UTF8.GetBytes(($Request | ConvertTo-Json -Compress))
    $pipe.Write($message,0,$message.Length)
    $buffer = [byte[]]::new(1024*1024)
    $read = $pipe.ReadAsync($buffer,0,$buffer.Length)
    if (-not $read.Wait(5000)) { throw 'Index response timed out' }
    [Text.Encoding]::UTF8.GetString($buffer,0,$read.Result) | ConvertFrom-Json
  } finally { $pipe.Dispose() }
}
function Await-Hit([string]$Name,[bool]$Present) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  do {
    $answer = Request-Index @{method='search';query=$Name;limit=50}
    $found = @($answer.hits | Where-Object {$_.path.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)}).Count -gt 0
    if ($found -eq $Present) { return $clock.Elapsed.TotalMilliseconds }
    if ($clock.Elapsed.TotalSeconds -gt 60) { throw "Volume hit presence did not become $Present" }
    Start-Sleep -Milliseconds 50
  } while ($true)
}
$drive = @('Z:','Y:','X:','W:') | Where-Object { -not [IndexVolumeFixture]::Mapping($_) } | Select-Object -First 1
if (-not $drive) { throw 'No free fixture drive letter' }
$root = $drive + '\'
$letter = [int][char]$drive[0] - [int][char]'A'
$tag = 'mypaper-volume-' + [Guid]::NewGuid().ToString('N') + '.txt'
$created = $false
try {
  if ((Request-Index @{method='status'}).scanning) { throw 'Wait for initial reconciliation' }
  [IO.Directory]::CreateDirectory($scratch) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scratch $tag),[byte[]]::new(123))
  & subst.exe $drive $scratch
  if ($LASTEXITCODE -ne 0) { throw 'SUBST creation failed' }
  $created = $true
  if ([IndexVolumeFixture]::GetDriveType($root) -ne 3) { throw 'Fixture does not appear as a fixed drive' }
  [IndexVolumeFixture]::Notify($index.Id,0x8000,$letter)
  $arrivalMs = Await-Hit $tag $true
  $before = Request-Index @{method='sizes';path=$root;limit=10}
  if ($before.hits[0].size -ne 123 -or $before.hits[0].files -ne 1) { throw 'Arriving volume size incorrect' }
  # Delete the owned backing folder while its watch is active. This produces an actual
  # ReadDirectoryChanges error; no synthetic removal message hides the failed watch.
  Remove-Item -LiteralPath $scratch -Recurse -Force
  $failureMs = Await-Hit $tag $false
  if ((Request-Index @{method='sizes';path=$root;limit=10}).hits.Count) { throw 'Offline volume still has Sizes hits' }
  $other = Request-Index @{method='sizes';path='C:\';limit=1}
  if (-not $other.hits.Count) { throw 'Other volume stopped answering' }
  [IO.Directory]::CreateDirectory($scratch) | Out-Null
  [IO.File]::WriteAllBytes((Join-Path $scratch $tag),[byte[]]::new(456))
  [IndexVolumeFixture]::Notify($index.Id,0x8000,$letter)
  $returnMs = Await-Hit $tag $true
  $returned = Request-Index @{method='sizes';path=$root;limit=10}
  if ($returned.hits[0].size -ne 456 -or $returned.hits[0].files -ne 1) { throw 'Returned volume kept stale records' }
  if ([IndexVolumeFixture]::Mapping($drive) -ne ('\??\' + $scratch)) { throw 'Fixture drive mapping changed' }
  & subst.exe $drive /d
  if ($LASTEXITCODE -ne 0) { throw 'SUBST removal failed' }
  $created = $false
  [IndexVolumeFixture]::Notify($index.Id,0x8004,$letter)
  $removalMs = Await-Hit $tag $false
  $index.Refresh()
  if ($index.HasExited -or $index.StartTime -ne $indexStart) { throw 'Index restarted during volume lifecycle' }
  [pscustomobject]@{index_pid=$index.Id;fixture_drive=$root;arrival_ms=$arrivalMs;watch_failure_hide_ms=$failureMs;return_ms=$returnMs;removal_hide_ms=$removalMs;arrived_sizes=$before;returned_sizes=$returned;other_volume_available=[bool]$other.hits.Count;peak_working_set_bytes=$index.PeakWorkingSet64} | ConvertTo-Json -Depth 6
} finally {
  if ($created) {
    if ([IndexVolumeFixture]::Mapping($drive) -ne ('\??\' + $scratch)) { throw 'Refusing cleanup of a changed drive mapping' }
    & subst.exe $drive /d
    [IndexVolumeFixture]::Notify($index.Id,0x8004,$letter)
  }
  if ([IO.Path]::GetFullPath($scratch).StartsWith($base + '\',[StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $scratch)) { Remove-Item -LiteralPath $scratch -Recurse -Force }
}
