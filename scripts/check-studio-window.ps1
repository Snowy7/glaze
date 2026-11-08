param(
    [ValidateSet('Start','Inspect','Invoke','Text','Scroll','Capture','Finish')]
    [string]$Mode = 'Inspect',
    [string]$Name,
    [string]$Value,
    [switch]$Prefix
)
# Review only our release exe, with disposable settings and PrintWindow-only captures.
$ErrorActionPreference = 'Stop'
$reviewRoot = 'C:\glaze-targets\studio-window'
$reviewState = Join-Path $reviewRoot 'live-window-state.json'
$Exe = Join-Path $reviewRoot 'target\release\glaze-studio.exe'
$Shots = Join-Path $reviewRoot 'shots'
$checkout = Split-Path $PSScriptRoot -Parent
. (Join-Path $checkout 'crates\studio\src\panel\memory_check.ps1') -Exe $Exe -Shots $Shots -HelpersOnly

function SaveReview($Review) {
    $Review | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reviewState -Encoding utf8
}
function MeasureReview($Review, [string]$Page) {
    $script:process.Refresh()
    $Review.memory += [pscustomobject]@{
        page = $Page
        workingSetMB = [math]::Round($script:process.WorkingSet64 / 1MB, 2)
        privateMB = [math]::Round($script:process.PrivateMemorySize64 / 1MB, 2)
        peakWorkingSetMB = [math]::Round($script:process.PeakWorkingSet64 / 1MB, 2)
    }
    SaveReview $Review
    $Review.memory[-1] | ConvertTo-Json -Compress
}

