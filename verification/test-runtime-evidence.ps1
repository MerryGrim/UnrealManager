#requires -Version 7.0
[CmdletBinding()]
param([string]$ReplayReport)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'RuntimeEvidence.ps1')
$id=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH-mm-ss-fff')+'-'+[guid]::NewGuid().ToString('N').Substring(0,8)
$directory=Join-Path $PSScriptRoot ('current/runtime-evidence-tests-'+$id)
$null=New-Item -ItemType Directory -Path $directory
$checks=[Collections.Generic.List[object]]::new()
$failure=$null;$child=$null;$identity=$null;$correction=$null
function Check([string]$Name,[scriptblock]$Action){& $Action;$checks.Add([pscustomobject]@{name=$Name;passed=$true})}
function Assert([bool]$Condition,[string]$Message){if(-not $Condition){throw $Message}}
function Row([int]$Id,[string]$Created,$CommandLine){[pscustomobject]@{Name='msedgewebview2.exe';ProcessId=$Id;ParentProcessId=100;CreatedAt=$Created;CommandLine=$CommandLine}}
function Sample([object[]]$Rows){[pscustomobject]@{Timestamp='2026-10-21T00:00:00Z';Processes=$Rows}}
$created='2026-10-21T00:00:00.123456Z';$flags='webview.exe --disable-gpu --disable-gpu-compositing'
try{
    Check 'Positive browser followed by unreadable same identity keeps both history and flag evidence' {
        $e=Get-UmSoftwareRenderingEvidence @((Sample @((Row 200 $created $flags))),(Sample @((Row 200 $created $null)))) 100
        Assert $e.SoftwareFlagsObserved 'Known identity lost its readable history';Assert ($e.UnreadableRootObservations.Count -eq 1) 'Raw unknown was lost';Assert ($e.CoveredUnreadableRootObservationCount -eq 1) 'Unknown identity was not linked'
    }
    Check 'Unreadable never-classified root candidate prevents pass' {
        $e=Get-UmSoftwareRenderingEvidence @((Sample @((Row 200 $created $flags),(Row 201 $created $null)))) 100
        Assert (-not $e.SoftwareFlagsObserved) 'Uncovered unknown was called configured';Assert ($e.UncoveredUnreadableRootObservations.Count -eq 1) 'Uncovered unknown missing'
    }
    Check 'Any explicit wrong flags fail despite later positive or null observations' {
        $e=Get-UmSoftwareRenderingEvidence @((Sample @((Row 200 $created 'webview.exe --disable-gpu'))),(Sample @((Row 200 $created $flags))),(Sample @((Row 200 $created $null)))) 100
        Assert (-not $e.SoftwareFlagsObserved) 'Explicit invalid flags were overwritten';Assert ($e.InvalidRootObservations.Count -eq 1) 'Wrong-flags history missing'
    }
    Check 'A reused PID cannot cover an unreadable new identity' {
        $e=Get-UmSoftwareRenderingEvidence @((Sample @((Row 200 $created $flags))),(Sample @((Row 200 '2026-10-21T00:00:01.123456Z' $null)))) 100
        Assert (-not $e.SoftwareFlagsObserved) 'PID reuse inherited old flags'
    }
    Check 'No observations and typed child processes are not positive root evidence' {
        Assert (-not (Get-UmSoftwareRenderingEvidence @() 100).SoftwareFlagsObserved) 'No samples claimed configured'
        $e=Get-UmSoftwareRenderingEvidence @((Sample @((Row 200 $created 'webview.exe "--type=renderer" --disable-gpu --disable-gpu-compositing')))) 100
        Assert (-not $e.SoftwareFlagsObserved) 'Typed child was called root'
    }
    Check 'UTC string and typed UTC timestamps produce the same exact identity' {
        $utc=[DateTime]::Parse($created,[Globalization.CultureInfo]::InvariantCulture,[Globalization.DateTimeStyles]::RoundtripKind)
        Assert ((Get-UmCreationTicks $utc) -eq (Get-UmCreationTicks $created)) 'Typed UTC lost precision/timezone'
    }
    if($IsWindows){
        Check 'Returned native process identity rejects altered time or image without acting on a PID' {
            $exe=(Get-Process -Id $PID).Path
            $script:TestChild=Start-Process -FilePath $exe -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60') -WindowStyle Hidden -PassThru
            $script:TestIdentity=Get-UmLaunchedProcessIdentity $script:TestChild $exe
            Assert ($null -ne $script:TestIdentity) 'Test child exited before attachment'
            Assert ((Get-UmVerifiedLaunchedProcess $script:TestChild $script:TestIdentity).Id -eq $script:TestChild.Id) 'Native handle was not validated'
            $forged=[pscustomobject]@{ProcessId=$script:TestIdentity.ProcessId;StartTicks=$script:TestIdentity.StartTicks+1;ExecutablePath=$script:TestIdentity.ExecutablePath}
            $rejected=$false;try{Get-UmVerifiedLaunchedProcess $script:TestChild $forged|Out-Null}catch{$rejected=$true};Assert $rejected 'Changed creation time passed'
            $forged.StartTicks=$script:TestIdentity.StartTicks;$forged.ExecutablePath=Join-Path $directory 'not-the-child.exe'
            $rejected=$false;try{Get-UmVerifiedLaunchedProcess $script:TestChild $forged|Out-Null}catch{$rejected=$true};Assert $rejected 'Changed image passed'
        }
        $child=$script:TestChild;$identity=$script:TestIdentity
        Check 'Extracted actual cleanup closes only returned owned child when CIM attachment never worked' {
            $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'measure-runtime.ps1'),[ref]$tokens,[ref]$errors)
            Assert ($errors.Count -eq 0) 'Collector has syntax errors'
            foreach($name in @('Get-LiveOwnedProcess','Close-OwnedTree')){
                $definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
                Assert ($null -ne $definition) ('Missing collector function '+$name)
                # Only the graceful deadline is shortened in this isolated
                # no-window child test. Identity/action branches stay exact.
                Invoke-Expression ($definition.Extent.Text.Replace('UtcNow.AddSeconds(15)','UtcNow.AddMilliseconds(100)'))
            }
            $script:CimCalls=0
            function Get-CimInstance{$script:CimCalls++;throw 'Simulated unavailable CIM'}
            function Get-OwnedTree{return @()}
            $script:LaunchedProcess=$child;$script:LaunchedNativeIdentity=$identity;$script:OwnedIdentities=@{}
            $script:ChildCleanup=Close-OwnedTree
            $child.Refresh();Assert $child.HasExited 'Attach-failure left the owned child running'
            Assert $script:ChildCleanup.NativeRootFallbackUsed 'Fallback was not exercised';Assert ($script:CimCalls -eq 0) 'Root cleanup depended on CIM'
            Assert ($script:ChildCleanup.Errors.Count -eq 0) 'Child cleanup reported errors'
            Assert (@($script:ChildCleanup.ForcedProcessIds|Where-Object{$_ -ne $child.Id}).Count -eq 0) 'Cleanup acted on another process'
        }
    }
    if($ReplayReport){
        $path=(Resolve-Path -LiteralPath $ReplayReport).ProviderPath
        $current=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'current'))+[IO.Path]::DirectorySeparatorChar
        if(-not $path.StartsWith($current,[StringComparison]::OrdinalIgnoreCase)){throw 'Replay input must be under verification/current.'}
        $before=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant();$runtime=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
        $roots=@($runtime.Samples|ForEach-Object{$_.Processes}|Where-Object{[string]::Equals($_.ExecutablePath,$runtime.IsolatedExecutable,[StringComparison]::OrdinalIgnoreCase)}|ForEach-Object{[pscustomobject]@{ProcessId=$_.ProcessId;Ticks=(Get-UmCreationTicks $_.CreatedAt)}}|Sort-Object ProcessId,Ticks -Unique)
        if($roots.Count -ne 1 -or $null -eq $roots[0].Ticks){throw 'Replay cannot bind a unique isolated native root identity.'}
        $evidence=Get-UmSoftwareRenderingEvidence -Samples @($runtime.Samples) -RootProcessId $roots[0].ProcessId
        if((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $before){throw 'Replay report changed while reading.'}
        $correction=Join-Path $directory 'runtime-observation-history-correction.json'
        [ordered]@{schema=1;createdUtc=[DateTime]::UtcNow.ToString('o');rawReport=$path;rawSha256=$before;helperSha256=(Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'RuntimeEvidence.ps1') -Algorithm SHA256).Hash.ToLowerInvariant();originalSoftwareFlagsObserved=$runtime.SoftwareFlagsObserved;samplingCompleted=$runtime.SamplingCompleted;sampleCount=@($runtime.Samples).Count;nativeExitAfterCleanup=$runtime.NativeExitAfterCleanup;originalCleanup=$runtime.Cleanup;softwareRenderingEvidence=$evidence;limitations='Read-only reinterpretation of existing owned-process observations only. Raw report unchanged. No new app launch, workload, GPU measurement or authentication acceptance.'}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $correction -Encoding utf8
    }
}catch{$failure=$_.Exception.Message}
finally{
    if($null -ne $child){$owned=Get-UmVerifiedLaunchedProcess $child $identity;if($null -ne $owned){$owned.Kill();$owned.WaitForExit()}}
    elseif(Get-Variable TestChild -Scope Script -ErrorAction SilentlyContinue){$owned=Get-UmVerifiedLaunchedProcess $script:TestChild $script:TestIdentity;if($null -ne $owned){$owned.Kill();$owned.WaitForExit()}}
    [ordered]@{schema=1;passed=$null -eq $failure;createdUtc=[DateTime]::UtcNow.ToString('o');checks=@($checks.ToArray());failure=$failure;correction=$correction;scope='Synthetic observation fixtures and one owned PowerShell sleep child only. No Unreal Manager or user profile was opened.';helperSha256=(Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'RuntimeEvidence.ps1') -Algorithm SHA256).Hash.ToLowerInvariant();collectorSha256=(Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'measure-runtime.ps1') -Algorithm SHA256).Hash.ToLowerInvariant()}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $directory 'test-report.json') -Encoding utf8
    Write-Output $directory
}
if($failure){throw $failure}
