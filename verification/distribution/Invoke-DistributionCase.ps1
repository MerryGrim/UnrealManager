#requires -Version 5.1
<#
Runs only explicit steps in a disposable Windows guest. VerifyPackages is read-only
apart from its new report directory and is safe on the preparation host.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('VerifyPackages','Initialize','Install','Launch','Snapshot','Close','Uninstall','Reinstall','Observe','Finish')][string]$Action,
    [string]$PackagesDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidateSet('installed','portable')][string]$Mode='installed',
    [ValidateSet('present','missing')][string]$ExpectedWebView='present',
    [ValidateSet('online','offline')][string]$Network='online',
    [switch]$DisposableVm,
    [string]$ForbiddenHostContextPath,
    [switch]$OfflineConfirmed,
    [switch]$ShowWindow,
    [ValidateSet('MainUi','EpicFreshLogin','EpicExpiredReauth','RedemptionAccount','NineLocales','KeyboardAndScale','OfflineError','UninstallReinstall')][string]$Observation,
    [ValidateSet('Pass','Fail','NotRun')][string]$Outcome='NotRun',
    [string]$Note=''
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$utf8=New-Object Text.UTF8Encoding($false)
$runtimeFiles=@('unreal-manager.exe','README.txt','LICENSE','THIRD_PARTY_NOTICES.md','THIRD-PARTY-LICENSES.txt','portable.marker')
$out=[IO.Path]::GetFullPath($OutputDirectory)
if (-not [IO.Path]::IsPathRooted($OutputDirectory) -or $out.StartsWith('\\')) { throw 'Use an absolute local guest workspace directory.' }
function Assert-NoLinks([string]$Path) {
    $part=[IO.Path]::GetFullPath($Path)
    while ($part) {
        if (Test-Path -LiteralPath $part) { $entry=Get-Item -LiteralPath $part -Force; if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Linked paths are not permitted.' } }
        $next=[IO.Path]::GetDirectoryName($part); if ($next -eq $part) { break }; $part=$next
    }
}
function Hash-Bytes([byte[]]$Bytes) { $h=[Security.Cryptography.SHA256]::Create(); try { ([BitConverter]::ToString($h.ComputeHash($Bytes))).Replace('-','').ToLowerInvariant() } finally { $h.Dispose() } }
function Hash-File([string]$Path) { Assert-NoLinks $Path; (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Write-NewJson([string]$Path,$Value) {
    Assert-NoLinks $Path
    $bytes=$utf8.GetBytes(($Value | ConvertTo-Json -Depth 15))
    $stream=[IO.File]::Open($Path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
    try { $stream.Write($bytes,0,$bytes.Length) } finally { $stream.Dispose() }
}
function Receipt([string]$Name,$Value) { $path=Join-Path $out ($Name+'-'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')+'-'+[guid]::NewGuid().ToString('N')+'.json'); Write-NewJson $path $Value; Write-Output $path }
function Read-BoundedJson([string]$Path,[long]$Maximum=262144) { Assert-NoLinks $Path; if ((Get-Item -LiteralPath $Path).Length -gt $Maximum) { throw 'JSON input too large.' }; Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json }
function WebView-Registration {
    $result=@()
    foreach ($hive in @([Microsoft.Win32.RegistryHive]::LocalMachine,[Microsoft.Win32.RegistryHive]::CurrentUser)) {
        foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32,[Microsoft.Win32.RegistryView]::Registry64)) {
            $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($hive,$view)
            try { $key=$base.OpenSubKey('SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'); if ($key) { try { $pv=[string]$key.GetValue('pv'); if ($pv -and $pv -ne '0.0.0.0') { $result+=@{hive=[string]$hive;view=[string]$view;version=$pv} } } finally { $key.Dispose() } } } finally { $base.Dispose() }
        }
    }
    return $result
}
function Uninstall-Registration {
    $result=@()
    foreach ($hive in @([Microsoft.Win32.RegistryHive]::LocalMachine,[Microsoft.Win32.RegistryHive]::CurrentUser)) {
        foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32,[Microsoft.Win32.RegistryView]::Registry64)) {
            $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($hive,$view)
            try { $key=$base.OpenSubKey('SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{61185C58-31CF-4783-BA27-429C3454B9A5}_is1'); if ($key) { try { $result+=@{hive=[string]$hive;view=[string]$view;installLocation=[string]$key.GetValue('InstallLocation');version=[string]$key.GetValue('DisplayVersion')} } finally { $key.Dispose() } } } finally { $base.Dispose() }
        }
    }
    return $result
}
function Assert-Guest {
    if (-not $DisposableVm) { throw 'Explicit -DisposableVm is required for every guest action.' }
    if (-not $ForbiddenHostContextPath -or -not [IO.Path]::IsPathRooted($ForbiddenHostContextPath)) { throw 'Provide the forbidden-host context generated on the preparation host.' }
    $context=Read-BoundedJson $ForbiddenHostContextPath 4096
    if ($context.schema -ne 1 -or $context.hostFingerprint -notmatch '^[0-9a-f]{64}$') { throw 'Invalid forbidden-host context.' }
    $guid=[string](Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Cryptography').MachineGuid
    if ((Hash-Bytes ($utf8.GetBytes($guid))) -eq $context.hostFingerprint) { throw 'Execution on the preparation host is forbidden.' }
    $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
    $principal=New-Object Security.Principal.WindowsPrincipal($identity)
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run the per-user case in a non-elevated guest PowerShell.' }
    $system=Get-CimInstance Win32_ComputerSystem
    if (($system.Manufacturer+' '+$system.Model) -notmatch '(?i)virtual|vmware|virtualbox|qemu|kvm|xen|parallels') { throw 'A recognized disposable virtual guest is required; HypervisorPresent alone is insufficient.' }
    $os=Get-CimInstance Win32_OperatingSystem
    if ([int]$os.BuildNumber -lt 10240 -or [int]$os.ProductType -ne 1 -or -not [Environment]::Is64BitOperatingSystem) { throw 'Windows 10/11 x64 desktop guest required.' }
    return @{manufacturer=$system.Manufacturer;model=$system.Model;caption=$os.Caption;build=$os.BuildNumber;machineFingerprint=(Hash-Bytes ($utf8.GetBytes($guid)));elevated=$false;administratorsGroupPresent=(@($identity.Groups | ForEach-Object {$_.Value}) -contains 'S-1-5-32-544')}
}
function Verify-Packages([string]$Directory) {
    Assert-NoLinks $Directory
    $root=[IO.Path]::GetFullPath($Directory)
    $manifestPath=Join-Path $root 'update-manifest.json'; $signaturePath=Join-Path $root 'update-manifest.sig'
    Assert-NoLinks $manifestPath; Assert-NoLinks $signaturePath
    if ((Get-Item -LiteralPath $manifestPath).Length -gt 65536) { throw 'Manifest too large.' }
    $raw=[IO.File]::ReadAllBytes($manifestPath)
    if ((Get-Item -LiteralPath $signaturePath).Length -gt 256) { throw 'Signature too large.' }
    $signature=[Convert]::FromBase64String(([IO.File]::ReadAllText($signaturePath)).Trim())
    $public=[Convert]::FromBase64String('RUNTMSAAAABirRciqs2aCkzH/wDyWTjc62RynUFl/Kpm9QkPkKfIgS6/vrS2FSZgsYDl2HyTAhaBSWBSRZ0GJ5qQgn3FTi24')
    $key=[Security.Cryptography.CngKey]::Import($public,[Security.Cryptography.CngKeyBlobFormat]::EccPublicBlob)
    $ecdsa=New-Object Security.Cryptography.ECDsaCng($key)
    try { if ($signature.Length -ne 64 -or -not $ecdsa.VerifyData($raw,$signature,[Security.Cryptography.HashAlgorithmName]::SHA256)) { throw 'Pinned ECDSA signature verification failed.' } } finally { $ecdsa.Dispose(); $key.Dispose() }
    $manifest=$utf8.GetString($raw) | ConvertFrom-Json
    if ($manifest.schema -ne 1 -or $manifest.target -ne 'windows-x64' -or $manifest.version -notmatch '^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?$' -or $manifest.appSha256 -notmatch '^[0-9a-f]{64}$') { throw 'Invalid signed manifest.' }
    if ($manifest.installed.name -cne "UnrealManager-$($manifest.version)-Windows-x64-Setup.exe" -or $manifest.portable.name -cne "UnrealManager-$($manifest.version)-Windows-x64-Portable.umupdate") { throw 'Noncanonical signed payload name.' }
    Assert-NoLinks (Join-Path $root 'SHA256SUMS.txt')
    $checksums=@{}
    foreach ($line in [IO.File]::ReadAllLines((Join-Path $root 'SHA256SUMS.txt'))) { if ($line -notmatch '^([0-9a-f]{64})  ([A-Za-z0-9._-]+)$') { throw 'Invalid checksum entry.' }; if ($checksums.ContainsKey($Matches[2])) { throw 'Duplicate checksum entry.' }; $checksums[$Matches[2]]=$Matches[1] }
    $required=@('update-manifest.json','update-manifest.sig',"UnrealManager-$($manifest.version)-Windows-x64-Setup.exe","UnrealManager-$($manifest.version)-Windows-x64-Portable.zip",$manifest.portable.name)
    $hashes=@{}
    foreach ($name in $required) { if ($name -notmatch '^[A-Za-z0-9._-]+$') { throw 'Unsafe package name.' }; $path=Join-Path $root $name; if ((Get-Item -LiteralPath $path).Length -gt 64MB) { throw 'Package exceeds bounded size.' }; $hash=Hash-File $path; if (-not $checksums.ContainsKey($name) -or $checksums[$name] -cne $hash) { throw "Checksum mismatch: $name" }; $hashes[$name]=$hash }
    foreach ($payload in @($manifest.installed,$manifest.portable)) { if ($payload.name -notin $required -or $payload.url -cne "https://github.com/MerryGrim/UnrealManager/releases/download/v$($manifest.version)/$($payload.name)" -or $payload.sha256 -cne $hashes[$payload.name] -or $payload.size -ne (Get-Item -LiteralPath (Join-Path $root $payload.name)).Length) { throw 'Signed payload does not match local packages.' } }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip=[IO.Compression.ZipFile]::OpenRead((Join-Path $root "UnrealManager-$($manifest.version)-Windows-x64-Portable.zip"))
    $plain=@{}; $total=0L
    try {
        if ($zip.Entries.Count -ne 6) { throw 'Portable ZIP must contain exactly six fixed files.' }
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -cnotmatch '^UnrealManager-Portable/([^/\\]+)$') { throw 'Unexpected ZIP path.' }
            $name=$Matches[1]
            if ($name -cnotin $runtimeFiles -or $plain.ContainsKey($name) -or $entry.Length -le 0 -or $entry.Length -gt 64MB) { throw 'Unexpected ZIP entry.' }
            $total+=$entry.Length; if ($total -gt 96MB) { throw 'Unpacked archive too large.' }
            $stream=$entry.Open(); $sha=[Security.Cryptography.SHA256]::Create()
            try { $plain[$name]=([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-','').ToLowerInvariant() } finally { $sha.Dispose(); $stream.Dispose() }
        }
    } finally { $zip.Dispose() }
    if ($plain['unreal-manager.exe'] -cne $manifest.appSha256) { throw 'ZIP application differs from signed manifest.' }
    return @{version=$manifest.version;appSha256=$manifest.appSha256;root=$root;files=$required;hashes=$hashes;plainHashes=$plain;signatureVerified=$true;zipFixedSixVerified=$true}
}
function Save-State($State) { $statePath=Join-Path $out 'session.json'; Assert-NoLinks $statePath; [IO.File]::WriteAllText($statePath,($State | ConvertTo-Json -Depth 15),$utf8) }
function Get-State { $state=Read-BoundedJson (Join-Path $out 'session.json'); if ($state.outputDirectory -cne $out -or $state.schema -ne 1) { throw 'Session identity mismatch.' }; return $state }
function Assert-NoAppProcesses { if (@(Get-CimInstance Win32_Process -Filter "Name='unreal-manager.exe'").Count) { throw 'Close all Unreal Manager instances in this disposable guest first; no process will be killed.' } }
function Sentinel-Status($State) { foreach ($s in $State.sentinels) { if (-not (Test-Path -LiteralPath $s.path) -or (Hash-File $s.path) -cne $s.sha256) { throw 'Synthetic preservation sentinel changed.' } }; return $true }
function Assert-UninstallerScope($State,[object[]]$Registrations=@()) {
    Assert-NoLinks $State.application
    $uninstaller=Join-Path $State.application 'unins000.exe'; $data=Join-Path $State.application 'unins000.dat'
    if (-not $State.uninstallerSha256 -or (Hash-File $uninstaller) -cne $State.uninstallerSha256) { throw 'Owned uninstaller identity changed.' }
    if (-not $State.uninstallerDatSha256 -or (Hash-File $data) -cne $State.uninstallerDatSha256) { throw 'Owned uninstaller data changed.' }
    if ($null -eq $Registrations -or $Registrations.Count -eq 0) { throw 'Owned uninstall registration is missing.' }
    $separators=[char[]]@([char]92,[char]47)
    $expected=[IO.Path]::GetFullPath($State.application).TrimEnd($separators)
    foreach ($record in $Registrations) {
        if ([string]::IsNullOrWhiteSpace([string]$record.installLocation) -or -not [IO.Path]::IsPathRooted([string]$record.installLocation)) { throw 'Unknown uninstall registration location.' }
        $actual=[IO.Path]::GetFullPath([string]$record.installLocation).TrimEnd($separators)
        if (-not $actual.Equals($expected,[StringComparison]::OrdinalIgnoreCase)) { throw 'Uninstall registration belongs to another location.' }
    }
}
function Identity($Process) { $Process.Refresh(); return @{pid=$Process.Id;createdFileTime=$Process.StartTime.ToUniversalTime().ToFileTimeUtc();path=$Process.Path;sha256=(Hash-File $Process.Path)} }
function Current-Own($State) {
    $list=@(); foreach ($pin in $State.launches) {
        $p=Get-Process -Id $pin.pid -ErrorAction SilentlyContinue
        if ($p) { try { if ($p.StartTime.ToUniversalTime().ToFileTimeUtc() -eq $pin.createdFileTime -and $p.Path -ieq $pin.path) { $list+=$p } } catch { throw 'Cannot confirm owned process identity.' } }
    }; return $list
}
function Runtime-Snapshot($State) {
    $all=@(Get-CimInstance Win32_Process); $owned=@{}
    foreach ($p in (Current-Own $State)) { $owned[[int]$p.Id]=$p.StartTime.ToUniversalTime().ToFileTimeUtc() }
    foreach ($pin in $State.observedProcesses) { $p=Get-Process -Id $pin.pid -ErrorAction SilentlyContinue; if ($p -and $p.StartTime.ToUniversalTime().ToFileTimeUtc() -eq $pin.createdFileTime) { $owned[[int]$pin.pid]=$pin.createdFileTime } }
    $rows=@(); $changed=$true
    while ($changed) { $changed=$false; foreach ($p in $all) { if (-not $owned.ContainsKey([int]$p.ProcessId) -and $owned.ContainsKey([int]$p.ParentProcessId) -and $p.CreationDate.ToUniversalTime().ToFileTimeUtc() -ge $owned[[int]$p.ParentProcessId]) { $child=Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue; if ($child) { $created=$child.StartTime.ToUniversalTime().ToFileTimeUtc(); if ([Math]::Abs($created-$p.CreationDate.ToUniversalTime().ToFileTimeUtc()) -ge 10) { throw 'Child process identity changed during observation.' }; $owned[[int]$p.ProcessId]=$created; $changed=$true } } } }
    foreach ($p in $all) { if ($owned.ContainsKey([int]$p.ProcessId)) {
        $isWebView=$p.Name -ieq 'msedgewebview2.exe'; $readable=$isWebView -and -not [string]::IsNullOrWhiteSpace([string]$p.CommandLine)
        $gpu=if($readable){$p.CommandLine -match '(?:^|\s)--disable-gpu(?:\s|$)'}else{$null}
        $compositing=if($readable){$p.CommandLine -match '(?:^|\s)--disable-gpu-compositing(?:\s|$)'}else{$null}
        $rows+=@{pid=$p.ProcessId;createdFileTime=$owned[[int]$p.ProcessId];parentPid=$p.ParentProcessId;name=$p.Name;commandLineReadable=$readable;browserRoot=($readable -and $p.CommandLine -notmatch '(?:^|\s)--type=');disableGpu=$gpu;disableCompositing=$compositing;softwareFlagStatus=$(if(-not $isWebView){'notWebView'}elseif(-not $readable){'unknown'}elseif($gpu -and $compositing){'softwareFlagsPresent'}else{'incomplete'})}
    } }
    return @{processes=$rows;webViewRegistry=@(WebView-Registration);uninstallRegistry=@(Uninstall-Registration);sentinelsPreserved=(Sentinel-Status $State);renderingScope='Observed command-line flags only; no physical GPU activity measurement.'}
}
Assert-NoLinks $out
if ($Action -eq 'VerifyPackages') {
    if (Test-Path -LiteralPath $out) { throw 'Choose a fresh output directory.' }
    [void][IO.Directory]::CreateDirectory($out)
    $verified=Verify-Packages $PackagesDirectory
    Receipt 'package-preflight' @{schema=1;passed=$true;checks=$verified;noRuntimeExecution=$true;cleanOsAcceptance=$false}
    return
}
$guest=Assert-Guest
if ($Action -eq 'Initialize') {
    if (Test-Path -LiteralPath $out) { throw 'Choose a fresh output directory.' }
    Assert-NoAppProcesses
    if (@(Uninstall-Registration).Count) { throw 'Unreal Manager uninstall registration already exists; revert to a fresh guest snapshot.' }
    $installedData=Join-Path $env:LOCALAPPDATA 'UnrealManager\Data'
    if (Test-Path -LiteralPath (Split-Path $installedData -Parent)) { throw 'Existing app profile detected. Use a fresh guest user/snapshot; nothing was deleted.' }
    $runtime=@(WebView-Registration)
    if (($ExpectedWebView -eq 'missing' -and $runtime.Count) -or ($ExpectedWebView -eq 'present' -and -not $runtime.Count)) { throw 'Guest WebView2 registration does not match the requested scenario.' }
    if ($Network -eq 'offline' -and -not $OfflineConfirmed) { throw 'Disconnect the guest NIC in the hypervisor first and provide -OfflineConfirmed; this script never changes network adapters.' }
    $verified=Verify-Packages $PackagesDirectory
    [void][IO.Directory]::CreateDirectory($out); [void][IO.Directory]::CreateDirectory((Join-Path $out 'packages'))
    foreach ($name in @($verified.files)+@('SHA256SUMS.txt')) { [IO.File]::Copy((Join-Path $verified.root $name),(Join-Path $out ('packages\'+$name)),$false) }
    $copied=Verify-Packages (Join-Path $out 'packages')
    if ($copied.appSha256 -cne $verified.appSha256) { throw 'Copied package identity changed.' }
    $app=Join-Path $out $(if ($Mode -eq 'portable') {'Portable'} else {'Installed'})
    [void][IO.Directory]::CreateDirectory($app)
    if ($Mode -eq 'portable') {
        $zip=[IO.Compression.ZipFile]::OpenRead((Join-Path $out "packages\UnrealManager-$($verified.version)-Windows-x64-Portable.zip"))
        try { foreach ($entry in $zip.Entries) { $name=$entry.FullName.Substring('UnrealManager-Portable/'.Length); [IO.Compression.ZipFileExtensions]::ExtractToFile($entry,(Join-Path $app $name),$false) } } finally { $zip.Dispose() }
    }
    $data=if ($Mode -eq 'portable') {Join-Path $app 'Data'} else {$installedData}
    Assert-NoLinks $app; Assert-NoLinks $data
    [void][IO.Directory]::CreateDirectory((Join-Path $data 'Settings')); [void][IO.Directory]::CreateDirectory((Join-Path $data 'Account')); [void][IO.Directory]::CreateDirectory((Join-Path $data 'Tasks')); [void][IO.Directory]::CreateDirectory((Join-Path $out 'Temp'))
    [IO.File]::WriteAllText((Join-Path $data 'Settings\window.json'),'"exit"',$utf8)
    [IO.File]::WriteAllText((Join-Path $data 'Settings\app-updates.json'),'{"enabled":false,"channel":"stable","lastAttempt":0}',$utf8)
    $id=[guid]::NewGuid().ToString(); $sentinels=@()
    foreach ($relative in @('Account\session.fixture','Tasks\task.fixture','user-file.fixture')) { $path=Join-Path $data $relative; [IO.File]::WriteAllText($path,('SYNTHETIC DISTRIBUTION FIXTURE '+$id+' '+$relative),$utf8); $sentinels+=@{path=$path;sha256=(Hash-File $path)} }
    $state=@{schema=1;id=$id;createdUtc=[DateTime]::UtcNow.ToString('o');outputDirectory=$out;guest=$guest;mode=$Mode;expectedWebView=$ExpectedWebView;network=$Network;offlineOperatorConfirmed=[bool]$OfflineConfirmed;package=$verified;application=$app;data=$data;sentinels=$sentinels;launches=@();observedProcesses=@();installed=$false;uninstalled=$false;uninstallerSha256='';uninstallerDatSha256='';observations=@()}
    Save-State $state
    Receipt 'initialize' @{schema=1;passed=$true;id=$id;guest=$guest;mode=$Mode;webViewBefore=$runtime;networkCondition='Operator assertion, not independently measured connectivity';signatureVerified=$true;syntheticOnly=$true}
    return
}
Assert-NoLinks (Join-Path $out 'case.lock')
$caseLock=[IO.File]::Open((Join-Path $out 'case.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
try {
$state=Get-State
if ($state.guest.machineFingerprint -cne $guest.machineFingerprint) { throw 'Session belongs to another guest.' }
if ($state.mode -notin @('installed','portable')) { throw 'Invalid session mode.' }
$expectedApp=Join-Path $out $(if($state.mode -eq 'portable'){'Portable'}else{'Installed'})
$expectedData=if($state.mode -eq 'portable'){Join-Path $expectedApp 'Data'}else{Join-Path $env:LOCALAPPDATA 'UnrealManager\Data'}
if ($state.application -cne $expectedApp -or $state.data -cne $expectedData) { throw 'Session paths do not match their fixed case scope.' }
Assert-NoLinks $state.application; Assert-NoLinks $state.data
$env:TEMP=Join-Path $out 'Temp'; $env:TMP=$env:TEMP
$exe=Join-Path $state.application 'unreal-manager.exe'
if ($Action -in @('Install','Reinstall')) {
    if ($state.mode -ne 'installed') { throw 'Install actions apply only to installed cases.' }
    if ($Action -eq 'Reinstall' -and -not $state.uninstalled) { throw 'Uninstall this case successfully before reinstalling.' }
    if ($Action -eq 'Install' -and $state.installed) { throw 'This case is already installed.' }
    Assert-NoAppProcesses; [void](Sentinel-Status $state)
    $setup=Join-Path $out "packages\UnrealManager-$($state.package.version)-Windows-x64-Setup.exe"
    if ((Hash-File $setup) -cne $state.package.hashes.PSObject.Properties[[IO.Path]::GetFileName($setup)].Value) { throw 'Setup bytes changed.' }
    $log=Join-Path $out ('setup-'+[guid]::NewGuid().ToString('N')+'.log')
    $p=Start-Process -FilePath $setup -ArgumentList @('/SP-','/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/LANG=en',('/DIR="'+$state.application+'"'),('/LOG="'+$log+'"')) -WindowStyle Hidden -PassThru
    $pin=Identity $p
    if (-not $p.WaitForExit(600000)) { Receipt 'install-timeout' @{passed=$false;ownedSetup=$pin;forceKillAttempted=$false;action='Retain evidence and reset only the disposable VM snapshot.'}; throw 'Setup timeout: installer was not killed.' }
    $p.Refresh(); $expectFailure=$state.expectedWebView -eq 'missing' -and $state.network -eq 'offline'
    $applicationPresent=Test-Path -LiteralPath $exe
    $pass=if ($expectFailure) { $p.ExitCode -ne 0 -and -not $applicationPresent -and @(WebView-Registration).Count -eq 0 } else { $p.ExitCode -eq 0 -and $applicationPresent -and (Hash-File $exe) -ceq $state.package.appSha256 -and -not (Test-Path -LiteralPath (Join-Path $state.application 'portable.marker')) }
    [void](Sentinel-Status $state)
    if (-not $expectFailure -and $pass) { $state.installed=$true; $state.uninstalled=$false; $state.uninstallerSha256=Hash-File (Join-Path $state.application 'unins000.exe'); $state.uninstallerDatSha256=Hash-File (Join-Path $state.application 'unins000.dat'); Save-State $state }
    Receipt $Action.ToLowerInvariant() @{passed=$pass;expectedMissingRuntimeFailure=$expectFailure;exitCode=$p.ExitCode;applicationPresent=$applicationPresent;log=$log;runtime=@(WebView-Registration);sentinelsPreserved=$true;scope='Actual Setup execution in the gated disposable guest only.'}
    if (-not $pass) { throw 'Installer outcome did not match this scenario; inspect retained log.' }
    return
}
if ($Action -eq 'Launch') {
    Assert-NoAppProcesses
    if ((Hash-File $exe) -cne $state.package.appSha256) { throw 'Application bytes changed.' }
    $window=if ($ShowWindow) {'Normal'} else {'Hidden'}
    $p=Start-Process -FilePath $exe -WorkingDirectory $state.application -WindowStyle $window -PassThru
    try { $pin=Identity $p } catch { $p.Refresh(); if (-not $p.HasExited) { throw }; Receipt 'launch-exited' @{passed=$false;pid=$p.Id;exitCode=$p.ExitCode;processCreationObserved=$true;meaning='Process exited before a durable identity/UI check; inspect the missing-runtime/error observation. No readiness pass inferred.'}; return }
    $state.launches=@($state.launches)+@($pin); Save-State $state
    Receipt 'launch' @{passed=$true;identity=$pin;windowStyle=$window;meaning='Process creation only. Record MainUi separately after inspecting the actual guest window.'}
    return
}
if ($Action -eq 'Snapshot') { $snapshot=Runtime-Snapshot $state; $state.observedProcesses=@($snapshot.processes); Save-State $state; Receipt 'snapshot' $snapshot; return }
if ($Action -eq 'Close') {
    $snap=Runtime-Snapshot $state
    $requested=@()
    foreach ($p in (Current-Own $state)) { $pin=Identity $p; $requested+=@{identity=$pin;closeMainWindow=$p.CloseMainWindow()}; [void]$p.WaitForExit(20000) }
    $deadline=[DateTime]::UtcNow.AddSeconds(10)
    do {
        $survivors=@()
        foreach ($row in $snap.processes) { $p=Get-Process -Id $row.pid -ErrorAction SilentlyContinue; if ($p -and $p.StartTime.ToUniversalTime().ToFileTimeUtc() -eq $row.createdFileTime) { $survivors+=@{pid=$row.pid;createdFileTime=$row.createdFileTime;name=$row.name} } }
        if ($survivors.Count) { Start-Sleep -Milliseconds 250 }
    } while ($survivors.Count -and [DateTime]::UtcNow -lt $deadline)
    Receipt 'close' @{passed=($survivors.Count -eq 0);requested=$requested;remainingOwned=$survivors;forcedTermination=$false;sentinelsPreserved=(Sentinel-Status $state)}
    if ($survivors.Count) { throw 'Owned process remains. No process was killed; inspect or revert the disposable guest.' }
    return
}
if ($Action -eq 'Uninstall') {
    if ($state.mode -ne 'installed' -or -not $state.installed) { throw 'Install this case before uninstalling.' }
    Assert-NoAppProcesses; [void](Sentinel-Status $state)
    $uninstaller=Join-Path $state.application 'unins000.exe'; Assert-NoLinks $uninstaller
    Assert-UninstallerScope $state -Registrations @(Uninstall-Registration)
    $log=Join-Path $out ('uninstall-'+[guid]::NewGuid().ToString('N')+'.log')
    $p=Start-Process -FilePath $uninstaller -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG="'+$log+'"')) -WindowStyle Hidden -PassThru
    $pin=Identity $p
    if (-not $p.WaitForExit(180000)) { Receipt 'uninstall-timeout' @{passed=$false;ownedUninstaller=$pin;forcedTermination=$false}; throw 'Uninstaller timeout; no process was killed.' }
    $p.Refresh(); $pass=$p.ExitCode -eq 0 -and -not (Test-Path -LiteralPath $exe) -and @(Uninstall-Registration).Count -eq 0
    [void](Sentinel-Status $state); if ($pass) { $state.uninstalled=$true; $state.installed=$false; Save-State $state }
    Receipt 'uninstall' @{passed=$pass;exitCode=$p.ExitCode;log=$log;dataPreserved=$true;syntheticSentinelsPreserved=$true}
    if (-not $pass) { throw 'Uninstall did not meet the case assertions.' }; return
}
if ($Action -eq 'Observe') {
    if (-not $Observation -or $Note.Length -gt 600 -or $Note -match '(?i)bearer\s|access_token|refresh_token|password\s*=|cookie\s*=|authorization\s*:' -or $Note -match '\S+@\S+') { throw 'Supply a bounded redacted observation; no credentials, account names, tokens, or emails.' }
    $entry=@{observation=$Observation;outcome=$Outcome;note=$Note;kind='manual';utc=[DateTime]::UtcNow.ToString('o')}
    $state.observations=@($state.observations)+@($entry); Save-State $state; Receipt 'observation' $entry; return
}
if ($Action -eq 'Finish') {
    $alive=@(Current-Own $state)
    $snapshot=Runtime-Snapshot $state
    Receipt 'finish' @{schema=1;caseId=$state.id;guest=$state.guest;mode=$state.mode;expectedWebView=$state.expectedWebView;network=$state.network;remainingOwnedApplications=$alive.Count;remainingObservedOwnedProcesses=@($snapshot.processes).Count;syntheticSentinelsPreserved=(Sentinel-Status $state);manualObservations=$state.observations;automaticCleanOsPass=$false;cleanup='Export redacted receipts, then revert the disposable VM snapshot. This script never deletes account/profile data or unrelated directories.';limits=@('No installer result is inferred from static package checks.','No fresh/expired Epic session is synthesized or decrypted.','No real key activation or Epic/Fab download is automated.','Software flags are not a measured physical GPU claim.','Review step receipts and manual observations before marking any matrix row passed.')}
}
} finally { $caseLock.Dispose() }
