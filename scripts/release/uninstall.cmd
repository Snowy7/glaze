@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1" %*
if errorlevel 1 (
  echo Uninstall stopped. Glaze files were kept for recovery. See the error above.
  pause
  exit /b 1
)
pause
