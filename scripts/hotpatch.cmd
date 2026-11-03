@echo off
rem Runs the wallpaper engine with live Rust hot-patching (subsecond via dx).
rem Status (docs/engine-notes.md): patches build and map, but the patched code still
rem crashes on its first call with dx 0.7.10 on Windows. The workarounds below are
rem what it takes to get that far.
rem Usage: scripts\hotpatch.cmd [extra wallpaper args, default: --windowed themes/aurora]
rem Optional: DX_FLAGS (extra dx flags), CARGO_TARGET_DIR (default: a short path, see below).
setlocal
rem vswhere goes on PATH: "%ProgramFiles(x86)%" cannot be expanded inside the loop below,
rem because the ")" of "(x86)" would end it.
set "PATH=%PATH%;%ProgramFiles(x86)%\Microsoft Visual Studio\Installer"
for /f "usebackq delims=" %%i in (`vswhere.exe -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VS=%%i"
if not defined VS (echo MSVC build tools not found & exit /b 1)
call "%VS%\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
rem dx's patch linker does not work with rust-lld, so use MSVC link.exe here.
set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=link.exe"
rem Patches are diffed from LLVM object files; Cranelift objects do not patch.
set "CARGO_PROFILE_DEV_CODEGEN_BACKEND=llvm"
rem A short target dir: with a long repo path (a worktree) the engine's rustc command line
rem passes 32 KB, cargo hands rustc a response file, and dx cannot capture its arguments
rem ("Missing rustc capture").
if not defined CARGO_TARGET_DIR set "CARGO_TARGET_DIR=%SystemDrive%\mypaper-hotpatch"
set "ARGS=%*"
if "%ARGS%"=="" set "ARGS=--windowed themes/aurora"
cd /d "%~dp0.."
rem dx fails on a second start over an unchanged tree ("Missing linker args") and trips
rem over the empty libdeps archive its last run left: touch the engine and drop the archive.
del /q "%CARGO_TARGET_DIR%\x86_64-pc-windows-msvc\desktop-dev\libdeps-*.a" 2>nul
copy /b "crates\wallpaper\src\main.rs" +,, >nul
rem /INCREMENTAL:NO: with incremental linking a function's address is a jump thunk, which
rem is not what dx's jump table maps, so Engine::tick would never be redirected.
dx serve --hotpatch -p mypaper-wallpaper --features hotpatch "--rustc-args=-Clink-arg=/INCREMENTAL:NO" "--args=%ARGS%" %DX_FLAGS%
