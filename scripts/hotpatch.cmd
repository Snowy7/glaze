@echo off
rem Runs the wallpaper engine with live Rust hot-patching (subsecond via dx).
rem dx links patches itself, so it needs the MSVC environment (LIB paths).
rem Usage: scripts\hotpatch.cmd [extra wallpaper args, default: --windowed themes/aurora]
setlocal
for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VS=%%i"
if not defined VS (echo MSVC build tools not found & exit /b 1)
call "%VS%\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
rem dx's patch linker does not work with rust-lld, so use MSVC link.exe here.
set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=link.exe"
rem Patches are diffed from LLVM object files; Cranelift objects do not patch.
set "CARGO_PROFILE_DEV_CODEGEN_BACKEND=llvm"
set "ARGS=%*"
if "%ARGS%"=="" set "ARGS=--windowed themes/aurora"
cd /d "%~dp0.."
dx serve --hotpatch -p mypaper-wallpaper --features hotpatch "--args=%ARGS%" %DX_FLAGS%
