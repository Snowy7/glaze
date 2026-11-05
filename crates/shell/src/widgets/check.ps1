param([switch]$Bless,[string]$Filter='widget',[switch]$Studio,[switch]$BlessStudio)
$ErrorActionPreference='Stop'
$widgetCheckLock='C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$widgetCheckRoot=(Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$widgetCheckOwned=$false
try {
    Write-Output 'Waiting for desktop lock before native rendering/input tests.'
    while (-not (New-Item -ItemType Directory -Path $widgetCheckLock -ErrorAction SilentlyContinue)) {
        $existing=Get-Item -LiteralPath $widgetCheckLock -ErrorAction SilentlyContinue
        if ($existing -and ((Get-Date)-$existing.CreationTime).TotalMinutes -gt 5) {
            Remove-Item -LiteralPath $widgetCheckLock -ErrorAction SilentlyContinue
        }
        Start-Sleep -Seconds 2
    }
    $widgetCheckOwned=$true
    Set-Location -LiteralPath $widgetCheckRoot
    if ($Bless) { $env:MYPAPER_BLESS='1' }
    cargo test -p mypaper-shell --lib $Filter -- --test-threads=1 --nocapture
    if ($LASTEXITCODE -ne 0) { throw 'Shell tests failed' }
    Remove-Item Env:MYPAPER_BLESS -ErrorAction SilentlyContinue
    cargo test -p mypaper-ui -- --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'UI tests failed' }
    if ($Studio) {
        if ($BlessStudio) {$env:MYPAPER_BLESS='1'}
        cargo test -p mypaper-studio -- --test-threads=1 --nocapture
        if ($LASTEXITCODE -ne 0) {throw 'Studio tests failed'}
    }
} finally {
    Remove-Item Env:MYPAPER_BLESS -ErrorAction SilentlyContinue
    if ($widgetCheckOwned) { Remove-Item -LiteralPath $widgetCheckLock -ErrorAction SilentlyContinue }
}