if ($Mode -eq 'Start') {
    if (Test-Path -LiteralPath $reviewState) {
        $previous = Get-Content -LiteralPath $reviewState -Raw | ConvertFrom-Json
        if (!$previous.closed -and (Get-Process -Id $previous.process -ErrorAction SilentlyContinue)) {
            throw 'The existing review instance must be finished first.'
        }
    }
    # Every fixture is created outside the checkout and outside the owner's settings.
    $fixtureRoot = Join-Path $reviewRoot 'live-data'
    $roaming = Join-Path $fixtureRoot 'roaming'
    $settings = Join-Path $roaming 'Glaze'
    [void][IO.Directory]::CreateDirectory($settings)
    $source = Get-Content -LiteralPath (Join-Path $checkout 'crates\studio\src\goldens.rs') -Raw
    foreach ($pair in @(@('CONFIG','config'), @('BARS','bars'), @('WIDGETS','widgets'))) {
        $match = [regex]::Match($source, ('(?s)const ' + $pair[0] + ': &str = r##"(.*?)"##;'))
        if (!$match.Success) { throw "Missing $($pair[0]) fixture." }
        $fixtureValue = $match.Groups[1].Value | ConvertFrom-Json
        if ($pair[0] -eq 'CONFIG') { $fixtureValue.theme = Join-Path $checkout 'themes\rain-city' }
        [IO.File]::WriteAllText((Join-Path $settings ($pair[1] + '.json')), ($fixtureValue | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding($false)))
    }
    [IO.File]::WriteAllText((Join-Path $settings 'studio.json'), (@{ first_run_done = $false; appearance = 'dark' } | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
    [void][IO.Directory]::CreateDirectory($Shots)
    $review = [pscustomobject]@{
        process = 0; exe = $Exe; title = 'Glaze Studio'; closed = $false
        sourceHead = (git -C $checkout rev-parse HEAD)
        exeSha256 = (Get-FileHash -LiteralPath $Exe).Hash
        startedAt = (Get-Date).ToUniversalTime().ToString('o')
        originalPid32732AtStart = [bool](Get-Process -Id 32732 -ErrorAction SilentlyContinue)
        settingsBefore = (SettingsHashes); settingsAfter = $null
        originalPid32732AtFinish = $null; changedUserSettings = @()
        memory = @(); captures = @(); finalMemory = $null; peakWorkingSetMB = 0
    }
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $Exe
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.EnvironmentVariables['APPDATA'] = $roaming
    $start.EnvironmentVariables['LOCALAPPDATA'] = Join-Path $fixtureRoot 'local'
    $start.EnvironmentVariables.Remove('GLAZE_STUDIO_PANEL')
    $start.EnvironmentVariables.Remove('GLAZE_STUDIO_TEST_INSTANCE')
    WithDesktopLock { $script:process = [Diagnostics.Process]::Start($start) }
    $review.process = $script:process.Id
    SaveReview $review
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        $script:process.Refresh()
        if ($script:process.HasExited) { throw 'Our release exe exited during startup.' }
        if ($script:process.MainWindowHandle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
} else {
    $review = Get-Content -LiteralPath $reviewState -Raw | ConvertFrom-Json
    if ($review.closed) { throw 'This review instance is already closed.' }
    $script:process = Get-Process -Id $review.process
}

$script:window = $script:process.MainWindowHandle
if ($script:process.Id -eq 32732 -or $script:process.Path -ne $Exe -or
    $script:process.MainWindowTitle -ne 'Glaze Studio' -or $script:window -eq [IntPtr]::Zero) {
    throw 'The selected process is not our standalone review window; no UI action was sent.'
}
WaitOwnStable

# The window's gallery is in its right inspector; the panel helper scrolls its center.
function WheelOwn([int]$Delta) {
    WithDesktopLock {
        $client = New-Object StudioPanelCheck+Rect
        [void][StudioPanelCheck]::GetClientRect($script:window, [ref]$client)
        $point = New-Object StudioPanelCheck+Point
        $point.X = $client.Right - 150
        $point.Y = 180
        [void][StudioPanelCheck]::ClientToScreen($script:window, [ref]$point)
        $position = [IntPtr](([int64]($point.Y -band 65535) -shl 16) -bor ($point.X -band 65535))
        # Address only the verified HWND, even when another window covers our inspector.
        [void][StudioMemoryTour]::PostMessage($script:window, 0x020A, [UIntPtr]([uint64]($Delta -band 65535) * 65536), $position)
    }
}

switch ($Mode) {
    'Start' { MeasureReview $review 'Startup'; OwnAll | ForEach-Object { $_.Current.Name } }
    'Inspect' { OwnAll | ForEach-Object { "$($_.Current.ControlType.ProgrammaticName): $($_.Current.Name)" } }
    'Invoke' {
        InvokePanel $Name -Exact:(!$Prefix)
        MeasureReview $review $Name
        OwnAll | ForEach-Object { $_.Current.Name }
    }
    'Text' {
        SetOwnText $Name $Value
        WaitOwnStable
        MeasureReview $review "Text: $Name"
        OwnAll | ForEach-Object { $_.Current.Name }
    }
    'Capture' {
        if ($Name -notmatch '^live-[a-z0-9-]+\.png$') { throw 'Captures need a simple live-*.png name.' }
        $capture = CaptureSettled $Name
        $review.captures += [pscustomobject]$capture
        SaveReview $review
        $capture | ConvertTo-Json
    }
    'Scroll' {
        $script:memory = @(); $script:visited = @(); $script:scrolls = @(); $script:pass = 1
        ScrollToEnd $Name
        $review.memory += $script:memory
        MeasureReview $review "$Name bottom"
    }
    'Finish' {
        MeasureReview $review 'After every page'
        $review.finalMemory = $review.memory[-1]
        $review.peakWorkingSetMB = $review.finalMemory.peakWorkingSetMB
        [void]$script:process.CloseMainWindow()
        if (!$script:process.WaitForExit(5000)) { throw 'Our review window did not close.' }
        $review.closed = $true
        $review.originalPid32732AtFinish = [bool](Get-Process -Id 32732 -ErrorAction SilentlyContinue)
        $after = SettingsHashes
        $review.settingsAfter = $after
        $review.changedUserSettings = @($review.settingsBefore.psobject.Properties | Where-Object { $_.Value -ne $after[$_.Name] } | ForEach-Object { $_.Name })
        SaveReview $review
        $review | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $Shots 'live-window-check.json') -Encoding utf8
        $review | Select-Object process, closed, finalMemory, peakWorkingSetMB, changedUserSettings,
            originalPid32732AtStart, originalPid32732AtFinish | ConvertTo-Json -Depth 5
    }
}
