#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'current'))+[IO.Path]::DirectorySeparatorChar
$out=[IO.Path]::GetFullPath($OutputDirectory)
if(-not $out.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $out)){throw 'Use a fresh verification/current output directory.'}
[void][IO.Directory]::CreateDirectory($out)
$checks=[Collections.Generic.List[string]]::new()
function Assert([bool]$Value,[string]$Message){if(-not $Value){throw $Message}}
function Load-Function([string]$File,[string]$Name){
    $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile($File,[ref]$tokens,[ref]$errors)
    Assert ($errors.Count -eq 0) ('PowerShell syntax: '+$File)
    $definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $Name},$true)
    Assert ($null -ne $definition) ('Missing function: '+$Name)
    # Run only the pure/controlled functions; never execute the hosted scripts.
    . ([scriptblock]::Create(($definition.Extent.Text -replace ('^function\s+'+[regex]::Escape($Name)),('function global:'+$Name))))
}
Load-Function (Join-Path $PSScriptRoot 'distribution/Get-HostedUiReadiness.ps1') 'Test-ActualUiReady'
Load-Function (Join-Path $PSScriptRoot 'distribution/Get-HostedUiReadiness.ps1') 'Invoke-FixedAboutControl'
Load-Function (Join-Path $PSScriptRoot 'distribution/Invoke-HostedLifecycle.ps1') 'Wait-UiProbe'
Load-Function (Join-Path $PSScriptRoot 'distribution/HostedLifecycle.Common.ps1') 'Assert-Plain'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$script:stages=[Collections.Generic.List[string]]::new()
function Stage([string]$Name){$script:stages.Add($Name)}
$required=@('Engines','Library','Projects');$report=@{aboutPatternErrors=0}
function Fake-Control([bool]$Invoke,[bool]$Expand,[bool]$Throw){
    $pattern=[pscustomobject]@{invoked=0;expanded=0}
    $pattern|Add-Member ScriptMethod Invoke {$this.invoked++}
    $pattern|Add-Member ScriptMethod Expand {$this.expanded++}
    $control=[pscustomobject]@{canInvoke=$Invoke;canExpand=$Expand;throws=$Throw;pattern=$pattern}
    $control|Add-Member ScriptMethod TryGetCurrentPattern {
        param($Requested,[ref]$Result)
        if($this.throws){throw 'Unsupported Pattern.'}
        $supported=($Requested.Id -eq [Windows.Automation.InvokePattern]::Pattern.Id -and $this.canInvoke) -or ($Requested.Id -eq [Windows.Automation.ExpandCollapsePattern]::Pattern.Id -and $this.canExpand)
        if($supported){$Result.Value=$this.pattern;return $true};return $false
    }
    return $control
}
$control=Fake-Control $true $true $false
Assert (Invoke-FixedAboutControl $control 'About') 'Supported Invoke action was not performed'
Assert ($control.pattern.invoked -eq 1 -and $control.pattern.expanded -eq 0) 'Invoke must be used when available'
$checks.Add('Supported fixed Invoke succeeds without expanding another pattern')
$control=Fake-Control $false $true $false
Assert (Invoke-FixedAboutControl $control 'Account menu') 'Account ExpandCollapse fallback failed'
Assert ($control.pattern.expanded -eq 1) 'Account menu was not expanded'
Assert (-not(Invoke-FixedAboutControl $control 'About')) 'An About menu item must not use the Account expansion fallback'
$checks.Add('Only the fixed Account menu may use ExpandCollapse; About still requires Invoke')
$control=Fake-Control $false $false $true
Assert (-not(Invoke-FixedAboutControl $control 'Account menu')) 'Unsupported Pattern was counted as success'
Assert ($report.aboutPatternErrors -eq 1 -and $control.pattern.invoked -eq 0) 'Unsupported action must remain diagnostic only'
Assert ($stages -contains 'fixed-Account menu-pattern-unavailable') 'Unsupported milestone missing'
$checks.Add('Unsupported Pattern returns false with a milestone and never counts an action')
$rejected=$false;try{Invoke-FixedAboutControl $control 'Login'|Out-Null}catch{$rejected=$true}
Assert $rejected 'An arbitrary action name was accepted'
Assert (-not(Test-ActualUiReady @('Engines','Library','Projects') $false)) 'Navigation alone passed readiness'
Assert (-not(Test-ActualUiReady @('Engines','Library') $true)) 'Partial navigation passed readiness'
Assert (Test-ActualUiReady @('Engines','Library','Projects') $true) 'Complete actual UI evidence rejected'
$checks.Add('Only the two fixed actions are allowed; all three navigation buttons and visible version remain required')
$launched=[DateTime]::UtcNow.AddSeconds(-60);$created=$launched.AddMilliseconds(1)
$process=[pscustomobject]@{Id=4321;StartTime=$created}
$process|Add-Member ScriptMethod WaitForExit {param($Milliseconds)return $true}
$handshake=Join-Path $out 'probe.started.json'
$entry=[DateTime]::UtcNow.AddSeconds(-1)
$value=@{schema=1;pid=$process.Id;created=$created.ToFileTimeUtc();enteredUtc=$entry.ToString('o');innerBudgetMilliseconds=30000}
[IO.File]::WriteAllText($handshake,($value|ConvertTo-Json -Compress))
$state=@{}
Assert (Wait-UiProbe $process $handshake $launched $state) 'A valid slow shell startup was rejected before its readiness budget'
Assert ($state.startupBudgetMilliseconds -eq 90000 -and $state.probeBudgetMilliseconds -eq 35000 -and $state.innerBudgetMilliseconds -eq 30000) 'Readiness budgets changed unexpectedly'
$checks.Add('A valid 59-second shell startup retains the bounded30-second readiness budget with5-second exit grace')
$value.pid++;[IO.File]::WriteAllText($handshake,($value|ConvertTo-Json -Compress));$rejected=$false
try{Wait-UiProbe $process $handshake $launched @{}|Out-Null}catch{$rejected=$true}
Assert $rejected 'Forged startup identity passed'
$value.pid--;$value.innerBudgetMilliseconds=25000;[IO.File]::WriteAllText($handshake,($value|ConvertTo-Json -Compress));$rejected=$false
try{Wait-UiProbe $process $handshake $launched @{}|Out-Null}catch{$rejected=$true}
Assert $rejected 'A different inner budget passed'
$checks.Add('PID/time-bound startup identity and the fixed inner budget cannot be replaced by a forged handshake')
$value.innerBudgetMilliseconds=30000;[IO.File]::WriteAllText($handshake,($value|ConvertTo-Json -Compress));$rejected=$false
try{Wait-UiProbe $process $handshake ([DateTime]::UtcNow.AddSeconds(-100)) @{}|Out-Null}catch{$rejected=$true}
Assert $rejected 'A late handshake outside the original startup deadline passed'
$checks.Add('A late handshake cannot extend the original startup deadline')
$result=@{schema=1;passed=$true;checks=@($checks);scope='Extracted actual helper functions with synthetic controls and startup handshakes only. No app, installer, real UI action or hosted acceptance executed.'}
[IO.File]::WriteAllText((Join-Path $out 'hosted-readiness-self-check.json'),($result|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
$result|ConvertTo-Json -Depth 6
