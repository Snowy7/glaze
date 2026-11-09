# Repeated release-memory check: every page twice; both galleries scroll from top to end.
# Saves the lifetime peak even when the 50 MiB budget fails, then closes only its own panel.
param(
    [string]$Exe = 'C:\glaze-targets\studio-panel\target-c\release\glaze-studio.exe',
    [string]$Shots = 'C:\glaze-targets\studio-panel\shots-c-r9',
    [switch]$HelpersOnly,
    [int]$Passes = 2,
    [string]$ReportPath = 'C:\glaze-targets\studio-panel\memory-r9.json'
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'live_check.ps1') -Exe $Exe -Shots $Shots -FunctionsOnly

function SettingsHashes {
    $result=@{}
    $root=Join-Path $env:APPDATA 'Glaze'
    $paths=@('config.json','bars.json','widgets.json','studio.json','integrations.json','studio-panel-history.json','plugin-manager.json'|ForEach-Object {Join-Path $root $_})
    $themes=Join-Path $root 'themes'
    if(Test-Path -LiteralPath $themes){$paths+=@(Get-ChildItem -LiteralPath $themes -Directory|ForEach-Object {Join-Path $_.FullName 'theme.json'})}
    foreach($path in $paths){
        $result[$path]=if(Test-Path -LiteralPath $path){$item=Get-Item -LiteralPath $path;"$((Get-FileHash -LiteralPath $path).Hash):$($item.LastWriteTimeUtc.Ticks)"}else{$null}
    }
    return $result
}
function OwnAll {
    for($attempt=0;$attempt -lt 5;$attempt++) {
        try {
            $root = [System.Windows.Automation.AutomationElement]::FromHandle($script:window)
            # Enumerations need values, not live references to every previous page.
            # https://learn.microsoft.com/dotnet/api/system.windows.automation.automationelementmode
            $cache=New-Object System.Windows.Automation.CacheRequest
            $cache.AutomationElementMode=[System.Windows.Automation.AutomationElementMode]::None
            foreach($property in 'Name','IsEnabled','ControlType','ProcessId','BoundingRectangle','IsOffscreen') {
                $cache.Add([System.Windows.Automation.AutomationElement]::("${property}Property"))
            }
            $active=$cache.Activate()
            try {
                return @($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|ForEach-Object {[pscustomobject]@{Current=$_.Cached}})
            } finally {$active.Dispose()}
        } catch [System.Windows.Automation.ElementNotAvailableException] {
            if($attempt -eq 4){throw}
            Start-Sleep -Milliseconds 180
        }
    }
}
function OwnLive([string]$Name) {
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($script:window)
    $condition=New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,$Name)
    $element=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
    if(!$element -or $element.Current.ProcessId -ne $script:process.Id){throw "Live target '$Name' is not our process."}
    return $element
}
function FindOwn([string]$Name,[switch]$Exact) {
    $found = @(OwnAll | Where-Object { $_.Current.IsEnabled -and $(if($Exact){$_.Current.Name -eq $Name}else{$_.Current.Name.StartsWith($Name)}) })
    if ($found.Count -ne 1) { throw "Expected one '$Name'; found $($found.Count). Names: $(@(OwnAll | ForEach-Object {$_.Current.Name}) -join ' | ')" }
    if ($found[0].Current.ProcessId -ne $script:process.Id) { throw 'UIA element is not our process.' }
    return OwnLive $found[0].Current.Name
}
function WaitOwnStable {
    $previous='';$stable=0
    for($attempt=0;$attempt -lt 40;$attempt++) {
        $names=@(OwnAll | ForEach-Object {$_.Current.Name})
        $fingerprint=($names | Sort-Object) -join "`n"
        if($names.Count -gt 2 -and $fingerprint -eq $previous){$stable++}else{$stable=0}
        if($stable -ge 3){return}
        $previous=$fingerprint
        Start-Sleep -Milliseconds 160
    }
    throw 'Our UIA tree did not settle before the next action.'
}
function InvokeElement($Element) {
    $invokedName=$Element.Current.Name
    if($Element -isnot [System.Windows.Automation.AutomationElement]){$Element=OwnLive $invokedName}
    WithDesktopLock {
        if ($Element.Current.ProcessId -ne $script:process.Id) { throw 'UIA target changed process.' }
        # This is a semantic Invoke to our exact process, including virtual reorder controls.
        # It never injects mouse/keyboard input or activates another window.
        $pattern=$null
        if($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
        elseif($Element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$pattern.Toggle()}
        elseif($Element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$pattern)){$pattern.Select()}
        else{throw "No activation pattern for '$($Element.Current.Name)'."}
    }
    Start-Sleep -Milliseconds 500
    WaitOwnStable
    $backNames=@(OwnAll | Where-Object {$_.Current.Name.StartsWith('Back to ')} | ForEach-Object {$_.Current.Name})
    Write-Host "[TOUR-r9] $invokedName => $($backNames -join ', ')"
}
function InvokePanel([string]$Name,[switch]$Exact) { InvokeElement (FindOwn $Name -Exact:$Exact) }
function BackRoot {
    WaitOwnStable
    for ($level=0;$level -lt 6;$level++) {
        $backs=@(OwnAll | Where-Object {$_.Current.Name.StartsWith('Back to ') -and $_.Current.IsEnabled})
        if (!$backs.Count) {
            [void](FindOwn 'Search everything' -Exact)
            return
        }
        if ($backs.Count -ne 1) { throw 'More than one panel back action.' }
        InvokeElement $backs[0]
    }
    throw 'Navigation did not reach Studio.'
}
function SetOwnText([string]$Name,[string]$Value) {
    WithDesktopLock {
        $element=FindOwn $Name -Exact
        $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)
    }
    Start-Sleep -Milliseconds 250
}
function CaptureSettled([string]$Name) { Start-Sleep -Milliseconds 400; CaptureOwn $Name }
function RecordPage([string]$Name) {
    $Name="pass-$script:pass/$Name"
    $script:visited += $Name
    $script:process.Refresh()
    $script:memory += @{threadCount=$script:process.Threads.Count;handleCount=$script:process.HandleCount;page=$Name; workingSetBytes=$script:process.WorkingSet64;peakWorkingSetBytes=$script:process.PeakWorkingSet64;peakWorkingSetMB=[math]::Round($script:process.PeakWorkingSet64/1MB,2);privateBytes=$script:process.PrivateMemorySize64;workingSetMB=[math]::Round($script:process.WorkingSet64/1MB,2); privateMB=[math]::Round($script:process.PrivateMemorySize64/1MB,2)}
    Write-Host ('[MEM-r9] '+$Name+' WS='+$script:memory[-1].workingSetMB+' private='+$script:memory[-1].privateMB+' threads='+$script:process.Threads.Count)
    $errors=@(OwnAll | Where-Object {$_.Current.Name -match "Couldn't save|Can't read|Saved by a newer|unknown field"})
    if($errors.Count){throw "Read/save feedback on $Name`: $(@($errors|ForEach-Object{$_.Current.Name}) -join ' | ')"}
}
function SearchNavigation([string]$Query,[string]$Result) {
    BackRoot
    $field=FindOwn 'Search everything' -Exact
    if($field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $Query){$Query+=' '}
    SetOwnText 'Search everything' $Query
    RecordPage 'search'
    InvokePanel $Result
}


Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class StudioMemoryTour { [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam);  }'
function EscapeOwn {
    for($attempt=0;$attempt -lt 3;$attempt++) {
        if(@(OwnAll|Where-Object {$_.Current.Name -eq 'Search everything'}).Count){return}
        WithDesktopLock {
            $point=New-Object StudioPanelCheck+Point
            $point.X=200;$point.Y=24
            [void][StudioPanelCheck]::ClientToScreen($script:window,[ref]$point)
            $owner=[uint32]0
            [void][StudioPanelCheck]::GetWindowThreadProcessId([StudioPanelCheck]::WindowFromPoint($point),[ref]$owner)
            if($owner -ne $script:process.Id){throw 'Escape target is not our panel; no input sent.'}
            [void][StudioMemoryTour]::PostMessage($script:window,0x0100,[UIntPtr]([uint64]27),[IntPtr]::Zero)
        }
        Start-Sleep -Milliseconds 250
    }
}
function WheelOwn([int]$Delta) {
    WithDesktopLock {
            $client=New-Object StudioPanelCheck+Rect
            [void][StudioPanelCheck]::GetClientRect($script:window,[ref]$client)
            $point=New-Object StudioPanelCheck+Point
            $point.X=[int](($client.Right-$client.Left)/2);$point.Y=[int]([math]::Min(180,($client.Bottom-$client.Top)/2))
            [void][StudioPanelCheck]::ClientToScreen($script:window,[ref]$point)
            $owner=[uint32]0
            [void][StudioPanelCheck]::GetWindowThreadProcessId([StudioPanelCheck]::WindowFromPoint($point),[ref]$owner)
            if($owner -ne $script:process.Id){throw "Scroll point ($($point.X),$($point.Y)) belongs to process $owner, expected $($script:process.Id); no input sent."}
            $position=[IntPtr](([int64]($point.Y -band 65535) -shl 16) -bor ($point.X -band 65535))
            # WM_MOUSEWHEEL to our exact HWND; each step exposes another set of cards.
            [void][StudioMemoryTour]::PostMessage($script:window,0x020A,[UIntPtr]([uint64]($Delta -band 65535)*65536),$position)
        }
}
function ScrollToEnd([string]$Name) {
    # Each pass starts at the top; the app remembers scroll offsets between visits.
    WheelOwn 32760
    Start-Sleep -Milliseconds 250
    $previous='';$stable=0
    for($step=0;$step -lt 120;$step++) {
        WheelOwn -480
        Start-Sleep -Milliseconds 240
        $fingerprint=@(OwnAll | Where-Object {!$_.Current.IsOffscreen} | ForEach-Object {
            $r=$_.Current.BoundingRectangle
            "$($_.Current.Name):$([math]::Round($r.Top)):$([math]::Round($r.Height))"
        }) -join '|'
        if($fingerprint -eq $previous){$stable++}else{$stable=0}
        $previous=$fingerprint
        RecordPage "$Name-scroll-$step"
        if($stable -ge 3){$script:scrolls+=@{pass=$script:pass;page=$Name;steps=$step+1;reachedEnd=$true};return}
    }
    throw "Did not reach the end of $Name."
}
if($HelpersOnly){return}
[void][System.IO.Directory]::CreateDirectory($Shots)
$script:scrolls=@()
$before=SettingsHashes
$componentsBefore=@(Get-Process -Name 'glaze-core','glaze-wallpaper' -ErrorAction SilentlyContinue|Select-Object Id,Path,StartTime)
$existing=@(Get-Process -Name 'glaze-studio' -ErrorAction SilentlyContinue | Select-Object Id,Path,MainWindowTitle)
if($existing|Where-Object{$_.MainWindowTitle -like '*Panel*'}){throw 'Another panel is running; it was not touched.'}
$script:visited=@();$script:memory=@();$captures=@();$uia=@{};$script:process=$null
$success=$false;$tourCompleted=$false
$prefix='live'
$start=New-Object System.Diagnostics.ProcessStartInfo
$start.FileName=$Exe;$start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardError=$true;$start.RedirectStandardOutput=$true
$start.EnvironmentVariables['GLAZE_STUDIO_PANEL']='1'
try {
    WithDesktopLock {$script:process=[System.Diagnostics.Process]::Start($start);$script:errorTask=$script:process.StandardError.ReadToEndAsync();$script:outputTask=$script:process.StandardOutput.ReadToEndAsync()}
    for($attempt=0;$attempt -lt 200;$attempt++){
        $script:process.Refresh()
        if($script:process.HasExited){throw 'Our panel exited at startup.'}
        if($script:process.MainWindowHandle -ne [IntPtr]::Zero){break}
        Start-Sleep -Milliseconds 100
    }
    $script:window=$script:process.MainWindowHandle
    if($script:window -eq [IntPtr]::Zero){throw 'Our panel has no window.'}
    Start-Sleep -Milliseconds 700
    for($script:pass=1;$script:pass -le $Passes;$script:pass++){
        BackRoot;RecordPage 'root'
        InvokePanel 'Look ';RecordPage 'look'
        if($script:pass -eq 1){InvokePanel 'More' -Exact};RecordPage 'look-more'
        BackRoot;InvokePanel 'Bars ';RecordPage 'bars';$captures+=CaptureSettled 'live-bars.png'
        InvokePanel 'Preset ';RecordPage 'bars-design';$captures+=CaptureSettled 'live-bars-design.png';InvokePanel 'Back to Bars' -Exact
        if($script:pass -eq 1){InvokePanel 'More' -Exact};InvokePanel 'Panels ';RecordPage 'panels'
        foreach($name in @('Start menu ','Calendar ','Sound ','Tray ','Notifications ')){
            InvokePanel $name;RecordPage $name.Trim();InvokePanel 'Back to Panels' -Exact
        }
        BackRoot;InvokePanel 'Bars ';InvokePanel 'Top bar ';RecordPage 'top'
        InvokePanel 'Design ';RecordPage 'top-design';InvokePanel 'Back to Top bar' -Exact
        $modules=@(OwnAll|Where-Object{$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::ListItem})
        if($modules.Count){InvokeElement $modules[0];RecordPage 'module';InvokePanel 'Back to Top bar' -Exact}
        InvokePanel 'Add a module' -Exact;RecordPage 'add-module';InvokePanel 'Cancel' -Exact
        BackRoot;InvokePanel 'Bars ';InvokePanel 'Dock ';RecordPage 'dock'
        InvokePanel 'Design ';RecordPage 'dock-design';InvokePanel 'Back to Dock' -Exact
        BackRoot;InvokePanel 'Widgets ';RecordPage 'widgets'
        $widgets=@(OwnAll|Where-Object{$_.Current.Name -match 'column ' -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button})
        if(!$widgets.Count){throw 'No widget row is available for the required live capture.'}
        InvokeElement $widgets[0];RecordPage 'widget';$captures+=CaptureSettled 'live-widget.png'
        BackRoot;InvokePanel 'Widgets ';InvokePanel 'Add' -Exact;RecordPage 'add-widget';ScrollToEnd 'add-widget';$captures+=CaptureSettled 'live-add-widget.png';InvokePanel 'Cancel' -Exact
        BackRoot;InvokePanel 'Wallpaper ';RecordPage 'wallpapers';ScrollToEnd 'wallpapers';$captures+=CaptureSettled 'live-wallpapers.png'
        $details=@(OwnAll|Where-Object{$_.Current.Name.EndsWith(' details') -and $_.Current.IsEnabled})
        if($details.Count){InvokeElement $details[0];RecordPage 'wallpaper';$captures+=CaptureSettled 'live-wallpaper.png';InvokePanel 'Back to Wallpapers' -Exact}
        InvokePanel 'Add' -Exact;RecordPage 'add-wallpaper';InvokePanel 'Cancel' -Exact
        BackRoot;InvokePanel 'Windows ';RecordPage 'windows';$captures+=CaptureSettled 'live-windows.png';if($script:pass -eq 1){InvokePanel 'More' -Exact};RecordPage 'windows-more'
        BackRoot;InvokePanel 'Connections ';RecordPage 'connections'
        $items=@(OwnAll|Where-Object{$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and $_.Current.Name -match '^(Now playing|Media|Weather and RSS|T3|Discord) '})
        if($items.Count){InvokeElement $items[0];RecordPage 'connection'}
        BackRoot;InvokePanel 'Settings ';RecordPage 'settings';$captures+=CaptureSettled 'live-settings.png'
        foreach($name in @('General ','Performance and power ','Keyboard ','Troubleshoot ','About ')){
            InvokePanel $name;RecordPage $name.Trim();InvokePanel 'Back to Settings' -Exact
        }
        BackRoot;InvokePanel 'Desktop menu ';RecordPage 'desktop-menu'
        BackRoot;InvokePanel 'History (Ctrl H)' -Exact;$historyRows=@(OwnAll|Where-Object {$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::RadioButton -and $_.Current.Name -match '[0-9]{2}:[0-9]{2}'});if($historyRows.Count){InvokeElement $historyRows[-1]};RecordPage 'history';$captures+=CaptureSettled 'live-history.png'
        SearchNavigation 'Start from a preset' 'Start from a preset '
        RecordPage 'first-run'
        EscapeOwn;BackRoot;RecordPage 'end-root'
    }
    $script:process.Refresh()
    $finalMemory=@{cpuMilliseconds=[math]::Round($script:process.TotalProcessorTime.TotalMilliseconds,2);elapsedSeconds=[math]::Round(((Get-Date)-$script:process.StartTime).TotalSeconds,2);workingSetBytes=$script:process.WorkingSet64;peakWorkingSetBytes=$script:process.PeakWorkingSet64;peakWorkingSetMB=[math]::Round($script:process.PeakWorkingSet64/1MB,2);privateBytes=$script:process.PrivateMemorySize64;workingSetMB=[math]::Round($script:process.WorkingSet64/1MB,2);privateMB=[math]::Round($script:process.PrivateMemorySize64/1MB,2)}
    $drivers=@($script:process.Modules|Where-Object{$_.ModuleName -match '^(nv.*umx|nvgpucomp64|amdxc64|atidx.*)\.dll$'}|ForEach-Object{$_.ModuleName})
    [void]$script:process.CloseMainWindow()
    if(!$script:process.WaitForExit(5000)){throw 'Our panel did not close.'}
    $tourCompleted=$true
    $success=$finalMemory.peakWorkingSetBytes -le 50MB
} finally {
    if($script:process -and !$script:process.HasExited){if(!$finalMemory){$script:process.Refresh();$finalMemory=@{workingSetMB=[math]::Round($script:process.WorkingSet64/1MB,2);privateMB=[math]::Round($script:process.PrivateMemorySize64/1MB,2);peakWorkingSetMB=[math]::Round($script:process.PeakWorkingSet64/1MB,2);peakWorkingSetBytes=$script:process.PeakWorkingSet64}};[void]$script:process.CloseMainWindow();if(!$script:process.WaitForExit(3000)){Stop-Process -Id $script:process.Id}}
    $componentsAfter=@(Get-Process -Name 'glaze-core','glaze-wallpaper' -ErrorAction SilentlyContinue|Select-Object Id,Path,StartTime)
    $componentChanges=@($componentsBefore|Where-Object {
        $beforePart=$_
        !@($componentsAfter|Where-Object {$_.Id -eq $beforePart.Id -and $_.StartTime -eq $beforePart.StartTime}).Count
    })
    $after=SettingsHashes
    $changed=@($before.Keys|Where-Object{$before[$_] -ne $after[$_]})
    if($script:errorTask){$script:errorTask.Result|Set-Content -LiteralPath (Join-Path $Shots "$prefix-process-r9.log") -Encoding utf8}
    $success=$success -and !$changed.Count -and !$componentChanges.Count
    $report=@{exe=$Exe;exeSha256=(Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash;profile="release";head=(git -C $PSScriptRoot rev-parse HEAD);capturedAt=(Get-Date).ToUniversalTime().ToString("o");workingSetBudgetMB=50;tourCompleted=$tourCompleted;success=$success;peakBudgetMet=($finalMemory.peakWorkingSetBytes -le 50MB);process=$script:process.Id;closed=$script:process.HasExited;exitCode=$script:process.ExitCode;existing=$existing;automationEnumeration='cached-properties-only; actions reacquire one live target';passes=$Passes;scrolls=$script:scrolls;componentsBefore=$componentsBefore;componentsAfter=$componentsAfter;componentChanges=$componentChanges;visited=$script:visited;memory=$script:memory;finalMemory=$finalMemory;peakWorkingSetBytes=$finalMemory.peakWorkingSetBytes;peakWorkingSetMB=$finalMemory.peakWorkingSetMB;gpuVendorDrivers=$drivers;captures=$captures;uia=$uia;changedUserSettings=$changed}
    $report|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $Shots "$prefix-verify-r9.json") -Encoding utf8
    $report|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $ReportPath -Encoding utf8
    $report|ConvertTo-Json -Depth 8
}
if($changed.Count){throw 'User settings changed during verification.'}
if($componentChanges.Count){throw 'An existing engine/core identity changed during verification.'}


if($finalMemory.peakWorkingSetBytes -gt 50MB){throw "Peak working set $($finalMemory.peakWorkingSetMB) MiB exceeds the 50 MiB budget."}
