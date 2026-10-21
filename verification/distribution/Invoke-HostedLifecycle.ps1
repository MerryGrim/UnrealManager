#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$PackagesDirectory,[Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'HostedLifecycle.Common.ps1')
$hostScope=Assert-HostedWindowsServer;$out=Hosted-Output $OutputDirectory
Assert-Plain $PackagesDirectory
$info=Get-Content -LiteralPath (Join-Path $PackagesDirectory 'build-info.json') -Raw|ConvertFrom-Json
if($info.schema -ne 1 -or $info.sourceCommit -cne $env:GITHUB_SHA -or $info.appSha256 -notmatch '^[0-9a-f]{64}$' -or $info.version -notmatch '^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?$'){throw 'CI package provenance mismatch.'}
foreach($file in $info.files){if($file.name -notmatch '^[A-Za-z0-9._-]+$'){throw 'Unsafe package name.'};$path=Join-Path $PackagesDirectory $file.name;if((Get-Item -LiteralPath $path).Length -ne $file.size -or (Test-Hash $path) -cne $file.sha256){throw 'CI package bytes changed.'}}
$setup=Join-Path $PackagesDirectory "UnrealManager-$($info.version)-Windows-x64-Setup.exe";$archive=Join-Path $PackagesDirectory "UnrealManager-$($info.version)-Windows-x64-Portable.zip"
foreach($path in @($setup,$archive)){if(@($info.files|Where-Object{$_.name -ceq [IO.Path]::GetFileName($path)}).Count -ne 1){throw 'Missing fixed CI package.'}}
if(@(Get-CimInstance Win32_Process -Filter "Name='unreal-manager.exe'").Count){throw 'Unexpected existing app process on disposable runner.'}
$profileParent=Join-Path $env:LOCALAPPDATA 'UnrealManager';$data=Join-Path $profileParent 'Data'
if((Test-Path -LiteralPath $profileParent) -or @(App-Registration).Count){throw 'Hosted image already has an app profile/registration; nothing was changed.'}
$id=[guid]::NewGuid().ToString();[void][IO.Directory]::CreateDirectory($out)
foreach($folder in @('reports','logs','temp','fixtures')){[void][IO.Directory]::CreateDirectory((Join-Path $out $folder))}
$env:TEMP=Join-Path $out 'temp';$env:TMP=$env:TEMP
$installerTemp=$env:TEMP;[IO.File]::WriteAllText((Join-Path $installerTemp '.distribution-owner'),$id);$caseStarted=[DateTime]::UtcNow.ToFileTimeUtc()
$report=[ordered]@{schema=1;passed=$false;startedUtc=[DateTime]::UtcNow.ToString('o');host=$hostScope;sourceCommit=$env:GITHUB_SHA;candidateSha256=$info.appSha256;version=$info.version;tests=@();runtimeBefore=@(Runtime-Registration);runtimeFilesBefore=@(Known-RuntimeFiles);missingRuntimeBootstrap='notRun';actualUiReadiness=@();rendering=@();cleanup=@();remainingOwned=@();error=$null;limits=@('Hosted Windows Server images contain developer dependencies; not clean Windows 10/11.','The default hosted account may be elevated with UAC disabled; no standard-user/UAC claim.','No genuine expired/fresh Epic login or key activation is automated.','No physical GPU measurement; only observed software-rendering flags.','No updater signing key/manifest/transition is involved in this lifecycle.','No runtime removal, fake registry hiding, firewall changes or offline network simulation. Runtime file absence is limited to canonical Evergreen install locations.')}
$pins=New-Object System.Collections.Generic.List[object];$installed=Join-Path $out 'fixtures/Installed';$portable=Join-Path $out 'fixtures/Portable';$sentinels=@{};$script:uninstallerPin=$null;$profileOwned=$false
$installerPins=New-Object System.Collections.Generic.List[object];$script:installerTrackingUncertain=$false;$script:appTrackingUncertain=$false;$script:probeTrackingUncertain=$false;$script:lifecycleStage='before-first-setup'
function Observe-TempInstaller{
    # Inno can hand off to a worker after its bootstrap exits. Its image must
    # belong to this fresh GUID-marked temp tree; a process name is never scope.
    Assert-Plain $installerTemp
    if([IO.File]::ReadAllText((Join-Path $installerTemp '.distribution-owner')) -cne $id){throw 'Installer temp owner changed.'}
    $prefix=[IO.Path]::GetFullPath($installerTemp).TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar
    foreach($row in @(Get-CimInstance Win32_Process -OperationTimeoutSec 2)){
        $image=[string]$row.ExecutablePath
        if([string]::IsNullOrWhiteSpace($image) -or -not $image.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)){continue}
        $process=$null
        try{
            if(-not $row.CreationDate -or $row.CreationDate.ToUniversalTime().ToFileTimeUtc() -lt $caseStarted){throw 'Unexpected process predates its owned temp scope.'}
            $process=Get-Process -Id $row.ProcessId -ErrorAction SilentlyContinue;if(-not $process){continue}
            $identity=Process-Identity $process
            if(-not $identity){if($process.HasExited){continue};throw 'Owned temp worker live identity unavailable.'}
            if([Math]::Abs($identity.created-$row.CreationDate.ToUniversalTime().ToFileTimeUtc()) -ge 10 -or -not(Same-Path $identity.path $image)){throw 'Owned temp worker identity changed.'}
            $known=@($installerPins|Where-Object{$_.pid -eq $identity.pid -and $_.created -eq $identity.created})
            if($known.Count){if(-not $known[0].ContainsKey('sha256')){$known[0].sha256=Test-Hash $identity.path;$known[0].scope='fresh-owned-temp'};continue}
            $identity.sha256=Test-Hash $identity.path;$identity.scope='fresh-owned-temp';$installerPins.Add($identity)
        }catch{if($process){$process.Refresh();if($process.HasExited){continue}};$script:installerTrackingUncertain=$true;throw}
    }
}
function Observe-Installer($Pin){
    foreach($row in @(Owned-Tree $Pin)){
        if(@($installerPins|Where-Object{$_.pid -eq $row.pid -and $_.created -eq $row.created}).Count){continue}
        $child=Get-Process -Id $row.pid -ErrorAction SilentlyContinue
        if($child){try{$identity=Process-Identity $child;if(-not $identity){if($child.HasExited){continue};throw 'Installer descendant live image cannot be confirmed.'};if($identity.created -ne $row.created){throw 'Installer descendant identity changed.'};$installerPins.Add($identity)}catch{$script:installerTrackingUncertain=$true;throw}}
    }
}
function Wait-Installer($Process,[string]$Executable,[int]$TimeoutMilliseconds,[string]$CompletionLog,[string]$Kind,[string]$OldData){
    # A timeout must retain profile/fixture data while the real installer can
    # still use it. Track the bootstrap process and observed descendants; never
    # force-kill Setup or an uninstaller to make cleanup appear successful.
    $script:installerTrackingUncertain=$true
    $pin=Pin-Process $Process $Executable;$installerPins.Add($pin)
    $deadline=[DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    $bootstrapExited=$false;$stableSince=$null
    do{
        Observe-TempInstaller
        foreach($known in @($installerPins.ToArray())){if(Find-Pinned $known){Observe-Installer $known}}
        if(-not $bootstrapExited){$bootstrapExited=$Process.WaitForExit(500)}
        if($bootstrapExited){
            $left=@($installerPins|Where-Object{Find-Pinned $_})
            $complete=Inno-LogComplete $CompletionLog $Kind
            $oldPairGone=$Kind -cne 'uninstall' -or (-not(Test-Path -LiteralPath $Executable) -and -not(Test-Path -LiteralPath $OldData))
            if(-not $left.Count -and $complete -and $oldPairGone){
                if($null -eq $stableSince){$stableSince=[DateTime]::UtcNow}
                if(([DateTime]::UtcNow-$stableSince).TotalMilliseconds -ge 500){
                    $Process.Refresh()
                    if(-not $report.Contains('installerWaits')){$report['installerWaits']=@()}
                    $report.installerWaits+=@{operation=$Kind;log=$CompletionLog;logComplete=$complete;oldPairAbsent=$oldPairGone;stableQuiescentMilliseconds=500;observed=@($installerPins.ToArray());finishedUtc=[DateTime]::UtcNow.ToString('o')}
                    $script:installerTrackingUncertain=$false
                    return
                }
            }else{$stableSince=$null}
            # Completion belongs to this operation's log and exact old pair,
            # as well as observed identities. Keep the ORIGINAL deadline.
            Start-Sleep -Milliseconds 100
        }
    }while([DateTime]::UtcNow -lt $deadline)
    throw 'Owned installer timeout; no force-kill attempted. Fixture data will be retained until quiescent.'
}
function Assert-InstallerQuiescent{
    if($script:installerTrackingUncertain){throw 'Installer identity could not be confirmed; no data cleanup.'}
    Observe-TempInstaller
    $left=@($installerPins|Where-Object{Find-Pinned $_})
    if($left.Count){$report.remainingOwned+=@($left|ForEach-Object{@{pid=$_.pid;created=$_.created;kind='installer';path=$_.path}});throw 'Owned installer identities remain; no data cleanup.'}
}
function Step([string]$Name,[bool]$Passed,$Details){$report.tests+=@{name=$Name;passed=$Passed;details=$Details};if(-not $Passed){throw ('Lifecycle assertion failed: '+$Name)}}
function Assert-UiProbeExit($ExitCode){if($null -eq $ExitCode -or $ExitCode -ne 0){throw 'UIA probe did not exit successfully; readiness report is not accepted.'}}
function Wait-UiProbe($Process,[string]$HandshakePath,[DateTime]$LaunchedAt,$State,[int]$StartupMilliseconds=45000,[int]$ProbeMilliseconds=30000){
    $State.startupBudgetMilliseconds=$StartupMilliseconds;$State.probeBudgetMilliseconds=$ProbeMilliseconds
    $created=$Process.StartTime.ToUniversalTime().ToFileTimeUtc();$startupDeadline=$LaunchedAt.AddMilliseconds($StartupMilliseconds)
    $State.created=$created;$State.startupDeadlineUtc=$startupDeadline.ToString('o')
    $handshake=$null
    do{
        if(Test-Path -LiteralPath $HandshakePath){
            Assert-Plain $HandshakePath
            if((Get-Item -LiteralPath $HandshakePath).Length -gt 4096){throw 'UIA startup handshake exceeds bounds.'}
            $handshake=Get-Content -LiteralPath $HandshakePath -Raw|ConvertFrom-Json
            if($handshake.schema -ne 1 -or $handshake.pid -ne $Process.Id -or $handshake.created -ne $created){throw 'UIA startup handshake identity mismatch.'}
            break
        }
        if($Process.WaitForExit(100)){$State.startupElapsedMilliseconds=[int]([DateTime]::UtcNow-$LaunchedAt).TotalMilliseconds;return $true}
    }while([DateTime]::UtcNow -lt $startupDeadline)
    if(-not $handshake){$State.startupElapsedMilliseconds=[int]([DateTime]::UtcNow-$LaunchedAt).TotalMilliseconds;$State.timeoutPhase='startup';return $false}
    $entered=if($handshake.enteredUtc -is [DateTime]){$handshake.enteredUtc.ToUniversalTime()}else{[DateTime]::Parse([string]$handshake.enteredUtc,[Globalization.CultureInfo]::InvariantCulture,[Globalization.DateTimeStyles]::RoundtripKind).ToUniversalTime()}
    if($entered -lt [DateTime]::FromFileTimeUtc($created) -or $entered -gt [DateTime]::UtcNow -or $entered -gt $startupDeadline){throw 'UIA startup handshake timestamp outside its original deadline.'}
    $State.handshakeUtc=$entered.ToString('o');$State.startupElapsedMilliseconds=[int]($entered-$LaunchedAt).TotalMilliseconds
    $probeDeadline=$entered.AddMilliseconds($ProbeMilliseconds)
    $State.probeDeadlineUtc=$probeDeadline.ToString('o');$State.innerBudgetMilliseconds=$handshake.innerBudgetMilliseconds
    do{
        if($Process.WaitForExit(100)){$State.probeElapsedMilliseconds=[int]([DateTime]::UtcNow-$entered).TotalMilliseconds;return $true}
    }while([DateTime]::UtcNow -lt $probeDeadline)
    $State.probeElapsedMilliseconds=[int]([DateTime]::UtcNow-$entered).TotalMilliseconds;$State.timeoutPhase='probe';return $false
}
function Seed-Data([string]$Root){
    Assert-Plain $Root;foreach($part in @('Settings','Account','Tasks')){[void][IO.Directory]::CreateDirectory((Join-Path $Root $part))}
    [IO.File]::WriteAllText((Join-Path $Root '.distribution-owner'),$id)
    New-Json (Join-Path $Root 'Settings/window.json') 'exit'
    New-Json (Join-Path $Root 'Settings/app-updates.json') @{enabled=$false;channel='stable';lastAttempt=0}
    New-Json (Join-Path $Root 'Settings/preferences.json') @{'unreal-manager.language.v1'='en'}
    foreach($part in @('Account/session.fixture','Tasks/task.fixture','user-project.fixture')){$file=Join-Path $Root $part;[IO.File]::WriteAllText($file,('INERT CI PRESERVATION SENTINEL '+$id+' '+$part));$sentinels[$file]=Test-Hash $file}
}
function Preserved{foreach($file in $sentinels.Keys){if(-not(Test-Path -LiteralPath $file) -or (Test-Hash $file) -cne $sentinels[$file]){throw 'Synthetic sentinel changed.'}}}
function Invoke-Setup([string]$Name){
    $script:lifecycleStage=$Name+'-setup'
    Assert-Plain $installed;$log=Join-Path $out ('logs/'+$Name+'.log')
    $p=Start-Process -FilePath $setup -ArgumentList @('/SP-','/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/CURRENTUSER','/LANG=en',('/DIR="'+$installed+'"'),('/LOG="'+$log+'"')) -WindowStyle Hidden -PassThru
    Wait-Installer $p $setup 600000 $log 'setup' ''
    Assert-InstallerQuiescent
    Step $Name ($p.ExitCode -eq 0 -and (Test-Hash (Join-Path $installed 'unreal-manager.exe')) -ceq $info.appSha256 -and -not(Test-Path -LiteralPath (Join-Path $installed 'portable.marker'))) @{exitCode=$p.ExitCode;runtimeAfter=@(Runtime-Registration)}
    $pair=Registered-Uninstaller $installed @(App-Registration)
    $script:uninstallerPin=@{exe=$pair.exe;dat=$pair.dat;exeSha256=(Test-Hash $pair.exe);datSha256=(Test-Hash $pair.dat)}
    Preserved
}
function Invoke-Uninstall([string]$Name){
    $script:lifecycleStage=$Name+'-uninstall'
    $pair=Registered-Uninstaller $installed @(App-Registration)
    if(-not $script:uninstallerPin -or -not(Same-Path $pair.exe $script:uninstallerPin.exe) -or -not(Same-Path $pair.dat $script:uninstallerPin.dat) -or (Test-Hash $pair.exe) -cne $script:uninstallerPin.exeSha256 -or (Test-Hash $pair.dat) -cne $script:uninstallerPin.datSha256){throw 'Owned uninstaller EXE/DAT or registry pointer changed.'}
    $log=Join-Path $out ('logs/'+$Name+'.log')
    $p=Start-Process -FilePath $pair.exe -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG="'+$log+'"')) -WindowStyle Hidden -PassThru
    Wait-Installer $p $pair.exe 180000 $log 'uninstall' $pair.dat
    Assert-InstallerQuiescent
    Preserved;Step $Name ($p.ExitCode -eq 0 -and -not(Test-Path -LiteralPath (Join-Path $installed 'unreal-manager.exe')) -and @(App-Registration).Count -eq 0) @{exitCode=$p.ExitCode;dataAndSyntheticSentinelsPreserved=$true}
}
function Close-Own($Pin){
    $script:appTrackingUncertain=$true
    $close=@{pid=$Pin.pid;created=$Pin.created;requested=$false;exitCode=$null;descendantsAbsent=$false}
    if(-not $report.Contains('normalCloses')){$report['normalCloses']=@()};$report.normalCloses+=@($close)
    $exitHandle=Open-OwnedExitHandle $Pin
    try{
    $tree=@(Owned-Tree $Pin);$p=Find-Pinned $Pin
    if(-not $p -or $exitHandle.Wait(0)){throw 'Owned app exited before the normal-close request; this is not a normal-close pass.'}
    $close.requested=$p.CloseMainWindow()
    if(-not $close.requested -or -not $exitHandle.Wait(15000)){throw 'Owned app did not close normally; no force-kill attempted.'}
    $close.exitCode=$exitHandle.ExitCode();$close.exitCodeSource='GetExitCodeProcess on retained identity-verified Win32 handle'
    if($close.exitCode -ne 0){throw ('Owned app normal-close exit code was '+$close.exitCode+'.')}
    $deadline=[DateTime]::UtcNow.AddSeconds(10)
    do{$left=@();foreach($row in $tree){$child=Get-Process -Id $row.pid -ErrorAction SilentlyContinue;if($child -and $child.StartTime.ToUniversalTime().ToFileTimeUtc() -eq $row.created){$left+=$row}};if($left.Count){Start-Sleep -Milliseconds 200}}while($left.Count -and [DateTime]::UtcNow -lt $deadline)
    if($left.Count){$report.remainingOwned=$left;throw 'Observed owned descendant remains; fixture data retained.'}
    $close.descendantsAbsent=$true;$script:appTrackingUncertain=$false
    }finally{$exitHandle.Dispose()}
}
function Launch-And-Verify([string]$Name,[string]$Folder){
    $script:lifecycleStage=$Name+'-launch-and-pin'
    $exe=Join-Path $Folder 'unreal-manager.exe';Step ($Name+'-exact-image') ((Test-Hash $exe) -ceq $info.appSha256) @{}
    $script:appTrackingUncertain=$true
    $p=$null
    try{
        $p=Start-Process -FilePath $exe -WorkingDirectory $Folder -WindowStyle Normal -RedirectStandardOutput (Join-Path $out ('logs/'+$Name+'-app.stdout.log')) -RedirectStandardError (Join-Path $out ('logs/'+$Name+'-app.stderr.log')) -PassThru
        $pin=Pin-Process $p $exe -FreshImage;$pins.Add($pin)
        if((Test-Hash $exe) -cne $info.appSha256){throw 'Owned app image changed during launch.'}
    }catch{
        $failure=$_;$diagnostic=@{mode=$Name;expectedImage=$exe;expectedSha256=$info.appSha256;pid=$null;created=$null;hasExited=$null;exitCode=$null;identityFailure=$failure.Exception.Message}
        if($p){$diagnostic.pid=$p.Id;try{$p.Refresh();$diagnostic.hasExited=$p.HasExited;if($p.HasExited){$diagnostic.exitCode=$p.ExitCode}else{$diagnostic.created=$p.StartTime.ToUniversalTime().ToFileTimeUtc()}}catch{$diagnostic['stateReadError']=$_.Exception.Message}}
        $report['failedLaunch']=$diagnostic;throw $failure
    }
    $script:lifecycleStage=$Name+'-actual-ui-readiness'
    $ui=Join-Path $out ('reports/'+$Name+'-ui.json');$ps=Join-Path $env:WINDIR 'System32/WindowsPowerShell/v1.0/powershell.exe'
    $probeLaunched=[DateTime]::UtcNow
    $script:probeTrackingUncertain=$true
    $probe=Start-Process -FilePath $ps -ArgumentList @('-NoProfile','-MTA','-ExecutionPolicy','Bypass','-File',('"'+(Join-Path $PSScriptRoot 'Get-HostedUiReadiness.ps1')+'"'),'-ProcessId',$pin.pid,'-CreatedFileTime',$pin.created,'-Executable',('"'+$exe+'"'),'-ExpectedVersion',$info.version,'-OutputPath',('"'+$ui+'"')) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $out ('logs/'+$Name+'-uia.stdout.log')) -RedirectStandardError (Join-Path $out ('logs/'+$Name+'-uia.stderr.log')) -PassThru
    $probeState=@{mode=$Name;pid=$probe.Id;startedUtc=$probeLaunched.ToString('o');timedOut=$false;timeoutPhase=$null;exitCode=$null;lastCheckpoint=$null}
    $report['uiaProbe']=$probeState
    $exited=Wait-UiProbe $probe ($ui+'.started.json') $probeLaunched $probeState
    if(-not $exited){$probeState.timedOut=$true;$probe.Kill();[void]$probe.WaitForExit(5000)}
    $probe.Refresh();if($probe.HasExited){$probeState.exitCode=$probe.ExitCode;$script:probeTrackingUncertain=$false}else{throw 'Owned UIA probe termination could not be confirmed; fixture data retained.'}
    $stageFile=$ui+'.stages.jsonl'
    if(Test-Path -LiteralPath $stageFile){$probeState.lastCheckpoint=Get-Content -LiteralPath $stageFile -Tail 1|ConvertFrom-Json}
    if(-not $exited){throw 'Own UIA test probe timed out; app was not force-killed. Last stage and probe output retained.'}
    Assert-UiProbeExit $probeState.exitCode
    if(-not(Test-Path -LiteralPath $ui)){throw 'UIA probe produced no readiness report.'}
    $uiResult=Get-Content -LiteralPath $ui -Raw|ConvertFrom-Json;$report.actualUiReadiness+=@{mode=$Name;ready=$uiResult.ready;method=$uiResult.method}
    $tree=@(Owned-Tree $pin);$browsers=@($tree|Where-Object{$_.browserRoot});$unknown=@($tree|Where-Object{$_.name -ieq 'msedgewebview2.exe' -and (-not $_.commandLineReadable -or -not $_.commandLineParsed -or $_.processType -ceq 'unknown')})
    $flags=$browsers.Count -gt 0 -and -not $unknown.Count -and @($browsers|Where-Object{-not $_.disableGpu -or -not $_.disableCompositing}).Count -eq 0
    $report.rendering+=@{mode=$Name;observedUtc=[DateTime]::UtcNow.ToString('o');browserRoots=$browsers.Count;unknownCommandLines=$unknown.Count;bothSoftwareFlags=$flags;rows=@($tree|Where-Object{$_.name -ieq 'msedgewebview2.exe'});scope='Configuration only. Exact Windows-token policy; valued forms are diagnostics and do not pass. No raw command line/URLs or physical GPU claim.'}
    Close-Own $pin;Preserved
    Step ($Name+'-actual-react-ui') ([bool]$uiResult.ready) @{report=$ui;processCreationAloneNotAccepted=$true}
    Step ($Name+'-software-flags') $flags @{}
    Step ($Name+'-normal-close-and-preservation') $true @{}
}
try{
    [void][IO.Directory]::CreateDirectory($profileParent);[IO.File]::WriteAllText((Join-Path $profileParent '.distribution-owner'),$id);$profileOwned=$true;Seed-Data $data
    [void][IO.Directory]::CreateDirectory($installed);Invoke-Setup 'installed-first'
    $report.missingRuntimeBootstrap=if($report.runtimeBefore.Count -eq 0 -and $report.runtimeFilesBefore.Count -eq 0){'Actual online Setup path with absent runtime registry and canonical files completed; nothing was hidden or removed.'}elseif($report.runtimeBefore.Count -eq 0){'Only missing-registration repair path observed; existing runtime files prevent a genuine missing-runtime claim.'}else{'NotRun: runtime already registered on this hosted image; no runtime was uninstalled.'}
    Launch-And-Verify 'installed-first' $installed;Invoke-Uninstall 'uninstall-first';Invoke-Setup 'installed-reinstall';Launch-And-Verify 'installed-reinstall' $installed;Invoke-Uninstall 'uninstall-final'
    [void][IO.Directory]::CreateDirectory($portable)
    $zip=[IO.Compression.ZipFile]::OpenRead($archive);$names=@('unreal-manager.exe','README.txt','LICENSE','THIRD_PARTY_NOTICES.md','THIRD-PARTY-LICENSES.txt','portable.marker');$seen=@{}
    try{if($zip.Entries.Count -ne 6){throw 'Portable ZIP fixed-six layout mismatch.'};foreach($entry in $zip.Entries){if($entry.FullName -cnotmatch '^UnrealManager-Portable/([^/\\]+)$'){throw 'Unsafe ZIP entry.'};$name=$Matches[1];if($name -cnotin $names -or $seen.ContainsKey($name) -or $entry.Length -le 0 -or $entry.Length -gt 64MB){throw 'Unexpected portable ZIP member.'};$seen[$name]=$true;[IO.Compression.ZipFileExtensions]::ExtractToFile($entry,(Join-Path $portable $name),$false)}}finally{$zip.Dispose()}
    [IO.File]::WriteAllText((Join-Path $portable '.distribution-owner'),$id);Seed-Data (Join-Path $portable 'Data')
    $marker=Test-Hash (Join-Path $portable 'portable.marker');Launch-And-Verify 'portable' $portable;Step 'portable-marker-preserved' ((Test-Hash (Join-Path $portable 'portable.marker')) -ceq $marker) @{}
    $report.passed=$true
}catch{$report.error=$_.Exception.Message;$report['failureContext']=@{stage=$script:lifecycleStage;line=$_.InvocationInfo.ScriptLineNumber;script=[IO.Path]::GetFileName($_.InvocationInfo.ScriptName);stack=$_.ScriptStackTrace}}
finally{
    try{
        foreach($pin in $pins){if(Find-Pinned $pin){Close-Own $pin}}
        if($script:appTrackingUncertain){throw 'Launched app identity is uncertain; no uninstall or fixture data cleanup.'}
        if($script:probeTrackingUncertain){throw 'UIA probe identity remains live or uncertain; no fixture data cleanup.'}
        if($report.remainingOwned.Count){throw 'Owned descendant identities remain; no data cleanup.'}
        Assert-InstallerQuiescent
        if(@(App-Registration).Count -and $script:uninstallerPin){Invoke-Uninstall 'cleanup-uninstall'}
        Assert-InstallerQuiescent
        if(@(App-Registration).Count){throw 'App uninstall registration remains; profile retained.'}
        if($profileOwned){$report.cleanup+=@{target='own blank installed profile';result=(Remove-OwnedTree $profileParent (Join-Path $env:LOCALAPPDATA 'UnrealManager') $id)}}
        if(Test-Path -LiteralPath (Join-Path $portable '.distribution-owner')){$report.cleanup+=@{target='own portable fixture';result=(Remove-OwnedTree $portable (Join-Path $out 'fixtures/Portable') $id)}}
    }catch{$report.passed=$false;$report.cleanup+=@{failed=$true;error=$_.Exception.Message}}
    $report.finishedUtc=[DateTime]::UtcNow.ToString('o');New-Json (Join-Path $out 'reports/lifecycle-report.json') $report
}
if(-not $report.passed){throw ('Hosted lifecycle failed: '+$report.error)}
