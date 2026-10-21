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
function Assert-CompilerEngineVersion([string]$Text,[int]$ExitCode){
    # ISCC's PE version resource is 0.0.0.0 in the official release. Its
    # documented --version queries the loaded compiler engine instead.
    $actual=$Text.Trim()
    if($ExitCode -ne 0 -or $actual -cnotmatch '^(\d+)\.(\d+)\.(\d+)$' -or [int]$Matches[1] -ne 7 -or [int]$Matches[2] -ne 1 -or [int]$Matches[3] -ne 0){throw ('Unexpected Inno compiler engine version: exit='+$ExitCode+', actual="'+$actual+'"; expected 7.1.0. See compiler-version.json.')}
    return @{major=[int]$Matches[1];minor=[int]$Matches[2];build=[int]$Matches[3];text=$actual}
}
$innoSetup=Join-Path $tools 'innosetup-7.1.0-x64.exe'
$toolRecords+=Download-Trusted 'https://github.com/jrsoftware/issrc/releases/download/is-7_1_0/innosetup-7.1.0-x64.exe' $innoSetup '(?:CN|O)=Pyrsys B\.V\.'
$inno=Join-Path $tools 'InnoSetup7'
$p=Start-Process -FilePath $innoSetup -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/NOICONS',('/DIR="'+$inno+'"')) -WindowStyle Hidden -PassThru
if(-not $p.WaitForExit(180000) -or $p.ExitCode -ne 0){throw 'Hosted Inno provisioning failed; no force-kill attempted.'}
$compiler=Join-Path $inno 'ISCC.exe';$compilerFiles=@()
foreach($name in @('ISCC.exe','ISCmplr.dll','ISPP.dll')){
    $file=Join-Path $inno $name;Assert-Plain $file
    $signature=Get-AuthenticodeSignature -LiteralPath $file
    if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(?:CN|O)=Pyrsys B\.V\.'){throw ('Installed compiler publisher signature mismatch: '+$name)}
    $metadata=[Diagnostics.FileVersionInfo]::GetVersionInfo($file)
    $compilerFiles+=@{file=$name;sha256=(Test-Hash $file);publisher=$signature.SignerCertificate.Subject;fileVersion=$metadata.FileVersion;productVersion=$metadata.ProductVersion;fileParts=@($metadata.FileMajorPart,$metadata.FileMinorPart,$metadata.FileBuildPart,$metadata.FilePrivatePart)}
}
$versionOut=Join-Path $out 'logs/compiler-version.stdout.log';$versionErr=Join-Path $out 'logs/compiler-version.stderr.log'
$query=Start-Process -FilePath $compiler -ArgumentList '--version' -WindowStyle Hidden -RedirectStandardOutput $versionOut -RedirectStandardError $versionErr -PassThru
$versionExited=$query.WaitForExit(10000)
if($versionExited){$query.Refresh()}
$versionText=[IO.File]::ReadAllText($versionOut)
$versionEvidence=@{schema=1;source='Official is-7_1_0 release; ISCC --version reads loaded CompilerVersionInfo.Version';exited=$versionExited;exitCode=$(if($versionExited){$query.ExitCode}else{$null});stdout=$versionText.Trim();files=$compilerFiles}
New-Json (Join-Path $out 'logs/compiler-version.json') $versionEvidence
if(-not $versionExited){throw 'Inno compiler version query timeout; packaging not started. See compiler-version.json.'}
$engineVersion=Assert-CompilerEngineVersion $versionText $query.ExitCode
$toolRecords+=@{source='Installed signed compiler';version=$engineVersion;files=$compilerFiles}
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
