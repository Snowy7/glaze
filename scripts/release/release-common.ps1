$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3

function Get-FullPath([string]$Path) {
    [IO.Path]::GetFullPath($Path).TrimEnd('\')
}

function Assert-ChildPath([string]$Path, [string]$Parent) {
    $full = Get-FullPath $Path
    $base = (Get-FullPath $Parent) + '\'
    if (-not $full.StartsWith($base, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path must be inside ${Parent}: $Path"
    }
    # Never copy through, or recursively delete through, a junction/symlink.
    $cursor = $full
    while ($cursor) {
        if ((Test-Path -LiteralPath $cursor) -and
            ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Reparse points are not supported: $cursor"
        }
        $cursor = Split-Path $cursor -Parent
    }
    $full
}

function Assert-PlainTree([string]$Path) {
    if (Get-ChildItem -LiteralPath $Path -Force -Recurse | Where-Object {
        $_.Attributes -band [IO.FileAttributes]::ReparsePoint
    } | Select-Object -First 1) { throw "Refusing a tree containing a junction/symlink: $Path" }
}

function Assert-OrdinaryUser {
    $principal = New-Object Security.Principal.WindowsPrincipal ([Security.Principal.WindowsIdentity]::GetCurrent())
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this installer/uninstaller normally, not as administrator. Installation is per user.'
    }
}

function Get-InstallProcesses([string]$Directory) {
    $base = (Get-FullPath $Directory) + '\'
    @(Get-Process -Name 'glaze-*' -ErrorAction SilentlyContinue | Where-Object {
        $_.Path -and $_.Path.StartsWith($base, [StringComparison]::OrdinalIgnoreCase)
    })
}

function Invoke-GlazeCommand([string]$Exe, [string[]]$Arguments, [int]$TimeoutSeconds = 120) {
    $stdout = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString('N') + '.out')
    $stderr = $stdout + '.err'
    $process = $null
    try {
        $process = Start-Process -FilePath $Exe -ArgumentList $Arguments -PassThru -WindowStyle Hidden `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $null = $process.Handle
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            # Terminate only this command, never another Glaze process.
            $process.Kill()
            throw "Timed out: $Exe $Arguments"
        }
        $output = Get-Content -LiteralPath $stdout -Raw
        $errors = Get-Content -LiteralPath $stderr -Raw
        if ($process.ExitCode -ne 0) { throw "$Exe $Arguments failed ($($process.ExitCode)): $errors $output" }
        if ($output) { $output.Trim() }
    } finally {
        if ($process) { $process.Dispose() }
        Remove-Item -LiteralPath $stdout,$stderr -Force -ErrorAction SilentlyContinue
    }
}

# The single contract with core's recovery command. Failure preserves the install for recovery.
function Invoke-GlazeRestore([string]$Directory) {
    Invoke-GlazeCommand (Join-Path $Directory 'glaze-core.exe') @('--restore') | Out-Host
}

function Stop-InstallProcesses([string]$Directory) {
    foreach ($process in @(Get-InstallProcesses $Directory)) {
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -ErrorAction Stop; $process.WaitForExit() }
    }
    if (@(Get-InstallProcesses $Directory).Count) { throw 'Glaze processes are still running; files were preserved.' }
}

function Read-InstallState([string]$Directory) {
    $state = Get-Content -LiteralPath (Join-Path $Directory 'glaze-install.json') -Raw | ConvertFrom-Json
    if ($state.product -ne 'Glaze' -or (Get-FullPath $state.directory) -ne (Get-FullPath $Directory)) {
        throw 'This directory is not a Glaze installation created by this installer.'
    }
    $state
}
