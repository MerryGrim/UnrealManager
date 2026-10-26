#requires -Version 5.1
# Actual UIA only; fixed About actions never invoke login or another window.
[CmdletBinding()]
param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][long]$CreatedFileTime,[Parameter(Mandatory)][string]$Executable,[Parameter(Mandatory)][string]$ExpectedVersion,[Parameter(Mandatory)][string]$OutputPath)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$started=[DateTime]::UtcNow;$deadline=$started.AddSeconds(30)
[Console]::Out.WriteLine('UIA stage: host-classification-start '+$started.ToString('o'))
# Publish script-entry before the comparatively slow CIM/UIAutomation prelude.
# Only this fresh workspace-owned output is written before full host checking.
if($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or $env:RUNNER_OS -cne 'Windows' -or -not $env:GITHUB_WORKSPACE){throw 'UIA probe is restricted to its hosted job.'}
$allowed=[IO.Path]::GetFullPath((Join-Path $env:GITHUB_WORKSPACE 'verification/current'))+[IO.Path]::DirectorySeparatorChar
if(-not [IO.Path]::GetFullPath($OutputPath).StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase)){throw 'UIA output is outside its workspace evidence scope.'}
# Avoid importing the common helper's cold Add-Type compilation before the
# startup handshake. Validate every ancestor with bounded, local IO first.
$part=[IO.Path]::GetFullPath($OutputPath)
while($part){if(([IO.File]::Exists($part) -or [IO.Directory]::Exists($part)) -and ([IO.File]::GetAttributes($part) -band [IO.FileAttributes]::ReparsePoint)){throw 'Linked UIA output rejected.'};$next=[IO.Path]::GetDirectoryName($part);if($next -eq $part){break};$part=$next}
foreach($candidate in @($OutputPath,($OutputPath+'.stages.jsonl'),($OutputPath+'.started.json'),($OutputPath+'.started.tmp'))){if(Test-Path -LiteralPath $candidate){throw 'UI report/startup evidence already exists.'}}
$self=[Diagnostics.Process]::GetCurrentProcess()
$startup=@{schema=1;pid=$PID;created=$self.StartTime.ToUniversalTime().ToFileTimeUtc();enteredUtc=$started.ToString('o');innerBudgetMilliseconds=30000}
$startupBytes=[Text.UTF8Encoding]::new($false).GetBytes(($startup|ConvertTo-Json -Compress))
$startupFile=[IO.File]::Open(($OutputPath+'.started.tmp'),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
try{$startupFile.Write($startupBytes,0,$startupBytes.Length)}finally{$startupFile.Dispose()}
[IO.File]::Move(($OutputPath+'.started.tmp'),($OutputPath+'.started.json'))
function Stage([string]$Name){
    $line=@{stage=$Name;utc=[DateTime]::UtcNow.ToString('o');elapsedMs=[int]([DateTime]::UtcNow-$started).TotalMilliseconds}|ConvertTo-Json -Compress
    [IO.File]::AppendAllText(($OutputPath+'.stages.jsonl'),$line+[Environment]::NewLine,[Text.UTF8Encoding]::new($false));[Console]::Out.WriteLine($line)
}
Stage 'helper-import-start'
. (Join-Path $PSScriptRoot 'HostedLifecycle.Common.ps1')
Stage 'helper-import-complete'
Stage 'host-classification-start'
$hostScope=Assert-HostedWindowsServer
Stage 'host-classification-complete'
Assert-Plain $Executable
$required=@('Engines','Library','Projects');$pin=@{pid=$ProcessId;created=$CreatedFileTime;path=$Executable}
$report=@{schema=1;ready=$false;method='Actual visible React navigation and version via UIA name or bounded TextPattern range';source='Native EXE/WebView only; no mocked IPC, DOM injection or PE-version readiness fallback';host=$hostScope;matchedButtons=@();versionObserved=$false;versionSource=$null;versionRectangle=$null;aboutActions=@();aboutPatternErrors=0;textPatternErrors=0;error=$null}
function Intersects-Window($Rectangle,$Window){
    foreach($n in @($Rectangle.X,$Rectangle.Y,$Rectangle.Width,$Rectangle.Height,$Window.X,$Window.Y,$Window.Width,$Window.Height)){if([double]::IsNaN($n) -or [double]::IsInfinity($n)){return $false}}
    return $Rectangle.Width -gt 0 -and $Rectangle.Height -gt 0 -and $Window.Width -gt 0 -and $Window.Height -gt 0 -and $Rectangle.X -lt ($Window.X+$Window.Width) -and ($Rectangle.X+$Rectangle.Width) -gt $Window.X -and $Rectangle.Y -lt ($Window.Y+$Window.Height) -and ($Rectangle.Y+$Rectangle.Height) -gt $Window.Y
}
function Version-Matches([string]$Text){return $Text -match ('(?:^|\D)'+[regex]::Escape($ExpectedVersion)+'(?:$|\D)')}
function Test-ActualUiReady([string[]]$Buttons,[bool]$VisibleVersion){return $VisibleVersion -and @($required|Where-Object{$_ -cin $Buttons}).Count -eq 3}
function Invoke-FixedAboutControl($Control,[string]$Name){
    # A Chromium menu button can expose ExpandCollapse rather than Invoke.
    # Unsupported/stale patterns never count as an action or UI readiness.
    if($Name -cnotin @('Account menu','About')){throw 'Unexpected fixed UIA action.'}
    try{
        $pattern=$null
        if($Control.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke();return $true}
        if($Name -ceq 'Account menu' -and $Control.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$pattern)){$pattern.Expand();return $true}
    }catch{$report.aboutPatternErrors++;Stage ('fixed-'+$Name+'-pattern-unavailable');return $false}
    Stage ('fixed-'+$Name+'-pattern-unsupported');return $false
}
try{
 Stage 'automation-assemblies-start';Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes;Stage 'automation-assemblies-loaded'
 $report['apartment']=[string][Threading.Thread]::CurrentThread.GetApartmentState()
 if($report.apartment -cne 'MTA'){throw 'The isolated UIA probe must run in MTA.'}
 do{
    if([DateTime]::UtcNow -ge $deadline){break}
    $p=Find-Pinned $pin;if(-not $p){throw 'Pinned UI process exited before readiness.'};$p.Refresh()
    if($p.MainWindowHandle -ne [IntPtr]::Zero){
        Stage 'window-from-handle';$root=[Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle);$bounds=$root.Current.BoundingRectangle
        $cache=[Windows.Automation.CacheRequest]::new()
        foreach($property in @([Windows.Automation.AutomationElement]::NameProperty,[Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.AutomationElement]::IsOffscreenProperty,[Windows.Automation.AutomationElement]::BoundingRectangleProperty)){$cache.Add($property)}
        $conditions=[Windows.Automation.Condition[]]@(
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Button),
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Text),
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Document),
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::MenuItem))
        $active=$cache.Activate()
        try{Stage 'cached-control-query';$elements=$root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.OrCondition]::new($conditions))}finally{$active.Dispose()}
        $buttons=@();$documents=@();$aboutControl=$null
        $nextAbout=if($report.aboutActions.Count -eq 0){'Account menu'}elseif($report.aboutActions.Count -eq 1){'About'}else{''}
        foreach($element in $elements){try{
            $c=$element.Cached
            if($c.IsOffscreen -or -not(Intersects-Window $c.BoundingRectangle $bounds)){continue}
            if($c.ControlType -eq [Windows.Automation.ControlType]::Button){$buttons+=$c.Name}
            if(Version-Matches $c.Name){$report.versionObserved=$true;$report.versionSource=$(if($report.aboutActions.Count -eq 2){'About visible UIA name'}else{'Visible UIA name'});$report.versionRectangle=@($c.BoundingRectangle.X,$c.BoundingRectangle.Y,$c.BoundingRectangle.Width,$c.BoundingRectangle.Height)}
            if($c.ControlType -eq [Windows.Automation.ControlType]::Document){$documents+=$element}
            if($nextAbout -and $c.Name -ceq $nextAbout -and $c.ControlType -in @([Windows.Automation.ControlType]::Button,[Windows.Automation.ControlType]::MenuItem)){$aboutControl=$element}
        }catch{}}
        $matched=@($required|Where-Object{$_ -cin $buttons});if($matched.Count -eq 3){$report.matchedButtons=$matched}
        foreach($document in $documents){
            if($report.versionObserved){break}
            Stage 'document-text-pattern'
            try{
                $pattern=$null
                if($document.TryGetCurrentPattern([Windows.Automation.TextPattern]::Pattern,[ref]$pattern)){
                    $range=$pattern.DocumentRange
                    if(Version-Matches ($range.GetText(16384))){
                        $match=$range.FindText($ExpectedVersion,$false,$false)
                        if($match){foreach($r in $match.GetBoundingRectangles()){if(Intersects-Window $r $bounds){$report.versionObserved=$true;$report.versionSource=$(if($report.aboutActions.Count -eq 2){'About visible TextPattern range'}else{'Visible TextPattern range'});$report.versionRectangle=@($r.X,$r.Y,$r.Width,$r.Height);break}}}
                    }
                }
            }catch{$report.textPatternErrors++;Stage 'document-text-unavailable'}
        }
        if(Test-ActualUiReady $report.matchedButtons $report.versionObserved){$report.ready=$true;break}
        if($report.matchedButtons.Count -eq 3 -and -not $report.versionObserved -and $aboutControl){
            # Only the two fixed, non-authentication controls in this pinned
            # blank fixture window; revalidate instance immediately before input.
            $live=Find-Pinned $pin;if(-not $live -or $live.MainWindowHandle -ne $p.MainWindowHandle){throw 'Pinned window changed before About action.'}
            if($aboutControl.Current.IsOffscreen -or -not $aboutControl.Current.IsEnabled -or -not(Intersects-Window $aboutControl.Current.BoundingRectangle $bounds)){throw 'Fixed About control is not visible/enabled.'}
            Stage ('invoke-fixed-'+$nextAbout)
             if(Invoke-FixedAboutControl $aboutControl $nextAbout){$report.aboutActions+=@($nextAbout)}
        }
    }
    Start-Sleep -Milliseconds 250
 }while([DateTime]::UtcNow -lt $deadline)
 if(-not $report.ready){$report.error='Actual visible React navigation and version were not both established by UIAutomation.'}
}catch{$report.error=$_.Exception.Message;$report['failureContext']=@{line=$_.InvocationInfo.ScriptLineNumber;stack=$_.ScriptStackTrace}}
Stage 'probe-result';$report['elapsedMs']=[int]([DateTime]::UtcNow-$started).TotalMilliseconds
New-Json $OutputPath $report
if(-not $report.ready){exit 1}
