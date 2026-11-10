# Kills Glaze's own processes under the desktop lock and snapshots the system state around it.
# -Case shell | core | both | restore. Output: one JSON line per snapshot, to stdout and $Out.
param([Parameter(Mandatory)][string]$Case, [string]$Out = "C:\glaze-targets\safety\probe\kill-$Case.jsonl",
      [string]$RestoreExe = "C:\glaze-targets\safety\release\glaze-core.exe")
$ErrorActionPreference = "Stop"
$live = "D:\Coding\projects\desktop\wallpaper\target\release\glaze-core.exe"
$lock = (Join-Path $env:LOCALAPPDATA "Temp\glaze-tools\desktop.lock")
$probe = Join-Path $PSScriptRoot "state.ps1"
function Snap($label) {
  $line = & powershell -NoProfile -ExecutionPolicy Bypass -File $probe -Label $label -ProbePid $script:probePid
  Add-Content -Path $Out -Value $line -Encoding utf8
  $line
}
function Pid-Of($name) { (Get-CimInstance Win32_Process -Filter "Name='$name'" | Where-Object { $_.ExecutablePath -eq ($live -replace 'glaze-core.exe', $name) }).ProcessId }
function Start-Glaze { & runas /trustlevel:0x20000 "`"$live`"" | Out-Null }

$deadline = (Get-Date).AddMinutes(2)
while (-not (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue)) {
  if ((Get-Date) -ge $deadline) { throw "Desktop lock is busy" }
  Start-Sleep -Milliseconds 500
}
Remove-Item $Out -ErrorAction SilentlyContinue
# A foreign window we own, in its own no-activate process, for DWM attributes.
$pidFile = Join-Path $env:TEMP "glaze-safety-probe.pid"
Remove-Item $pidFile -ErrorAction SilentlyContinue
$host_ = Start-Process -FilePath (Join-Path $PSScriptRoot "probe-host.exe") -ArgumentList $pidFile -PassThru
for ($i = 0; $i -lt 50 -and -not (Test-Path $pidFile); $i++) { Start-Sleep -Milliseconds 100 }
$script:probePid = $host_.Id
try {
  Snap "before"
  switch ($Case) {
    "shell" { Stop-Process -Id (Pid-Of "glaze-shell.exe") -Force }
    "core" { Stop-Process -Id (Pid-Of "glaze-core.exe") -Force }
    "both" {
      $ids = @((Pid-Of "glaze-core.exe"), (Pid-Of "glaze-shell.exe"))
      & taskkill /f /pid $ids[0] /pid $ids[1] | Out-Null
    }
    "restore" {
      $p = Start-Process -FilePath $RestoreExe -ArgumentList "--restore" -Wait -PassThru -NoNewWindow -RedirectStandardOutput "$Out.stdout"
      Add-Content -Path $Out -Value ("{`"restore_exit`":" + $p.ExitCode + ",`"report`":" + ([string](Get-Content "$Out.stdout" -Raw)).Trim() + "}")
    }
  }
  Start-Sleep -Milliseconds 300; Snap "after+0.3s"
  Start-Sleep -Milliseconds 1700; Snap "after+2s"
  Start-Sleep -Seconds 4; Snap "after+6s"
  if (-not (Pid-Of "glaze-core.exe")) {
    if ($Case -eq "both") {
      # Nothing survived to restore: prove the journal path with --restore before restarting.
      $p = Start-Process -FilePath $RestoreExe -ArgumentList "--restore" -Wait -PassThru -NoNewWindow -RedirectStandardOutput "$Out.stdout"
      Add-Content -Path $Out -Value ("{`"restore_exit`":" + $p.ExitCode + ",`"report`":" + ([string](Get-Content "$Out.stdout" -Raw)).Trim() + "}")
      Snap "after-restore"
    }
    Start-Glaze
    Start-Sleep -Seconds 8
    Snap "glaze-restarted"
  }
} finally {
  Stop-Process -Id $host_.Id -Force -ErrorAction SilentlyContinue
  Remove-Item $pidFile -ErrorAction SilentlyContinue
  [IO.Directory]::Delete($lock)
}
