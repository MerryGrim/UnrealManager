#requires -Version 5.1
[CmdletBinding()]
param([Parameter(Mandatory)][string]$OutputPath,[Parameter(Mandatory)][string]$ToolRoot,[string[]]$ImageRoots=@())
$ErrorActionPreference='Stop'
$target=[IO.Path]::GetFullPath($OutputPath)
if (Test-Path -LiteralPath $target) { throw 'Refusing to overwrite audit evidence.' }
$features=@(Get-CimInstance Win32_OptionalFeature -Filter "Name='Microsoft-Hyper-V-All' OR Name='Containers-DisposableClientVM' OR Name='VirtualMachinePlatform' OR Name='HypervisorPlatform'" | Select-Object Name,InstallState)
$services=@(Get-CimInstance Win32_Service -Filter "Name='vmms' OR Name='vmcompute' OR Name='hns'" | Select-Object Name,State,StartMode)
$system=Get-CimInstance Win32_ComputerSystem
$os=Get-CimInstance Win32_OperatingSystem
$commands=@(Get-Command Get-VM,Get-VMHost,WindowsSandbox.exe,vmrun.exe,VBoxManage.exe,qemu-system-x86_64.exe -ErrorAction SilentlyContinue | Select-Object Name,Source)
$paths=@("$env:WINDIR\System32\WindowsSandbox.exe","$env:WINDIR\System32\virtmgmt.msc","$env:ProgramFiles\Oracle\VirtualBox\VBoxManage.exe","$env:ProgramFiles\VMware\VMware Workstation\vmrun.exe",(Join-Path $ToolRoot 'VirtualBox\VBoxManage.exe'),(Join-Path $ToolRoot 'qemu\qemu-system-x86_64.exe'))
$executables=foreach($path in $paths){@{path=$path;exists=Test-Path -LiteralPath $path}}
$images=foreach($root in $ImageRoots){$present=Test-Path -LiteralPath $root; $found=@();if($present){$found=@(Get-ChildItem -LiteralPath $root -File -Depth 2 -ErrorAction SilentlyContinue | Where-Object{$_.Extension -in @('.iso','.vhd','.vhdx','.vmdk','.ova','.qcow2','.wim','.esd')} | Select-Object FullName,Length)};@{root=$root;exists=$present;maximumDepth=2;images=$found}}
$result=@{schema=1;observedUtc=[DateTime]::UtcNow.ToString('o');readOnly=$true;system=@{manufacturer=$system.Manufacturer;model=$system.Model;hypervisorPresent=$system.HypervisorPresent};os=@{caption=$os.Caption;build=$os.BuildNumber;version=$os.Version;architecture=$os.OSArchitecture};features=$features;services=$services;commands=$commands;knownExecutablePaths=$executables;boundedImageSearch=$images;featuresEnabledByAudit=$false;osDownloads=$false;claim='HypervisorPresent alone does not establish a configured or usable VM. Image absence is limited to the named roots and depth.'}
[IO.File]::WriteAllText($target,($result|ConvertTo-Json -Depth 12),(New-Object Text.UTF8Encoding($false)))
Write-Output $target
