#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'HostedLifecycle.Common.ps1')
$hostScope=Assert-HostedWindowsServer;$out=Hosted-Output $OutputDirectory
[void][IO.Directory]::CreateDirectory($out);[void][IO.Directory]::CreateDirectory((Join-Path $out 'logs'))
$tools=Join-Path $out 'tools';[void][IO.Directory]::CreateDirectory($tools)
$source=Join-Path $env:GITHUB_WORKSPACE 'source';$packages=Join-Path $out 'Packages'
$env:UM_TEMP_DIR=Join-Path $out 'temp';[void][IO.Directory]::CreateDirectory($env:UM_TEMP_DIR);$env:TEMP=$env:UM_TEMP_DIR;$env:TMP=$env:UM_TEMP_DIR
$toolRecords=@()
function Download-Trusted([string]$Url,[string]$Path,[string]$Publisher){
    Invoke-WebRequest -Uri $Url -OutFile $Path -TimeoutSec 120
    if((Get-Item -LiteralPath $Path).Length -gt 32MB){throw 'Unexpected tool download size.'}
    $signature=Get-AuthenticodeSignature -LiteralPath $Path
    if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch $Publisher){throw 'Tool publisher signature mismatch.'}
    return @{url=$Url;sha256=(Test-Hash $Path);publisher=$signature.SignerCertificate.Subject}
}
$innoSetup=Join-Path $tools 'innosetup-7.1.0-x64.exe'
$toolRecords+=Download-Trusted 'https://github.com/jrsoftware/issrc/releases/download/is-7_1_0/innosetup-7.1.0-x64.exe' $innoSetup '(?:CN|O)=Pyrsys B\.V\.'
$inno=Join-Path $tools 'InnoSetup7'
$p=Start-Process -FilePath $innoSetup -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/NOICONS',('/DIR="'+$inno+'"')) -WindowStyle Hidden -PassThru
if(-not $p.WaitForExit(180000) -or $p.ExitCode -ne 0){throw 'Hosted Inno provisioning failed; no force-kill attempted.'}
$compiler=Join-Path $inno 'ISCC.exe';if([Diagnostics.FileVersionInfo]::GetVersionInfo($compiler).FileVersion -notlike '7.1.0*'){throw 'Unexpected Inno compiler version.'}
$dependency=Join-Path $source 'installer/dependencies';[void][IO.Directory]::CreateDirectory($dependency)
$bootstrap=Join-Path $dependency 'MicrosoftEdgeWebview2Setup.exe'
if(Test-Path -LiteralPath $bootstrap){$sig=Get-AuthenticodeSignature -LiteralPath $bootstrap;if($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation'){throw 'Existing bootstrapper signature mismatch.'};$toolRecords+=@{source='existing official local file';sha256=(Test-Hash $bootstrap)}}else{$toolRecords+=Download-Trusted 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' $bootstrap 'O=Microsoft Corporation'}
$env:CARGO_NET_OFFLINE='true';$env:npm_config_offline='true';$env:npm_config_update_notifier='false'
& (Join-Path $source 'BUILD-WINDOWS.ps1') -Configuration release -SkipDependencies *> (Join-Path $out 'logs/production-build.log')
if($LASTEXITCODE -ne 0){throw 'Hosted production build failed.'}
& (Join-Path $source 'BUILD-INSTALLER.ps1') -SkipBuild -Compiler $compiler -OutputDirectory $packages *> (Join-Path $out 'logs/installer-build.log')
if($LASTEXITCODE -ne 0){throw 'Hosted Setup packaging failed.'}
$version=(Get-Content -LiteralPath (Join-Path $source 'package.json') -Raw|ConvertFrom-Json).version
$portable=Join-Path $packages 'UnrealManager-Portable';$zip=Join-Path $packages "UnrealManager-$version-Windows-x64-Portable.zip"
[IO.Compression.ZipFile]::CreateFromDirectory($portable,$zip,[IO.Compression.CompressionLevel]::Optimal,$true)
$files=@(Get-ChildItem -LiteralPath $packages -File|ForEach-Object{@{name=$_.Name;size=$_.Length;sha256=(Test-Hash $_.FullName)}})
New-Json (Join-Path $packages 'build-info.json') @{schema=1;sourceCommit=$env:GITHUB_SHA;version=$version;appSha256=(Test-Hash (Join-Path $portable 'unreal-manager.exe'));files=$files;updateSigningKeyUsed=$false;publishableRelease=$false}
New-Json (Join-Path $out 'build-report.json') @{schema=1;passed=$true;host=$hostScope;sourceCommit=$env:GITHUB_SHA;version=$version;tools=$toolRecords;packages=$packages;signingSecretsUsed=$false;published=$false;runtimeExecuted=$false;scope='Local unsigned CI Setup/ZIP built from this checkout; no application-update signing manifest or release publication.'}
Write-Output $packages
