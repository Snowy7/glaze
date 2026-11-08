# A finite real-Windows qualification: use the production Hub under the shell's package identity,
# send a toast from a different process/AUMID, then measure a full minute with no traffic.
param(
    [Parameter(Mandatory)][string]$InstallDirectory,
    [Parameter(Mandatory)][string]$ProbePath,
    [string]$EvidenceDirectory = (Join-Path $PSScriptRoot 'evidence')
)
. "$PSScriptRoot\common.ps1"
$InstallDirectory = (Resolve-Path -LiteralPath $InstallDirectory).Path
$ProbePath = (Resolve-Path -LiteralPath $ProbePath).Path
$EvidenceDirectory = [IO.Path]::GetFullPath($EvidenceDirectory)
New-Item -ItemType Directory -Path $EvidenceDirectory -Force | Out-Null
$lock = 'C:\Users\you\AppData\Local\Temp\glaze-tools\desktop.lock'
while (-not (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)) {
    Start-Sleep -Milliseconds 250
}
$appId = 'glaze.NotificationTest.' + [Guid]::NewGuid().ToString('N')
$probe = Join-Path $InstallDirectory ('notification-check-' + [Guid]::NewGuid().ToString('N') + '.exe')
$observer = $null
$output = Join-Path $EvidenceDirectory 'provider-live.jsonl'
$errors = Join-Path $EvidenceDirectory 'provider-stderr.txt'
try {
    Get-Process glaze-* -ErrorAction SilentlyContinue | Select-Object ProcessName, Id, Path | Format-Table -AutoSize | Out-Host
    Copy-Item -LiteralPath $ProbePath -Destination $probe
    $exeManifest = Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'crates\shell\glaze-shell.manifest'
    Invoke-PackageTool (Get-SdkTool 'mt.exe') @('-nologo', '-manifest', $exeManifest, "-outputresource:$probe;#1")
    & $probe notifications --registration-only | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'packaged-registration.jsonl') -Encoding UTF8
    $arguments = @('notifications','--live','--runs=1','--seconds=60','--baseline-seconds=60',"--watch-aumid=$appId",'--watch-seconds=12')
    $observer = Start-Process -FilePath $probe -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput $output -RedirectStandardError $errors
    # Retain the process handle: otherwise Windows PowerShell can lose ExitCode after exit.
    $null = $observer.Handle
    $deadline = (Get-Date).AddSeconds(80)
    do {
        Start-Sleep -Milliseconds 100
        $lines = @(Get-Content -LiteralPath $output | ForEach-Object { $_ | ConvertFrom-Json })
        $ready = $lines | Where-Object stage -eq 'watch_ready' | Select-Object -Last 1
        if ($observer.HasExited) { throw "Probe exited early: $(Get-Content -LiteralPath $output -Raw) $(Get-Content -LiteralPath $errors -Raw)" }
    } until ($ready -or (Get-Date) -gt $deadline)
    if (-not $ready -or $ready.availability -ne 'Live') { throw 'Packaged provider is not live (grant access in the real shell first)' }
    $sent = & "$PSScriptRoot\test-toast.ps1" -AppId $appId | ConvertFrom-Json
    $deadline = (Get-Date).AddSeconds(3)
    do {
        Start-Sleep -Milliseconds 20
        $change = Get-Content -LiteralPath $output | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.stage -eq 'notification_change' -and $_.count -eq 1 } | Select-Object -First 1
    } until ($change -or (Get-Date) -gt $deadline)
    if (-not $change -or $change.deliveries -lt 1) { throw 'No event-driven toast addition' }
    $addMs = ($change.at_unix_us - $sent.at_unix_us) / 1000.0
    $removedAt = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() * 1000
    & "$PSScriptRoot\test-toast.ps1" -AppId $appId -Remove
    $deadline = (Get-Date).AddSeconds(3)
    do {
        Start-Sleep -Milliseconds 20
        $removed = Get-Content -LiteralPath $output | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.stage -eq 'notification_change' -and $_.count -eq 0 -and $_.at_unix_us -ge $removedAt } | Select-Object -First 1
    } until ($removed -or (Get-Date) -gt $deadline)
    if (-not $removed) { throw 'No event-driven toast removal' }
    $removeMs = ($removed.at_unix_us - $removedAt) / 1000.0
    @{ addition_ms = $addMs; removal_ms = $removeMs; gate_ms = 1000 } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'event-latency.json') -Encoding UTF8
    if ($addMs -gt 1000 -or $removeMs -gt 1000) { throw 'Event latency exceeded 1 second' }
    if (-not $observer.WaitForExit(90000)) { throw 'Probe did not finish its bounded idle sample' }
    if ($observer.ExitCode -ne 0) { throw 'Probe failed during idle sample' }
    $idle = Get-Content -LiteralPath $output | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object stage -eq 'enabled'
    if (-not $idle -or $idle.availability -ne 'Live' -or $idle.deliveries -ne 0 -or $idle.snapshot_changes -ne 0 -or $idle.cpu_core_pct -ne 0) {
        throw "Idle gate failed: $($idle | ConvertTo-Json -Compress)"
    }
    Write-Host "Live provider passed: add ${addMs}ms, remove ${removeMs}ms, 0% CPU / 0 deliveries / 0 changes over 60s."
} finally {
    try {
        if ($observer -and -not $observer.HasExited) { Stop-Process -Id $observer.Id -ErrorAction SilentlyContinue; $observer.WaitForExit() }
        try { & "$PSScriptRoot\test-toast.ps1" -AppId $appId -Remove }
        finally { if (Test-Path -LiteralPath $probe) { Remove-Item -LiteralPath $probe -Force } }
    } finally { [IO.Directory]::Delete($lock) }
}
