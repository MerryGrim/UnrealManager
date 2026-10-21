#requires -Version 5.1
# Read actual UIA from a pinned test-owned window; no input, DOM injection or CDP.
[CmdletBinding()]
param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][long]$CreatedFileTime,[Parameter(Mandatory)][string]$Executable,[Parameter(Mandatory)][string]$ExpectedVersion,[Parameter(Mandatory)][string]$OutputPath)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'HostedLifecycle.Common.ps1')
$hostScope=Assert-HostedWindowsServer
Assert-Plain $Executable;Assert-Plain $OutputPath
if(Test-Path -LiteralPath $OutputPath){throw 'UI report exists.'}
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$required=@('Engines','Library','Projects')
$report=@{schema=1;ready=$false;method='Windows UIAutomation: real React navigation button names and visible app version';source='Actual native EXE/WebView, no mocked IPC or injected script';host=$hostScope;matchedButtons=@();versionObserved=$false;error=$null}
$deadline=[DateTime]::UtcNow.AddSeconds(25)
try{
 do{
    $p=Get-Process -Id $ProcessId -ErrorAction Stop
    if($p.StartTime.ToUniversalTime().ToFileTimeUtc() -ne $CreatedFileTime -or -not(Same-Path $p.Path $Executable)){throw 'UI process identity changed.'}
    $p.Refresh()
    if($p.MainWindowHandle -ne [IntPtr]::Zero){
        $root=[Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
        $elements=$root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
        $names=@();$buttons=@()
        foreach($element in $elements){try{$name=$element.Current.Name;if($name){$names+=$name;if($element.Current.ControlType -eq [Windows.Automation.ControlType]::Button){$buttons+=$name}}}catch{}}
        $report.matchedButtons=@($required|Where-Object{$_ -cin $buttons})
        $report.versionObserved=@($names|Where-Object{$_ -match ('(?:^|\D)'+[regex]::Escape($ExpectedVersion)+'(?:$|\D)')}).Count -gt 0
        if($report.matchedButtons.Count -eq $required.Count -and $report.versionObserved){$report.ready=$true;break}
    }
    Start-Sleep -Milliseconds 250
 }while([DateTime]::UtcNow -lt $deadline)
 if(-not $report.ready){$report.error='Actual React UI was not established by UIAutomation in this hosted desktop session.'}
}catch{$report.error=$_.Exception.Message}
New-Json $OutputPath $report
if(-not $report.ready){exit 1}
