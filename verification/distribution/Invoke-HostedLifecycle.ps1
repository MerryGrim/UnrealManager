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
$report=[ordered]@{schema=1;passed=$false;startedUtc=[DateTime]::UtcNow.ToString('o');host=$hostScope;sourceCommit=$env:GITHUB_SHA;candidateSha256=$info.appSha256;version=$info.version;tests=@();runtimeBefore=@(Runtime-Registration);runtimeFilesBefore=@(Known-RuntimeFiles);missingRuntimeBootstrap='notRun';actualUiReadiness=@();rendering=@();cleanup=@();remainingOwned=@();error=$null;limits=@('Hosted Windows Server images contain developer dependencies; not clean Windows 10/11.','The default hosted account may be elevated with UAC disabled; no standard-user/UAC claim.','No genuine expired/fresh Epic login or key activation is automated.','No physical GPU measurement; only observed software-rendering flags.','No updater signing key/manifest/transition is involved in this lifecycle.','No runtime removal, fake registry hiding, firewall changes or offline network simulation. Runtime file absence is limited to canonical Evergreen install locations.')}
$pins=New-Object System.Collections.Generic.List[object];$installed=Join-Path $out 'fixtures/Installed';$portable=Join-Path $out 'fixtures/Portable';$sentinels=@{};$uninstallerHashes=@{};$profileOwned=$false
$installerPins=New-Object System.Collections.Generic.List[object];$script:installerTrackingUncertain=$false;$script:appTrackingUncertain=$false;$script:lifecycleStage='before-first-setup'
function Observe-Installer($Pin){
    foreach($row in @(Owned-Tree $Pin)){
        if(@($installerPins|Where-Object{$_.pid -eq $row.pid -and $_.created -eq $row.created}).Count){continue}
        $child=Get-Process -Id $row.pid -ErrorAction SilentlyContinue
        if($child){try{$identity=Process-Identity $child;if(-not $identity){if($child.HasExited){continue};throw 'Installer descendant live image cannot be confirmed.'};if($identity.created -ne $row.created){throw 'Installer descendant identity changed.'};$installerPins.Add($identity)}catch{$script:installerTrackingUncertain=$true;throw}}
    }
}
function Wait-Installer($Process,[string]$Executable,[int]$TimeoutMilliseconds){
    # A timeout must retain profile/fixture data while the real installer can
    # still use it. Track the bootstrap process and observed descendants; never
    # force-kill Setup or an uninstaller to make cleanup appear successful.
    $script:installerTrackingUncertain=$true
    $pin=Pin-Process $Process $Executable;$installerPins.Add($pin)
    $script:installerTrackingUncertain=$false;$deadline=[DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    do{
        foreach($known in @($installerPins.ToArray())){if(Find-Pinned $known){Observe-Installer $known}}
        if($Process.WaitForExit(500)){$Process.Refresh();return}
    }while([DateTime]::UtcNow -lt $deadline)
    throw 'Owned installer timeout; no force-kill attempted. Fixture data will be retained until quiescent.'
}
function Assert-InstallerQuiescent{
    if($script:installerTrackingUncertain){throw 'Installer identity could not be confirmed; no data cleanup.'}
    $left=@($installerPins|Where-Object{Find-Pinned $_})
    if($left.Count){$report.remainingOwned+=@($left|ForEach-Object{@{pid=$_.pid;created=$_.created;kind='installer';path=$_.path}});throw 'Owned installer identities remain; no data cleanup.'}
}
function Step([string]$Name,[bool]$Passed,$Details){$report.tests+=@{name=$Name;passed=$Passed;details=$Details};if(-not $Passed){throw ('Lifecycle assertion failed: '+$Name)}}
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
    Wait-Installer $p $setup 600000
    Assert-InstallerQuiescent
    Step $Name ($p.ExitCode -eq 0 -and (Test-Hash (Join-Path $installed 'unreal-manager.exe')) -ceq $info.appSha256 -and -not(Test-Path -LiteralPath (Join-Path $installed 'portable.marker'))) @{exitCode=$p.ExitCode;runtimeAfter=@(Runtime-Registration)}
    foreach($fileName in @('unins000.exe','unins000.dat')){$uninstallerHashes[$fileName]=Test-Hash (Join-Path $installed $fileName)}
    Preserved
}
function Invoke-Uninstall([string]$Name){
    $script:lifecycleStage=$Name+'-uninstall'
    $records=@(App-Registration);if(-not $records.Count){throw 'Owned app uninstall registration missing.'}
    foreach($record in $records){if(-not $record.installLocation -or -not(Same-Path $record.installLocation $installed)){throw 'Uninstall registry location is not this owned case.'}}
    foreach($fileName in @('unins000.exe','unins000.dat')){if((Test-Hash (Join-Path $installed $fileName)) -cne $uninstallerHashes[$fileName]){throw 'Owned uninstaller EXE/DAT changed.'}}
    $p=Start-Process -FilePath (Join-Path $installed 'unins000.exe') -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG="'+(Join-Path $out ('logs/'+$Name+'.log'))+'"')) -WindowStyle Hidden -PassThru
    Wait-Installer $p (Join-Path $installed 'unins000.exe') 180000
    Assert-InstallerQuiescent
    Preserved;Step $Name ($p.ExitCode -eq 0 -and -not(Test-Path -LiteralPath (Join-Path $installed 'unreal-manager.exe')) -and @(App-Registration).Count -eq 0) @{exitCode=$p.ExitCode;dataAndSyntheticSentinelsPreserved=$true}
}
function Close-Own($Pin){
    $tree=@(Owned-Tree $Pin);$p=Find-Pinned $Pin
    if($p){if(-not $p.CloseMainWindow() -or -not $p.WaitForExit(15000)){throw 'Owned app did not close normally; no force-kill attempted.'}}
    $deadline=[DateTime]::UtcNow.AddSeconds(10)
    do{$left=@();foreach($row in $tree){$child=Get-Process -Id $row.pid -ErrorAction SilentlyContinue;if($child -and $child.StartTime.ToUniversalTime().ToFileTimeUtc() -eq $row.created){$left+=$row}};if($left.Count){Start-Sleep -Milliseconds 200}}while($left.Count -and [DateTime]::UtcNow -lt $deadline)
    if($left.Count){$report.remainingOwned=$left;throw 'Observed owned descendant remains; fixture data retained.'}
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
    $script:appTrackingUncertain=$false;$script:lifecycleStage=$Name+'-actual-ui-readiness'
    $ui=Join-Path $out ('reports/'+$Name+'-ui.json');$ps=Join-Path $env:WINDIR 'System32/WindowsPowerShell/v1.0/powershell.exe'
    $probe=Start-Process -FilePath $ps -ArgumentList @('-NoProfile','-STA','-ExecutionPolicy','Bypass','-File',('"'+(Join-Path $PSScriptRoot 'Get-HostedUiReadiness.ps1')+'"'),'-ProcessId',$pin.pid,'-CreatedFileTime',$pin.created,'-Executable',('"'+$exe+'"'),'-ExpectedVersion',$info.version,'-OutputPath',('"'+$ui+'"')) -WindowStyle Hidden -PassThru
    if(-not $probe.WaitForExit(35000)){ $probe.Kill();[void]$probe.WaitForExit(5000);throw 'Own UIA test probe timed out; app was not force-killed.' }
    if(-not(Test-Path -LiteralPath $ui)){throw 'UIA probe produced no readiness report.'}
    $uiResult=Get-Content -LiteralPath $ui -Raw|ConvertFrom-Json;$report.actualUiReadiness+=@{mode=$Name;ready=$uiResult.ready;method=$uiResult.method}
    $tree=@(Owned-Tree $pin);$browsers=@($tree|Where-Object{$_.browserRoot});$unknown=@($tree|Where-Object{$_.name -ieq 'msedgewebview2.exe' -and -not $_.commandLineReadable})
    $flags=$browsers.Count -gt 0 -and -not $unknown.Count -and @($browsers|Where-Object{-not $_.disableGpu -or -not $_.disableCompositing}).Count -eq 0
    $report.rendering+=@{mode=$Name;browserRoots=$browsers.Count;unknownCommandLines=$unknown.Count;bothSoftwareFlags=$flags;scope='Configuration only; no physical GPU claim.'}
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
        if($report.remainingOwned.Count){throw 'Owned descendant identities remain; no data cleanup.'}
        Assert-InstallerQuiescent
        if((Test-Path -LiteralPath (Join-Path $installed 'unins000.exe')) -and $uninstallerHashes.Count -eq 2){Invoke-Uninstall 'cleanup-uninstall'}
        Assert-InstallerQuiescent
        if(@(App-Registration).Count){throw 'App uninstall registration remains; profile retained.'}
        if($profileOwned){$report.cleanup+=@{target='own blank installed profile';result=(Remove-OwnedTree $profileParent (Join-Path $env:LOCALAPPDATA 'UnrealManager') $id)}}
        if(Test-Path -LiteralPath (Join-Path $portable '.distribution-owner')){$report.cleanup+=@{target='own portable fixture';result=(Remove-OwnedTree $portable (Join-Path $out 'fixtures/Portable') $id)}}
    }catch{$report.passed=$false;$report.cleanup+=@{failed=$true;error=$_.Exception.Message}}
    $report.finishedUtc=[DateTime]::UtcNow.ToString('o');New-Json (Join-Path $out 'reports/lifecycle-report.json') $report
}
if(-not $report.passed){throw ('Hosted lifecycle failed: '+$report.error)}
