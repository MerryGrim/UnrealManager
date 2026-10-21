# Test-only helpers. Never imported by the launcher or its installer.
Set-StrictMode -Version Latest
function Assert-HostedWindowsServer {
    if ($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or $env:RUNNER_OS -cne 'Windows' -or $env:GITHUB_RUN_ID -notmatch '^\d+$' -or -not $env:GITHUB_WORKSPACE) { throw 'This operation is restricted to disposable GitHub-hosted Windows jobs.' }
    $os=Get-CimInstance Win32_OperatingSystem; $system=Get-CimInstance Win32_ComputerSystem
    if ([int]$os.ProductType -eq 1 -or ($system.Manufacturer+' '+$system.Model) -notmatch '(?i)virtual|vmware|qemu|kvm') { throw 'A hosted Windows Server VM is required; desktop/user hosts are forbidden.' }
    $principal=[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    return @{caption=$os.Caption;version=$os.Version;build=[int]$os.BuildNumber;productType=[int]$os.ProductType;imageVersion=$env:ImageVersion;imageOS=$env:ImageOS;elevated=$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator);scope='Disposable hosted Server image with development tools. Not clean Windows 10/11 or standard-user/UAC acceptance.'}
}
function Assert-Plain([string]$Path) {
    $part=[IO.Path]::GetFullPath($Path)
    while($part){if(Test-Path -LiteralPath $part){if((Get-Item -LiteralPath $part -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked test input/output rejected.'}};$next=[IO.Path]::GetDirectoryName($part);if($next -eq $part){break};$part=$next}
}
function Hosted-Output([string]$Path) {
    $out=[IO.Path]::GetFullPath($Path);$allowed=[IO.Path]::GetFullPath((Join-Path $env:GITHUB_WORKSPACE 'verification/current'))
    if(-not $out.StartsWith($allowed+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Hosted outputs must be a fresh child of workspace verification/current.'}
    Assert-Plain $out;if(Test-Path -LiteralPath $out){throw 'Hosted output already exists; refusing overwrite.'};return $out
}
function Test-Hash([string]$Path){Assert-Plain $Path;(Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()}
function New-Json([string]$Path,$Value){Assert-Plain $Path;$bytes=[Text.UTF8Encoding]::new($false).GetBytes(($Value|ConvertTo-Json -Depth 16));$file=[IO.File]::Open($Path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read);try{$file.Write($bytes,0,$bytes.Length)}finally{$file.Dispose()}}
function Runtime-Registration {
    foreach($hive in @([Microsoft.Win32.RegistryHive]::LocalMachine,[Microsoft.Win32.RegistryHive]::CurrentUser)){foreach($view in @([Microsoft.Win32.RegistryView]::Registry32,[Microsoft.Win32.RegistryView]::Registry64)){
        $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($hive,$view)
        try{$key=$base.OpenSubKey('SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}');if($key){try{$pv=[string]$key.GetValue('pv');if($pv -and $pv -ne '0.0.0.0'){@{hive=[string]$hive;view=[string]$view;version=$pv}}}finally{$key.Dispose()}}}finally{$base.Dispose()}
    }}
}
function Known-RuntimeFiles {
    foreach($base in @((Join-Path ${env:ProgramFiles(x86)} 'Microsoft/EdgeWebView/Application'),(Join-Path $env:ProgramFiles 'Microsoft/EdgeWebView/Application'),(Join-Path $env:LOCALAPPDATA 'Microsoft/EdgeWebView/Application'))){
        if(Test-Path -LiteralPath $base){foreach($version in @(Get-ChildItem -LiteralPath $base -Directory -ErrorAction Stop)){$exe=Join-Path $version.FullName 'msedgewebview2.exe';if(Test-Path -LiteralPath $exe){@{version=$version.Name;location=$(if($base.StartsWith($env:LOCALAPPDATA,[StringComparison]::OrdinalIgnoreCase)){'currentUser'}else{'programFiles'})}}}}
    }
}
function App-Registration {
    foreach($hive in @([Microsoft.Win32.RegistryHive]::LocalMachine,[Microsoft.Win32.RegistryHive]::CurrentUser)){foreach($view in @([Microsoft.Win32.RegistryView]::Registry32,[Microsoft.Win32.RegistryView]::Registry64)){
        $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($hive,$view)
        try{$key=$base.OpenSubKey('SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{61185C58-31CF-4783-BA27-429C3454B9A5}_is1');if($key){try{@{hive=[string]$hive;view=[string]$view;installLocation=[string]$key.GetValue('InstallLocation')}}finally{$key.Dispose()}}}finally{$base.Dispose()}
    }}
}
function Same-Path([string]$Left,[string]$Right){[IO.Path]::GetFullPath($Left).TrimEnd('\','/').Equals([IO.Path]::GetFullPath($Right).TrimEnd('\','/'),[StringComparison]::OrdinalIgnoreCase)}
function Pin-Process($Process,[string]$ExpectedPath){$Process.Refresh();if(-not(Same-Path $Process.Path $ExpectedPath)){throw 'Test process image mismatch.'};@{pid=$Process.Id;created=$Process.StartTime.ToUniversalTime().ToFileTimeUtc();path=$Process.Path}}
function Find-Pinned($Pin){$p=Get-Process -Id $Pin.pid -ErrorAction SilentlyContinue;if($p){try{if($p.StartTime.ToUniversalTime().ToFileTimeUtc() -eq $Pin.created -and (Same-Path $p.Path $Pin.path)){return $p}}catch{throw 'Cannot confirm pinned process identity.'}}}
function Owned-Tree($Pin){
    $root=Find-Pinned $Pin;if(-not $root){return}
    $all=@(Get-CimInstance Win32_Process);$ids=@{};$ids[[int]$Pin.pid]=[long]$Pin.created;$changed=$true
    while($changed){$changed=$false;foreach($p in $all){if(-not $ids.ContainsKey([int]$p.ProcessId) -and $ids.ContainsKey([int]$p.ParentProcessId) -and $p.CreationDate.ToUniversalTime().ToFileTimeUtc() -ge $ids[[int]$p.ParentProcessId]){$child=Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue;if($child){$created=$child.StartTime.ToUniversalTime().ToFileTimeUtc();if([Math]::Abs($created-$p.CreationDate.ToUniversalTime().ToFileTimeUtc()) -ge 10){throw 'Child identity changed.'};$ids[[int]$p.ProcessId]=$created;$changed=$true}}}}
    foreach($p in $all){if($ids.ContainsKey([int]$p.ProcessId)){$wv=$p.Name -ieq 'msedgewebview2.exe';$readable=$wv -and -not [string]::IsNullOrWhiteSpace([string]$p.CommandLine);@{pid=$p.ProcessId;created=$ids[[int]$p.ProcessId];name=$p.Name;browserRoot=($readable -and $p.CommandLine -notmatch '(?:^|\s)--type=');commandLineReadable=$readable;disableGpu=$(if($readable){$p.CommandLine -match '(?:^|\s)--disable-gpu(?:\s|$)'}else{$null});disableCompositing=$(if($readable){$p.CommandLine -match '(?:^|\s)--disable-gpu-compositing(?:\s|$)'}else{$null})}}}
}
function Remove-OwnedTree([string]$Path,[string]$Expected,[string]$OwnerId){
    if(-not(Test-Path -LiteralPath $Path)){return @{absent=$true;files=0;bytes=0}}
    if(-not(Same-Path $Path $Expected)){throw 'Cleanup target mismatch.'};Assert-Plain $Path
    $marker=Join-Path $Path '.distribution-owner';if(-not(Test-Path -LiteralPath $marker) -or [IO.File]::ReadAllText($marker) -cne $OwnerId){throw 'Cleanup owner marker missing or changed.'}
    $entries=@(Get-ChildItem -LiteralPath $Path -Force -Recurse);if($entries.Count -gt 20000){throw 'Unexpected fixture inventory size.'}
    foreach($entry in $entries){if($entry.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Cleanup refused a linked fixture entry.'};if(-not $entry.FullName.StartsWith($Path.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Cleanup inventory escaped target.'}}
    $files=@($entries|Where-Object{-not $_.PSIsContainer});$bytes=($files|Measure-Object Length -Sum).Sum
    foreach($file in $files){[IO.File]::Delete($file.FullName)}
    foreach($dir in @($entries|Where-Object{$_.PSIsContainer}|Sort-Object {$_.FullName.Length} -Descending)){[IO.Directory]::Delete($dir.FullName,$false)}
    [IO.Directory]::Delete($Path,$false);@{absent=(-not(Test-Path -LiteralPath $Path));files=$files.Count;bytes=$bytes}
}
