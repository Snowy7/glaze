# Runs the opt-in DWM live tests against owned probe windows: a medium-integrity one (styled,
# then made full-monitor) and an elevated one (must never be touched). Under the desktop lock.
$ErrorActionPreference = "Stop"
$lock = "C:\Users\you\AppData\Local\Temp\glaze-tools\desktop.lock"
$probe = "C:\glaze-targets\safety\probe\probe-host.exe"
$test = Get-ChildItem C:\glaze-targets\safety\debug\build\glaze-tweaks -Recurse -Filter "glaze_tweaks-*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
function Wait-Pid($file) { for ($i = 0; $i -lt 80 -and -not (Test-Path $file); $i++) { Start-Sleep -Milliseconds 100 }; [int](Get-Content $file) }
New-Item -ItemType Directory $lock -ErrorAction Stop | Out-Null
$pids = @()
try {
  # Medium integrity (like the user's apps), started without elevation and without activation.
  $medium = Join-Path $env:TEMP "glaze-safety-medium.pid"; Remove-Item $medium -ErrorAction SilentlyContinue
  & runas /trustlevel:0x20000 "`"$probe`" `"$medium`"" | Out-Null
  $env:GLAZE_CORNER_PROBE_PID = Wait-Pid $medium; $pids += $env:GLAZE_CORNER_PROBE_PID
  & $test.FullName --ignored --exact dwm::live_tests::foreign_window_corner_lifecycle --nocapture --test-threads 1
  "medium exit=$LASTEXITCODE"
  # Elevated: this shell is elevated, so a direct start is elevated with no UAC prompt.
  $high = Join-Path $env:TEMP "glaze-safety-high.pid"; Remove-Item $high -ErrorAction SilentlyContinue
  $p = Start-Process $probe -ArgumentList "`"$high`"" -PassThru
  $env:GLAZE_ELEVATED_CORNER_PROBE_PID = Wait-Pid $high; $pids += $p.Id
  & $test.FullName --ignored --exact dwm::live_tests::elevated_window_is_untouched --nocapture --test-threads 1
  "elevated exit=$LASTEXITCODE"
} finally {
  foreach ($id in $pids + (Get-Process probe-host -ErrorAction SilentlyContinue).Id) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
  Remove-Item (Join-Path $env:TEMP "glaze-safety-*.pid") -ErrorAction SilentlyContinue
  [IO.Directory]::Delete($lock)
}
