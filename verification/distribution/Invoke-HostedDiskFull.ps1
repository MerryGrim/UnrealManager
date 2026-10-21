#requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$TestExecutable,
    [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$TestExecutableSha256,
    [Parameter(Mandatory)][string]$OutputDirectory
)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'HostedLifecycle.Common.ps1')
$hostScope=Assert-HostedWindowsServer
if(-not $hostScope.elevated){throw 'A disposable elevated hosted Server token is required for this isolated VHD test.'}
$out=Hosted-Output $OutputDirectory
$workspace=[IO.Path]::GetFullPath($env:GITHUB_WORKSPACE)
$exe=[IO.Path]::GetFullPath($TestExecutable);Assert-Plain $exe
if(-not $exe.StartsWith($workspace+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($exe) -cnotmatch '^unreal_manager_lib-[a-f0-9]+\.exe$' -or (Test-Hash $exe) -cne $TestExecutableSha256.ToLowerInvariant() -or $env:GITHUB_SHA -notmatch '^[a-f0-9]{40}$'){throw 'The exact current-run library test executable and source commit are required.'}
[void][IO.Directory]::CreateDirectory($out)
foreach($name in @('reports','logs')){[void][IO.Directory]::CreateDirectory((Join-Path $out $name))}
$id=[guid]::NewGuid().ToString();$vhd=Join-Path $out ('fixture-'+$id+'.vhd')
$caseName='Case-'+[guid]::NewGuid().ToString();$fixedBytes=67108864L
$testName='engine::install::tests::real_enospc_atomic_preserves_record_and_retries'
$nativeReport=Join-Path $out 'reports/native-enospc.json'
$report=[ordered]@{schema=1;passed=$false;status='notRun';startedUtc=[DateTime]::UtcNow.ToString('o');host=$hostScope;sourceCommit=$env:GITHUB_SHA;test=@{name=$testName;path=$exe;sha256=$TestExecutableSha256.ToLowerInvariant()};vhd=@{path=$vhd;fixedCapacityBytes=$fixedBytes;diskNumber=$null;driveLetter=$null};native=$null;cleanup=@();error=$null;limits=@('Only a newly created 64 MiB virtual disk on a disposable hosted Windows Server is used. No physical disk is selected.','This proves actual NTFS disk-full handling, not clean Windows 10/11, physical storage failure or Epic transfer acceptance.','The ignored native case alone owns bounded filler bytes. No shared workspace volume is filled.');finishedUtc=$null}
$diskNumber=$null;$diskUniqueId=$null;$diskPartPin=$null;$testPin=$null;$script:processIdentityUncertain=$false;$caseRoot=$null
$oldRoot=[Environment]::GetEnvironmentVariable('UM_REAL_ENOSPC_ROOT','Process')
$oldReport=[Environment]::GetEnvironmentVariable('UM_REAL_ENOSPC_REPORT','Process')
function Owned-Disk {
    Assert-Plain $vhd
    if(-not(Test-Path -LiteralPath $vhd -PathType Leaf)){throw 'Owned VHD is missing.'}
    $images=@(Get-DiskImage -ImagePath $vhd -ErrorAction Stop)
    if($images.Count -ne 1 -or -not $images[0].Attached -or -not(Same-Path $images[0].ImagePath $vhd)){throw 'Exact owned VHD is not attached.'}
    $disks=@($images[0]|Get-Disk -ErrorAction Stop)
    if($disks.Count -ne 1){throw 'Owned VHD did not resolve to exactly one disk.'}
    $disk=$disks[0]
    if([long]$disk.Size -ne $fixedBytes -or $disk.IsBoot -or $disk.IsSystem -or $disk.IsOffline -or $disk.IsReadOnly){throw 'Unsafe VHD disk identity or state.'}
    if($null -ne $script:diskNumber -and ([int]$disk.Number -ne $script:diskNumber -or ($null -ne $script:diskUniqueId -and [string]$disk.UniqueId -cne $script:diskUniqueId))){throw 'Owned VHD disk identity changed.'}
    return $disk
}
function Require-Quiescent {
    if($script:processIdentityUncertain){throw 'Fixture process identity could not be pinned; VHD retained without detach/delete.'}
    foreach($pin in @($diskPartPin,$testPin)){if($pin -and (Find-Pinned $pin)){throw 'Owned fixture process remains alive; VHD retained without detach/delete.'}}
}
try{
    foreach($name in @('Get-DiskImage','Get-Disk','Get-Partition','Get-Volume','Initialize-Disk','New-Partition','Format-Volume','Dismount-DiskImage')){if(-not(Get-Command $name -ErrorAction SilentlyContinue)){throw ('Required supported Storage API unavailable: '+$name)}}
    $diskpart=Join-Path $env:WINDIR 'System32/diskpart.exe';Assert-Plain $diskpart
    if(-not(Test-Path -LiteralPath $diskpart -PathType Leaf)){throw 'Native diskpart is unavailable; no alternative disk mutation attempted.'}
    if(Test-Path -LiteralPath $vhd){throw 'VHD path already exists.'}
    if($vhd.IndexOfAny([char[]]@('"',"`r","`n")) -ge 0){throw 'Unsafe VHD path.'}
    $commands=Join-Path $out 'logs/create-owned-vhd.txt'
    [IO.File]::WriteAllText($commands,('create vdisk file="'+$vhd+'" maximum=64 type=fixed'+"`r`n"+'attach vdisk'+"`r`n"+'exit'+"`r`n"))
    $report.status='running'
    $script:processIdentityUncertain=$true
    $p=Start-Process -FilePath $diskpart -ArgumentList @('/s',('"'+$commands+'"')) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $out 'logs/diskpart.log') -RedirectStandardError (Join-Path $out 'logs/diskpart-error.log') -PassThru
    $diskPartPin=Pin-Process $p $diskpart;$script:processIdentityUncertain=$false
    if(-not $p.WaitForExit(120000)){throw 'Owned diskpart timeout; no force-kill or detach attempted.'}
    $p.Refresh();if($p.ExitCode -ne 0){throw 'Native creation/attachment of the owned VHD failed.'}
    $disk=Owned-Disk;$diskNumber=[int]$disk.Number
    $report.vhd.diskNumber=$diskNumber
    if([string]$disk.PartitionStyle -cne 'RAW' -or @($disk|Get-Partition -ErrorAction SilentlyContinue).Count){throw 'New VHD is not an empty RAW disk.'}
    # Every mutation receives a disk resolved again from this exact image.
    $disk=Owned-Disk;$initialized=$disk|Initialize-Disk -PartitionStyle MBR -PassThru -ErrorAction Stop
    if([int]$initialized.Number -ne $diskNumber){throw 'Initialization returned another disk.'}
    # MBR initialization assigns the disk signature. Pin it only afterwards;
    # image path, exact capacity and disk number are checked on both sides.
    $diskUniqueId=[string]$initialized.UniqueId
    $letter=$null
    foreach($candidate in [char[]]'RSTUVWXYZ'){
        $volumes=@(Get-Volume -DriveLetter $candidate -ErrorAction SilentlyContinue)
        $partitions=@(Get-Partition -DriveLetter $candidate -ErrorAction SilentlyContinue)
        $drives=@(Get-PSDrive -Name ([string]$candidate) -ErrorAction SilentlyContinue)
        if(-not $volumes.Count -and -not $partitions.Count -and -not $drives.Count -and -not(Test-Path -LiteralPath ($candidate+':\'))){$letter=[string]$candidate;break}
    }
    if(-not $letter){throw 'No unused R-Z drive letter; existing mounts were not changed.'}
    $disk=Owned-Disk
    $partition=$disk|New-Partition -UseMaximumSize -DriveLetter $letter -ErrorAction Stop
    $disk=Owned-Disk
    $actual=@($disk|Get-Partition -ErrorAction Stop|Where-Object{$_.PartitionNumber -eq $partition.PartitionNumber})
    if($actual.Count -ne 1 -or [int]$actual[0].DiskNumber -ne $diskNumber -or [string]$actual[0].DriveLetter -cne $letter -or $actual[0].IsBoot -or $actual[0].IsSystem){throw 'Owned VHD partition identity changed before format.'}
    $volume=$actual[0]|Format-Volume -FileSystem NTFS -NewFileSystemLabel ('UM-'+$id.Substring(0,8)) -Force -Confirm:$false -ErrorAction Stop
    $disk=Owned-Disk
    $volumes=@($disk|Get-Partition -ErrorAction Stop|Where-Object{$_.PartitionNumber -eq $partition.PartitionNumber}|Get-Volume -ErrorAction Stop)
    if($volumes.Count -ne 1 -or [string]$volumes[0].DriveLetter -cne $letter -or [string]$volumes[0].FileSystemType -cne 'NTFS' -or [long]$volumes[0].Size -gt $fixedBytes -or [long]$volumes[0].Size -le 8MB){throw 'Owned NTFS volume bounds were not confirmed.'}
    $report.vhd.driveLetter=$letter;$report.vhd.volumeCapacityBytes=[long]$volumes[0].Size
    $caseRoot=$letter+':\'+$caseName
    if(Test-Path -LiteralPath $caseRoot){throw 'Native case root must be fresh and absent.'}
    $env:UM_REAL_ENOSPC_ROOT=$caseRoot;$env:UM_REAL_ENOSPC_REPORT=$nativeReport
    if((Test-Hash $exe) -cne $TestExecutableSha256.ToLowerInvariant()){throw 'Native test image changed before execution.'}
    $script:processIdentityUncertain=$true
    $p=Start-Process -FilePath $exe -ArgumentList @('--ignored','--exact',$testName,'--nocapture','--test-threads=1') -WorkingDirectory $workspace -WindowStyle Hidden -RedirectStandardOutput (Join-Path $out 'logs/native-enospc.log') -RedirectStandardError (Join-Path $out 'logs/native-enospc-error.log') -PassThru
    $testPin=Pin-Process $p $exe;$script:processIdentityUncertain=$false
    if(-not $p.WaitForExit(180000)){throw 'Owned native test timeout; VHD retained until process quiescence, no force-kill.'}
    $p.Refresh();Require-Quiescent
    if(-not(Test-Path -LiteralPath $nativeReport -PathType Leaf)){throw 'Exact ignored case produced no native report; zero selected tests is not a pass.'}
    if((Get-Item -LiteralPath $nativeReport).Length -gt 65536){throw 'Unexpected native report size.'}
    $native=Get-Content -LiteralPath $nativeReport -Raw|ConvertFrom-Json
    if($p.ExitCode -ne 0 -or $native.schema -ne 1 -or -not $native.passed -or $native.rawOsError -notin @(112,39) -or -not(Same-Path $native.root $caseRoot) -or $native.volumeCapacityBytes -gt $fixedBytes -or -not $native.observations.oldBytesPreserved -or -not $native.observations.oldModificationTimePreserved -or -not $native.observations.failedTemporaryAbsent -or $native.observations.retryBytes -ne 8388608 -or $native.observations.fillerBytes -gt $fixedBytes -or $native.observations.availableBeforeAtomic -ge 8388608 -or $native.cleanupError -or -not $native.fixtureAbsent -or $native.diskCreatedOrMountedByTest -or (Test-Path -LiteralPath $caseRoot)){throw 'Real disk-full native assertions or owned fixture cleanup did not pass.'}
    $report.native=$native;$report.passed=$true;$report.status='passed'
}catch{$report.error=$_.Exception.Message;if($report.status -ne 'notRun'){$report.status='failed'}}
finally{
    [Environment]::SetEnvironmentVariable('UM_REAL_ENOSPC_ROOT',$oldRoot,'Process')
    [Environment]::SetEnvironmentVariable('UM_REAL_ENOSPC_REPORT',$oldReport,'Process')
    try{
        Require-Quiescent
        if(Test-Path -LiteralPath $vhd -PathType Leaf){
            Assert-Plain $vhd
            $images=@(Get-DiskImage -ImagePath $vhd -ErrorAction Stop)
            if($images.Count -ne 1 -or -not(Same-Path $images[0].ImagePath $vhd)){throw 'Cleanup cannot confirm exact image identity; retained.'}
            if($images[0].Attached){[void](Owned-Disk);Dismount-DiskImage -ImagePath $vhd -ErrorAction Stop}
            $images=@(Get-DiskImage -ImagePath $vhd -ErrorAction Stop)
            if($images.Count -ne 1 -or $images[0].Attached -or -not(Same-Path $images[0].ImagePath $vhd)){throw 'Image detach not confirmed; backing file retained.'}
            [IO.File]::Delete($vhd);$report.cleanup+=@{ownedImageDetached=$true;ownBackingFileRemoved=(-not(Test-Path -LiteralPath $vhd))}
        }else{$report.cleanup+=@{ownedImageAbsent=$true}}
    }catch{$report.passed=$false;$report.status='failed';$report.cleanup+=@{failed=$true;error=$_.Exception.Message;backingFileRetained=(Test-Path -LiteralPath $vhd)}}
    $report.finishedUtc=[DateTime]::UtcNow.ToString('o')
    New-Json (Join-Path $out 'reports/disk-full-report.json') $report
}
if(-not $report.passed){throw ('Hosted real disk-full case did not pass: '+$report.error)}
