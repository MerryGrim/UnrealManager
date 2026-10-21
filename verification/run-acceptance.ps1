#requires -Version 7.0
[CmdletBinding()]
param(
    [switch]$ValidateOnly,
    [string]$PlaywrightModule = $env:UM_PLAYWRIGHT_MODULE,
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'current'),
    [switch]$IncludeSignedUpdater,
    [string]$SigningKeyPath
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Acceptance requires Windows, PowerShell 7, MSVC and an installed Microsoft Edge.' }
$project = Split-Path $PSScriptRoot -Parent
$source = Join-Path $project 'source'
$current = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'current'))
$parent = [IO.Path]::GetFullPath($OutputDirectory)
if ($parent -ne $current -and -not $parent.StartsWith($current + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Reports must stay under verification/current.' }
$id = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH-mm-ss-fff') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
$run = Join-Path $parent ('acceptance-' + $id)
if (Test-Path -LiteralPath $run) { throw 'Acceptance output already exists.' }
$null = New-Item -ItemType Directory -Path $run,(Join-Path $run 'logs'),(Join-Path $run 'reports')
$reportPath = Join-Path $run 'reports/acceptance-report.json'
$report = [ordered]@{
    schema=1;startedAt=[DateTime]::UtcNow.ToString('o');finishedAt=$null
    mode=$(if($ValidateOnly){'validation-only'}else{'offline-acceptance'});passed=$null
    version=$null;sourceCommit=$env:GITHUB_SHA;nativeTestExecutable=$null;stages=@();sourceInputs=@();sourceInputsScope='Metadata and entry-point hashes only; not a full production-source attestation.';tools=@{}
    signedUpdater=[ordered]@{requested=[bool]$IncludeSignedUpdater;executed=$false;reason='Opt-in local signing-key fixtures; never part of CI.'}
    limitations=@('Mock UI is not a real Epic/Fab transfer, engine install or application update.','No application/runtime/GPU collector, installer, publication or key generation runs by default.','Dependency provisioning is separate; offline acceptance cannot populate missing caches.')
    fatalError=$null
}
$oldLocation = Get-Location
$names = @('TEMP','TMP','UM_TEMP_DIR','UM_PLAYWRIGHT_MODULE','UM_VERIFICATION_OUTPUT_DIRECTORY','UM_ENGINE_VERIFY_FIXTURE_ROOT','UM_ENGINE_APPLY_FIXTURE_ROOT','UM_ENGINE_ACCESS_FIXTURE_ROOT','UM_ELEVATION_FIXTURE_ROOT','CARGO_NET_OFFLINE','npm_config_offline','npm_config_update_notifier')
$previous = @{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
$temporary = Join-Path $current ('at-' + [guid]::NewGuid().ToString('N').Substring(0,8))
$PSNativeCommandUseErrorActionPreference = $false
function Stage([string]$Name,[string]$Executable,[string[]]$Arguments){
    $entry=[ordered]@{name=$Name;startedAt=[DateTime]::UtcNow.ToString('o');finishedAt=$null;executable=$Executable;arguments=$Arguments;exitCode=$null;passed=$false;log=(Join-Path $run ('logs/'+$Name+'.log'))}
    try {Write-Host "RUN $Name";& $Executable @Arguments 2>&1 | Tee-Object -LiteralPath $entry.log | Out-Host;$entry.exitCode=$LASTEXITCODE;$entry.passed=$entry.exitCode -eq 0}
    catch {$_ | Out-String | Add-Content -LiteralPath $entry.log;$entry.exitCode=-1}
    finally {$entry.finishedAt=[DateTime]::UtcNow.ToString('o');$report.stages+=,[pscustomobject]$entry;Write-Host "$(if($entry.passed){'PASS'}else{'FAIL'}) $Name"}
    return $entry.passed
}
try {
    $null=New-Item -ItemType Directory -Path $temporary
    $env:UM_TEMP_DIR=$temporary
    . (Join-Path $source 'BUILD-ENV.ps1')
    if($PlaywrightModule){$env:UM_PLAYWRIGHT_MODULE=[IO.Path]::GetFullPath($PlaywrightModule)}
    $env:UM_VERIFICATION_OUTPUT_DIRECTORY=Join-Path $run 'ui'
    $env:UM_ENGINE_VERIFY_FIXTURE_ROOT=Join-Path $run 'fixtures/verify'
    $env:UM_ENGINE_APPLY_FIXTURE_ROOT=Join-Path $run 'fixtures/apply'
    $env:UM_ENGINE_ACCESS_FIXTURE_ROOT=Join-Path $run 'fixtures/access'
    $env:UM_ELEVATION_FIXTURE_ROOT=Join-Path $run 'fixtures/elevation'
    $report.fixtureRoots=@{temporary=$temporary;verify=$env:UM_ENGINE_VERIFY_FIXTURE_ROOT;apply=$env:UM_ENGINE_APPLY_FIXTURE_ROOT;access=$env:UM_ENGINE_ACCESS_FIXTURE_ROOT;elevation=$env:UM_ELEVATION_FIXTURE_ROOT}
    $env:CARGO_NET_OFFLINE='true';$env:npm_config_offline='true';$env:npm_config_update_notifier='false'
    $report.version=(Get-Content -LiteralPath (Join-Path $source 'package.json') -Raw|ConvertFrom-Json).version
    $report.host=[ordered]@{githubActions=$env:GITHUB_ACTIONS -eq 'true';runnerOS=$env:RUNNER_OS;imageOS=$env:ImageOS;imageVersion=$env:ImageVersion;declaredScope=$env:UM_ACCEPTANCE_HOSTED_SCOPE;osCaption=$null;osVersion=$null;osProductType=$null;administrativeToken=$null;metadataError=$null}
    try {
        $os=Get-CimInstance Win32_OperatingSystem
        $report.host.osCaption=$os.Caption;$report.host.osVersion=$os.Version;$report.host.osProductType=[int]$os.ProductType
        $token=[Security.Principal.WindowsIdentity]::GetCurrent()
        try{$report.host.administrativeToken=([Security.Principal.WindowsPrincipal]::new($token)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)}finally{$token.Dispose()}
    }catch{$report.host.metadataError='OS/token classification unavailable; no platform or token-scope inference.'}
    foreach($relative in @('source/package.json','source/package-lock.json','source/src-tauri/Cargo.toml','source/src-tauri/Cargo.lock','source/src-tauri/tauri.conf.json','source/BUILD-ENV.ps1','verification/run-acceptance.ps1','verification/run-regression.cjs')){
        $file=Join-Path $project $relative;$report.sourceInputs+=,[pscustomobject]@{path=$relative;sha256=(Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()}
    }
    $node=(Get-Command node -ErrorAction Stop).Source;$cargo=(Get-Command cargo -ErrorAction Stop).Source
    $report.tools.node=(& $node --version);$report.tools.cargo=(& $cargo --version);$report.tools.rustc=(& rustc --version);$report.tools.target=$env:CARGO_TARGET_DIR
    foreach($file in @('node_modules/typescript/bin/tsc','node_modules/vite/bin/vite.js')){
        if(-not(Test-Path -LiteralPath (Join-Path $source $file)-PathType Leaf)){throw "Missing locked project dependency $file. Run npm ci separately."}
    }
    if($IncludeSignedUpdater){
        if($env:CI){throw 'Signed updater fixtures are local opt-in only and cannot run in CI.'}
        if(-not $SigningKeyPath -or -not(Test-Path -LiteralPath $SigningKeyPath -PathType Leaf)){throw 'IncludeSignedUpdater requires an explicit existing DPAPI SigningKeyPath. No key is created.'}
        $SigningKeyPath=[IO.Path]::GetFullPath($SigningKeyPath)
    }
    Set-Location -LiteralPath $source
    if(-not(Stage 'validate-ui-plan' $node @((Join-Path $PSScriptRoot 'run-regression.cjs'),'--validate'))){throw 'UI dependency/selection validation failed.'}
    if($ValidateOnly){$report.validated=$true}
    else {
        $report.validated=$true
        $null=Stage 'typescript' $node @((Join-Path $source 'node_modules/typescript/bin/tsc'),'--noEmit','--project',(Join-Path $source 'tsconfig.json'))
        # generate_context embeds frontendDist in release tests. Never install
        # dependencies/npx here; only build assets with the locked local Vite.
        $assets=Stage 'frontend-assets' $node @((Join-Path $source 'node_modules/vite/bin/vite.js'),'build')
        if($assets){
            $null=Stage 'native-production-check' $cargo @('check','--manifest-path',(Join-Path $source 'src-tauri/Cargo.toml'),'--lib','--release','--locked','--offline')
            $native=Stage 'native-unit' $cargo @('test','--manifest-path',(Join-Path $source 'src-tauri/Cargo.toml'),'--lib','--release','--locked','--offline','--message-format=json','--','--test-threads=1')
            if($native){
                $binaries=@(Get-Content -LiteralPath (Join-Path $run 'logs/native-unit.log') | ForEach-Object {
                    try{$line=$_|ConvertFrom-Json -ErrorAction Stop;if($line.reason -eq 'compiler-artifact' -and $line.target.name -eq 'unreal_manager_lib' -and $line.profile.test -and $line.executable){$line.executable}}catch{}
                } | Select-Object -Unique)
                if($binaries.Count -ne 1 -or -not(Test-Path -LiteralPath $binaries[0] -PathType Leaf)){throw 'Cannot identify the freshly tested library executable from this Cargo JSON.'}
                $report.nativeTestExecutable=[ordered]@{path=[IO.Path]::GetFullPath($binaries[0]);sha256=(Get-FileHash -LiteralPath $binaries[0] -Algorithm SHA256).Hash.ToLowerInvariant();sourceCommit=$env:GITHUB_SHA;target='unreal_manager_lib';profile='release-test';source='This run native-unit Cargo compiler-artifact JSON, unique test executable.'}
            }
        }else{$native=$false}
        $null=Stage 'ui-regression' $node @((Join-Path $PSScriptRoot 'run-regression.cjs'))
        $null=Stage 'runtime-collector-contract' (Get-Process -Id $PID).Path @('-NoProfile','-File',(Join-Path $PSScriptRoot 'test-runtime-evidence.ps1'))
        $null=Stage 'privacy-self-test' $node @((Join-Path $PSScriptRoot 'check-public-privacy.cjs'),'--self-test')
        $null=Stage 'privacy-source' $node @((Join-Path $PSScriptRoot 'check-public-privacy.cjs'),'--output',(Join-Path $run 'reports/privacy-source.json'))
        if($IncludeSignedUpdater){
            if(-not $native){throw "Signed updater fixtures require this run's successful native suite."}
            if(-not $report.nativeTestExecutable){throw 'Missing this run native test executable provenance.'}
            $report.signedUpdater.executed=$true
            $null=Stage 'signed-updater-local' $node @((Join-Path $PSScriptRoot 'test-update-native.cjs'),'--test-executable',$report.nativeTestExecutable.path,'--powershell',(Get-Process -Id $PID).Path,'--private-key',$SigningKeyPath,'--output-directory',(Join-Path $run 'signed-updater'))
        }
        $report.passed=@($report.stages|Where-Object{-not $_.passed}).Count -eq 0
    }
}catch{$report.fatalError=$_.Exception.Message;if(-not $ValidateOnly){$report.passed=$false};Write-Warning $report.fatalError}
finally{
    $report.finishedAt=[DateTime]::UtcNow.ToString('o');$report.temporaryDirectory=$temporary;$report.temporaryRetained=(Test-Path -LiteralPath $temporary)
    # Retain evidence/fixtures; do not hide recursive deletion/process control.
    $report|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $reportPath -Encoding utf8
    Set-Location -LiteralPath $oldLocation
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
    Write-Host "Report: $reportPath"
}
if($report.fatalError -or (-not $ValidateOnly -and -not $report.passed)){exit 1}
