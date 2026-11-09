# Run in a medium-integrity Windows PowerShell. When T3 is elevated, invoke this through
# runas /trustlevel:0x20000. This never launches another desktop engine, core or shell.
param(
    [Parameter(Mandatory)][string]$Archive,
    [Parameter(Mandatory)][string]$TestRoot,
    [switch]$Warp
)
. "$PSScriptRoot\release-common.ps1"
Assert-OrdinaryUser
$TestRoot = Get-FullPath $TestRoot
if (Test-Path -LiteralPath $TestRoot) { throw 'Choose a new test directory.' }
if (@(Get-Process -Name glaze-studio -ErrorAction SilentlyContinue).Count) {
    throw 'Studio is already running. Wait for its owner to close it before testing first launch.'
}
New-Item -ItemType Directory -Path $TestRoot | Out-Null
$results = [Collections.Generic.List[object]]::new()
$owned = [Collections.Generic.List[Diagnostics.Process]]::new()
$profile = Join-Path $TestRoot 'profile'
$install = Join-Path $profile 'Local\Programs\Glaze'
$env:APPDATA = Join-Path $profile 'Roaming'
$env:LOCALAPPDATA = Join-Path $profile 'Local'
$env:USERPROFILE = $profile
$env:TEMP = Join-Path $TestRoot 'Temp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Path $env:TEMP -Force | Out-Null
$env:PATH = "$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\WindowsPowerShell\v1.0"
Get-ChildItem Env:GLAZE_* | Remove-Item
$env:RUST_LOG = 'info'
if ($Warp) { $env:GLAZE_FORCE_WARP = '1' }
$timer = [Diagnostics.Stopwatch]::StartNew()
function Record([string]$Name, $Detail) {
    $results.Add([ordered]@{ check = $Name; detail = $Detail; elapsed_ms = $timer.ElapsedMilliseconds })
    Write-Host "$Name : $($Detail | ConvertTo-Json -Compress -Depth 5)"
}
function Start-Owned([string]$Name, [string[]]$Arguments) {
    $params = @{
        FilePath = Join-Path $install ($Name + '.exe'); PassThru = $true; WindowStyle = 'Hidden'
        WorkingDirectory = $TestRoot
        RedirectStandardOutput = (Join-Path $TestRoot ($Name + '.stdout.txt'))
        RedirectStandardError = (Join-Path $TestRoot ($Name + '.stderr.txt'))
    }
    if ($Arguments.Count) { $params.ArgumentList = $Arguments }
    $process = Start-Process @params
    $null = $process.Handle
    $owned.Add($process)
    $process
}
function Wait-Until([scriptblock]$Condition, [int]$Seconds, [string]$Failure) {
    $until = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $until)
    throw $Failure
}
try {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $extracted = Join-Path $TestRoot 'extracted'
    [IO.Compression.ZipFile]::ExtractToDirectory((Resolve-Path -LiteralPath $Archive).Path, $extracted)
    $package = Join-Path $extracted 'Glaze-0.1.0-windows-x64'
    & "$package\install.ps1" -InstallDirectory $install -ProfileRoot $profile -Isolated
    $config = Get-Content -LiteralPath (Join-Path $env:APPDATA 'glaze\config.json') -Raw | ConvertFrom-Json
    if (@($config.PSObject.Properties).Count -ne 0) { throw 'First-run config must inherit all Config defaults.' }
    Record 'isolated-install-defaults' @{ installed = $install; config = '{} (inherits Config::default)'; path = $env:PATH }

    # compile_all also checks linker compatibility; the existence of a cache alone is insufficient.
    Invoke-GlazeCommand (Join-Path $install 'glaze-plugins.exe') @('--compile', ('"' + (Join-Path $install 'plugins') + '"')) -TimeoutSeconds 300 | Out-Host
    $cache = @(Get-ChildItem -Path (Join-Path $env:LOCALAPPDATA 'glaze\plugins\cache\*.cwasm') -File)
    $components = @(Get-ChildItem -Path (Join-Path $install 'plugins\*\*.wasm') -File)
    if ($cache.Count -ne $components.Count) { throw "Expected $($components.Count) plugin caches, found $($cache.Count)." }
    Record 'plugins-compiled' @{ components = $components.Count; caches = $cache.Count }
    $bench = Start-Owned 'glaze-plugins' @('--bench', ('"' + (Join-Path $install 'plugins\hello') + '"'))
    if (-not $bench.WaitForExit(60000) -or $bench.ExitCode -ne 0) { throw 'Hello plugin host instantiation/action check failed.' }
    Record 'plugin-host-hello' @{ exit = $bench.ExitCode; exercised = 'cold/warm init, face, 1000 actions, budget interruption' }
    foreach ($command in 'weather','rss') {
        $info = New-Object Diagnostics.ProcessStartInfo
        $info.FileName = Join-Path $install "plugins\$command\$command.exe"
        $info.UseShellExecute = $false
        $info.CreateNoWindow = $true
        $info.RedirectStandardInput = $true
        $info.RedirectStandardOutput = $true
        $info.RedirectStandardError = $true
        $native = [Diagnostics.Process]::Start($info)
        $owned.Add($native)
        $native.StandardInput.Close()
        if (-not $native.WaitForExit(10000) -or $native.ExitCode -ne 0) { throw "Native $command plugin did not handle clean stdin EOF." }
        Record "native-plugin-$command" @{ exit = $native.ExitCode; input = 'EOF (no network requests)' }
    }

    # No activation means Studio skips its full-desktop onboarding Session (no other windows move).
    $studio = Start-Owned 'glaze-studio' @('--no-activate','--onboarding')
    Wait-Until { $studio.Refresh(); $studio.HasExited -or $studio.MainWindowHandle -ne 0 } 45 'Studio created no window.'
    if ($studio.HasExited) { throw "Studio exited: $($studio.ExitCode)" }
    Start-Sleep -Seconds 2
    Record 'studio-first-run-window' @{ pid = $studio.Id; handle = $studio.MainWindowHandle.ToInt64(); alive = -not $studio.HasExited }
    # Captures only our window, not the user's desktop or neighboring applications.
    Add-Type -AssemblyName System.Drawing
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class GlazeReleaseCapture {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int L,T,R,B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
}
'@
    function Capture-Owned($Process, [string]$File) {
        $Process.Refresh()
        $rect = New-Object GlazeReleaseCapture+Rect
        if (-not [GlazeReleaseCapture]::GetWindowRect($Process.MainWindowHandle, [ref]$rect)) { throw 'GetWindowRect failed.' }
        $bitmap = New-Object Drawing.Bitmap ($rect.R - $rect.L), ($rect.B - $rect.T)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $dc = $graphics.GetHdc()
        try { if (-not [GlazeReleaseCapture]::PrintWindow($Process.MainWindowHandle, $dc, 2)) { throw 'PrintWindow failed.' } }
        finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
        try { $bitmap.Save($File, [Drawing.Imaging.ImageFormat]::Png) } finally { $bitmap.Dispose() }
    }
    Capture-Owned $studio (Join-Path $TestRoot 'studio-first-run.png')
    $null = $studio.CloseMainWindow()
    if (-not $studio.WaitForExit(15000) -or $studio.ExitCode -ne 0) { throw 'Studio did not close cleanly.' }
    Record 'studio-close' @{ exit = $studio.ExitCode }

    $fixture = Join-Path $TestRoot 'fixture'
    New-Item -ItemType Directory -Path $fixture | Out-Null
    'Glaze release fixture' | Set-Content -LiteralPath (Join-Path $fixture 'release-fixture.txt')
    $files = Start-Owned 'glaze-files' @('--no-activate', ('"' + $fixture + '"'))
    Wait-Until { $files.Refresh(); $files.HasExited -or $files.MainWindowHandle -ne 0 } 30 'Files created no window.'
    if ($files.HasExited) { throw "Files exited: $($files.ExitCode)" }
    Start-Sleep -Seconds 2
    Capture-Owned $files (Join-Path $TestRoot 'files.png')
    Record 'files-window' @{ pid = $files.Id; handle = $files.MainWindowHandle.ToInt64(); alive = -not $files.HasExited }
    $null = $files.CloseMainWindow()
    if (-not $files.WaitForExit(15000) -or $files.ExitCode -ne 0) { throw 'Files did not close cleanly.' }

    # The service pipe is session-wide and belongs to live core. Scan via the shipped CLI only.
    $index = Start-Owned 'glaze-index' @('--bench-scan', ('"' + $fixture + '"'))
    if (-not $index.WaitForExit(30000) -or $index.ExitCode -ne 0) { throw 'Index fixture scan failed.' }
    $scans = @(Get-Content -LiteralPath (Join-Path $TestRoot 'glaze-index.stdout.txt') | ForEach-Object { $_ | ConvertFrom-Json })
    if ($scans.Count -ne 4 -or @($scans | Where-Object entries -lt 1).Count) { throw 'Index did not find the fixture.' }
    Record 'index-fixture-scan' $scans

    foreach ($name in 'aurora','builder-demo') {
        $theme = Join-Path $install "themes\examples\$name"
        $out = Join-Path $TestRoot ($name + '.jpg')
        $preview = Start-Owned 'glaze-wallpaper' @('--preview', ('"' + $theme + '"'), '--out', ('"' + $out + '"'), '--size','320x180','--fps','1')
        Wait-Until { $preview.HasExited -or (Test-Path -LiteralPath $out) } 60 "Preview did not render: $name"
        if ($preview.HasExited) { throw "Preview $name exited: $($preview.ExitCode)" }
        $image = [Drawing.Image]::FromFile($out)
        try {
            if ($image.Width -ne 320 -or $image.Height -ne 180) { throw 'Wrong preview dimensions.' }
            Record "preview-$name" @{ width = $image.Width; height = $image.Height; bytes = (Get-Item $out).Length; warp = [bool]$Warp }
        } finally { $image.Dispose() }
        $preview.Kill(); $preview.WaitForExit()
    }
    $log = Join-Path $env:LOCALAPPDATA 'glaze\glaze.log'
    if ((Test-Path -LiteralPath $log) -and (Select-String -LiteralPath $log -Pattern 'PANIC|panicked|allocation.*failed')) {
        throw 'Crash signature in isolated Glaze log.'
    }
    'keep me' | Set-Content -LiteralPath (Join-Path $env:APPDATA 'glaze\retained-test.txt')
    & "$install\uninstall.ps1" -KeepUserData
    if (Test-Path -LiteralPath $install) { throw 'Isolated uninstall left the installation behind.' }
    if (-not (Test-Path -LiteralPath (Join-Path $env:APPDATA 'glaze\retained-test.txt'))) { throw 'Uninstall deleted retained user data.' }
    Record 'isolated-uninstall' @{ files_removed = $true; user_data_kept = $true; restore_skipped = 'live session singleton belongs to real install' }
    Record 'result' 'PASS'
} catch {
    Record 'result' @{ status = 'FAIL'; message = $_.Exception.Message; location = $_.InvocationInfo.PositionMessage }
    throw
} finally {
    foreach ($process in $owned) {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
    # Keep finite evidence; the caller owns TestRoot and may remove it after reviewing.
    $results | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $TestRoot 'results.json') -Encoding UTF8
}
