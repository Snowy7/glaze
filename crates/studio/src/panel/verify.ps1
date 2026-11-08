param(
    [string]$Exe = 'C:\glaze-targets\studio-panel\target\debug\glaze-studio.exe',
    [string]$Shots = 'C:\glaze-targets\studio-panel\shots',
    [switch]$Fixture
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'live_check.ps1') -Exe $Exe -Shots $Shots -FunctionsOnly

Add-Type -ReferencedAssemblies @([System.Windows.Automation.Automation].Assembly.Location,[System.Windows.Automation.AutomationElementIdentifiers].Assembly.Location) -TypeDefinition @"
using System;
using System.Threading;
using System.Windows.Automation;
public static class StudioPanelLiveEvents {
    public static int Count;
    private static AutomationEventHandler handler = (sender,args) => Interlocked.Increment(ref Count);
    public static void Start(AutomationElement root) { Count=0; Automation.AddAutomationEventHandler(AutomationElementIdentifiers.LiveRegionChangedEvent,root,TreeScope.Subtree,handler); }
    public static void Stop(AutomationElement root) { Automation.RemoveAutomationEventHandler(AutomationElementIdentifiers.LiveRegionChangedEvent,root,handler); }
}
"@

function OwnAll {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($script:window)
    return $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
}
function FindOwn([string]$Name,[switch]$Exact) {
    $found = @(OwnAll | Where-Object { $_.Current.IsEnabled -and $(if($Exact){$_.Current.Name -eq $Name}else{$_.Current.Name.StartsWith($Name)}) })
    if ($found.Count -ne 1) { throw "Expected one '$Name'; found $($found.Count). Names: $(@(OwnAll | ForEach-Object {$_.Current.Name}) -join ' | ')" }
    if ($found[0].Current.ProcessId -ne $script:process.Id) { throw 'UIA element is not our process.' }
    return $found[0]
}
function InvokeElement($Element) {
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
    Start-Sleep -Milliseconds 220
}
function InvokePanel([string]$Name,[switch]$Exact) { InvokeElement (FindOwn $Name -Exact:$Exact) }
function BackRoot {
    for ($level=0;$level -lt 6;$level++) {
        $backs=@(OwnAll | Where-Object {$_.Current.Name.StartsWith('Back to ') -and $_.Current.IsEnabled})
        if (!$backs.Count) { return }
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
function RecordPage([string]$Name) {
    $script:visited += $Name
    $script:process.Refresh()
    $script:memory += @{page=$Name; workingSetMB=[math]::Round($script:process.WorkingSet64/1MB,2); privateMB=[math]::Round($script:process.PrivateMemorySize64/1MB,2)}
    $errors=@(OwnAll | Where-Object {$_.Current.Name -match "Couldn't save|Can't read|Saved by a newer|unknown field"})
    if($errors.Count){throw "Read/save feedback on $Name`: $(@($errors|ForEach-Object{$_.Current.Name}) -join ' | ')"}
}
function SearchNavigation([string]$Query,[string]$Result) {
    BackRoot
    SetOwnText 'Search everything' $Query
    RecordPage 'search'
    InvokePanel $Result
}

$before=SettingsHashes
$existing=@(Get-Process -Name 'glaze-studio' -ErrorAction SilentlyContinue | Select-Object Id,Path,MainWindowTitle)
if($existing|Where-Object{$_.MainWindowTitle -like '*Panel*'}){throw 'Another panel is running; it was not touched.'}
$script:visited=@();$script:memory=@();$captures=@();$uia=@{};$script:process=$null
$success=$false
$prefix=if($Fixture){'fixture'}else{'live'}
$start=New-Object System.Diagnostics.ProcessStartInfo
$start.FileName=$Exe;$start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardError=$true;$start.RedirectStandardOutput=$true
$start.EnvironmentVariables['GLAZE_STUDIO_PANEL']='1'
$fixtureRoot=$null
if($Fixture){
    $fixtureRoot=Join-Path $Shots ('uia-fixture-'+[guid]::NewGuid().ToString('N'))
    $settings=Join-Path $fixtureRoot 'Glaze'
    [void][System.IO.Directory]::CreateDirectory($settings)
    [System.IO.File]::WriteAllText((Join-Path $settings 'config.json'),'{"look":{"style":"liquid","material":{"clarity":0.55}},"desktop_menu":{"items":["view","sort","refresh"]}}')
    [System.IO.File]::WriteAllText((Join-Path $settings 'bars.json'),'{"version":1,"top":{"left":["start"],"right":["clock"]}}')
    [System.IO.File]::WriteAllText((Join-Path $settings 'widgets.json'),'{"version":2,"instances":[]}')
    [System.IO.File]::WriteAllText((Join-Path $settings 'studio.json'),'{"first_run_done":true}')
    $start.EnvironmentVariables['APPDATA']=$fixtureRoot
}
try {
    WithDesktopLock {$script:process=[System.Diagnostics.Process]::Start($start);$script:errorTask=$script:process.StandardError.ReadToEndAsync();$script:outputTask=$script:process.StandardOutput.ReadToEndAsync()}
    for($attempt=0;$attempt -lt 40;$attempt++){
        $script:process.Refresh()
        if($script:process.HasExited){throw 'Our panel exited at startup.'}
        if($script:process.MainWindowHandle -ne [IntPtr]::Zero){break}
        Start-Sleep -Milliseconds 100
    }
    $script:window=$script:process.MainWindowHandle
    if($script:window -eq [IntPtr]::Zero){throw 'Our panel has no window.'}
    Start-Sleep -Milliseconds 700
    RecordPage 'root';$captures+=CaptureOwn "$prefix-root.png"
    InvokePanel 'Look '
    RecordPage 'look';$captures+=CaptureOwn "$prefix-look.png"
    $ranges=@(OwnAll|Where-Object{$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Slider})
    $uia.ranges=@($ranges|ForEach-Object{
        $p=$_.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current
        @{name=$_.Current.Name;value=$p.Value;minimum=$p.Minimum;maximum=$p.Maximum;step=$p.SmallChange;readOnly=$p.IsReadOnly}
    })
    if(!$uia.ranges.Count){throw 'No UIA RangeValue patterns on material sliders.'}
    if($Fixture){
        $liveRoot=[System.Windows.Automation.AutomationElement]::FromHandle($script:window)
        [StudioPanelLiveEvents]::Start($liveRoot)
        $clarity=FindOwn 'Clarity,'
        WithDesktopLock{$clarity.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.7)}
        Start-Sleep -Milliseconds 300
        $saved=Get-Content -LiteralPath (Join-Path $settings 'config.json') -Raw|ConvertFrom-Json
        if([math]::Abs($saved.look.material.clarity-0.7) -gt 0.0001){throw 'UIA range edit did not save the fixture.'}
        $uia.rangeSetSaved=$saved.look.material.clarity
        $uia.liveRegionEvents=[StudioPanelLiveEvents]::Count
        $uia.liveRegions=@(OwnAll|Where-Object{$_.GetCurrentPropertyValue([System.Windows.Automation.AutomationElementIdentifiers]::LiveSettingProperty) -eq 1}|ForEach-Object{$_.Current.Name})
        [StudioPanelLiveEvents]::Stop($liveRoot)
        if($uia.liveRegionEvents -lt 1){throw 'The save did not raise a UIA live-region announcement.'}
        InvokePanel 'Undo (Ctrl Z)' -Exact
        $saved=Get-Content -LiteralPath (Join-Path $settings 'config.json') -Raw|ConvertFrom-Json
        if([math]::Abs($saved.look.material.clarity-0.55) -gt 0.0001){throw 'Range edit did not undo.'}
        BackRoot;InvokePanel 'Desktop menu '
        $row=@(OwnAll|Where-Object{$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::ListItem -and $_.Current.Name -eq 'View'})[0]
        if(!$row){throw 'Sortable View row is absent.'}
        $move=$row.FindFirst([System.Windows.Automation.TreeScope]::Children,(New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,'Move down')))
        if(!$move){throw 'UIA reorder action is absent.'}
        InvokeElement $move
        $saved=Get-Content -LiteralPath (Join-Path $settings 'config.json') -Raw|ConvertFrom-Json
        if(($saved.desktop_menu.items -join ',') -ne 'sort,view,refresh'){throw 'UIA reorder did not move the fixture row.'}
        $uia.reordered=@($saved.desktop_menu.items)
        InvokePanel 'Undo (Ctrl Z)' -Exact
        BackRoot;SetOwnText 'Search everything' 'clock'
        $value=(FindOwn 'Search everything' -Exact).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
        if($value -ne 'clock'){throw 'UIA text edit is not exposed as the current value.'}
        $uia.textValue=$value
        $captures+=CaptureOwn 'fixture-uia-search.png'
        BackRoot
    } else {
        InvokePanel 'More' -Exact;RecordPage 'look-more'
        BackRoot;InvokePanel 'Bars ';RecordPage 'bars';$captures+=CaptureOwn 'live-bars.png'
        InvokePanel 'More' -Exact;InvokePanel 'Panels ';RecordPage 'panels'
        foreach($name in @('Start menu ','Calendar ','Sound ','Tray ','Notifications ')){
            InvokePanel $name;RecordPage $name.Trim();InvokePanel 'Back to Panels' -Exact
        }
        BackRoot;InvokePanel 'Bars ';InvokePanel 'Top bar ';RecordPage 'top'
        $modules=@(OwnAll|Where-Object{$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::ListItem -and $_.Current.Name.Contains('Click:')})
        if($modules.Count){InvokeElement $modules[0];RecordPage 'module';InvokePanel 'Back to Top bar' -Exact}
        InvokePanel 'Add a module' -Exact;RecordPage 'add-module';InvokePanel 'Cancel' -Exact
        BackRoot;InvokePanel 'Bars ';InvokePanel 'Dock ';RecordPage 'dock'
        BackRoot;InvokePanel 'Widgets ';RecordPage 'widgets'
        $widgets=@(OwnAll|Where-Object{$_.Current.Name -match 'column ' -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button})
        if(!$widgets.Count){throw 'No widget row is available for the required live capture.'}
        InvokeElement $widgets[0];RecordPage 'widget';$captures+=CaptureOwn 'live-widget.png'
        BackRoot;InvokePanel 'Widgets ';InvokePanel 'Add' -Exact;RecordPage 'add-widget';InvokePanel 'Cancel' -Exact
        BackRoot;InvokePanel 'Wallpaper ';RecordPage 'wallpapers'
        $details=@(OwnAll|Where-Object{$_.Current.Name.EndsWith(' details') -and $_.Current.IsEnabled})
        if($details.Count){InvokeElement $details[0];RecordPage 'wallpaper';InvokePanel 'Back to Wallpaper' -Exact}
        InvokePanel 'Add' -Exact;RecordPage 'add-wallpaper';InvokePanel 'Cancel' -Exact
        BackRoot;InvokePanel 'Windows ';RecordPage 'windows';InvokePanel 'More' -Exact;RecordPage 'windows-more'
        BackRoot;InvokePanel 'Connections ';RecordPage 'connections'
        $items=@(OwnAll|Where-Object{$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and $_.Current.Name -match '^(Now playing|Media|Weather and RSS|T3|Discord) '})
        if($items.Count){InvokeElement $items[0];RecordPage 'connection'}
        BackRoot;InvokePanel 'Settings ';RecordPage 'settings'
        foreach($name in @('General ','Performance and power ','Keyboard ','Troubleshoot ','About ')){
            InvokePanel $name;RecordPage $name.Trim();InvokePanel 'Back to Settings' -Exact
        }
        BackRoot;InvokePanel 'Desktop menu ';RecordPage 'desktop-menu'
        BackRoot;InvokePanel 'History (Ctrl H)' -Exact;InvokePanel 'When Studio opened';RecordPage 'history';$captures+=CaptureOwn 'live-history.png'
        SearchNavigation 'Start from a preset' 'Start from a preset '
        RecordPage 'first-run'
        BackRoot
    }
    $script:process.Refresh()
    $finalMemory=@{workingSetMB=[math]::Round($script:process.WorkingSet64/1MB,2);privateMB=[math]::Round($script:process.PrivateMemorySize64/1MB,2)}
    $drivers=@($script:process.Modules|Where-Object{$_.ModuleName -match '^(nv.*umx|nvgpucomp64|amdxc64|atidx.*)\.dll$'}|ForEach-Object{$_.ModuleName})
    InvokePanel 'Done' -Exact
    if(!$script:process.WaitForExit(5000)){throw 'Done did not close our panel.'}
    $success=$true
} finally {
    if($script:process -and !$script:process.HasExited){[void]$script:process.CloseMainWindow();if(!$script:process.WaitForExit(3000)){Stop-Process -Id $script:process.Id}}
    $after=SettingsHashes
    $changed=@($before.Keys|Where-Object{$before[$_] -ne $after[$_]})
    if($script:errorTask){$script:errorTask.Result|Set-Content -LiteralPath (Join-Path $Shots "$prefix-process-r4.log") -Encoding utf8}
    $report=@{success=$success;fixture=[bool]$Fixture;fixtureRoot=$fixtureRoot;process=$script:process.Id;closed=$script:process.HasExited;exitCode=$script:process.ExitCode;existing=$existing;visited=$script:visited;memory=$script:memory;finalMemory=$finalMemory;gpuVendorDrivers=$drivers;captures=$captures;uia=$uia;changedUserSettings=$changed}
    $report|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $Shots "$prefix-verify-r4.json") -Encoding utf8
    $report|ConvertTo-Json -Depth 8
}
if($changed.Count){throw 'User settings changed during verification.'}
