#requires -Version 7.0
[CmdletBinding()]
param(
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'current'),
    [string]$FixtureExecutable
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'The signing regression requires Windows.' }
$source = Join-Path (Split-Path $PSScriptRoot -Parent) 'source'
$stamp = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH-mm-ss-fff') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
$root = Join-Path ([IO.Path]::GetFullPath($OutputDirectory)) ('prepare-update-' + $stamp)
$fixtureSource = Join-Path $root 'source'
$release = Join-Path $root 'release'
$portable = Join-Path $release 'UnrealManager-Portable'
$null = New-Item -ItemType Directory -Path $fixtureSource, (Join-Path $fixtureSource 'src-tauri'), $portable, (Join-Path $root 'temp')
$previousTemp = $env:TEMP
$previousTmp = $env:TMP
$env:TEMP = Join-Path $root 'temp'
$env:TMP = $env:TEMP
$checks = [Collections.Generic.List[string]]::new()
$failure = $null
function Check([bool]$Condition, [string]$Name) { if (-not $Condition) { throw "Signing regression failed: $Name" }; $checks.Add($Name); Write-Host "PASS $Name" }
function Reject([scriptblock]$Action, [string]$Message, [string]$Name) {
    $caught = $null
    try { & $Action | Out-Null } catch { $caught = $_.Exception.Message }
    if ($null -eq $caught -or -not $caught.Contains($Message, [StringComparison]::Ordinal)) { throw "Expected refusal '$Message' for '$Name'; received: $caught" }
    Check ($null -ne $caught -and $caught.Contains($Message, [StringComparison]::Ordinal)) $Name
}
function Hash([byte[]]$Bytes) { [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($Bytes)).ToLowerInvariant() }
function Fixture-Key([string]$PrivatePath, [string]$PublicPath) {
    $emptyNamedKeyBefore = [Security.Cryptography.CngKey]::Exists([string]::Empty)
    & (Join-Path $source 'CREATE-UPDATE-KEY.ps1') -PrivateKeyPath $PrivatePath -PublicKeyPath $PublicPath
    Check ([Security.Cryptography.CngKey]::Exists([string]::Empty) -eq $emptyNamedKeyBefore) 'key creation preserves CNG named-key state'
}
function Fixture-Version([string]$Version) {
    $package = Get-Content -LiteralPath (Join-Path $fixtureSource 'package.json') -Raw | ConvertFrom-Json -AsHashtable
    $package.version = $Version
    $package | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $fixtureSource 'package.json') -Encoding utf8NoBOM
    $lock = Get-Content -LiteralPath (Join-Path $fixtureSource 'package-lock.json') -Raw | ConvertFrom-Json -AsHashtable
    $lock.version = $Version; $lock.packages[''].version = $Version
    $lock | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $fixtureSource 'package-lock.json') -Encoding utf8NoBOM
    $tauri = Get-Content -LiteralPath (Join-Path $fixtureSource 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json -AsHashtable
    $tauri.version = $Version
    $tauri | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $fixtureSource 'src-tauri/tauri.conf.json') -Encoding utf8NoBOM
    $cargoPath = Join-Path $fixtureSource 'src-tauri/Cargo.toml'
    $cargo = [IO.File]::ReadAllText($cargoPath)
    [IO.File]::WriteAllText($cargoPath, [regex]::Replace($cargo, '(?m)^version\s*=\s*"[^"]+"', ('version = "' + $Version + '"')), [Text.UTF8Encoding]::new($false))
}
try {
    foreach ($name in @('PREPARE-UPDATE.ps1', 'package.json', 'package-lock.json')) { Copy-Item -LiteralPath (Join-Path $source $name) -Destination $fixtureSource }
    foreach ($name in @('Cargo.toml', 'tauri.conf.json')) { Copy-Item -LiteralPath (Join-Path $source ('src-tauri/' + $name)) -Destination (Join-Path $fixtureSource 'src-tauri') }
    if (-not $FixtureExecutable) { $FixtureExecutable = Join-Path $source 'src-tauri/target/release/unreal-manager.exe' }
    $FixtureExecutable = [IO.Path]::GetFullPath($FixtureExecutable)
    $version = ([Diagnostics.FileVersionInfo]::GetVersionInfo($FixtureExecutable).ProductVersion).Trim()
    if (-not $version) { throw 'Build a native Unreal Manager release executable first, or pass -FixtureExecutable.' }
    Fixture-Version $version
    $private = Join-Path $root 'fixture-key.dpapi'
    $public = Join-Path $fixtureSource 'src-tauri/update-public-key.json'
    Fixture-Key $private $public
    $wrongPrivate = Join-Path $root 'wrong-key.dpapi'
    $wrongPublic = Join-Path $root 'wrong-public.json'
    Fixture-Key $wrongPrivate $wrongPublic
    $exe = Join-Path $portable 'unreal-manager.exe'
    [IO.File]::Copy($FixtureExecutable, $exe)
    $setup = Join-Path $release "UnrealManager-$version-Windows-x64-Setup.exe"
    [IO.File]::Copy($exe, $setup)
    foreach ($name in @('README.txt', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD-PARTY-LICENSES.txt')) { [IO.File]::WriteAllText((Join-Path $portable $name), "Synthetic test content: $name`n", [Text.UTF8Encoding]::new($false)) }
    [IO.File]::WriteAllText((Join-Path $portable 'portable.marker'), 'fixture marker must never enter the bundle')
    $prepare = Join-Path $fixtureSource 'PREPARE-UPDATE.ps1'
    $bundle = Join-Path $release "UnrealManager-$version-Windows-x64-Portable.umupdate"
    $manifestPath = Join-Path $release 'update-manifest.json'
    $signaturePath = Join-Path $release 'update-manifest.sig'
    $outputs = @($bundle, $manifestPath, $signaturePath)
    Reject { & $prepare -ReleaseDirectory $release -PrivateKeyPath $wrongPrivate } 'private signing key does not match' 'wrong signing key rejected'
    Check (@($outputs | Where-Object { Test-Path -LiteralPath $_ }).Count -eq 0) 'wrong key creates no outputs'
    Fixture-Version '0.0.0'
    $wrongVersionSetup = Join-Path $release 'UnrealManager-0.0.0-Windows-x64-Setup.exe'
    [IO.File]::Copy($exe, $wrongVersionSetup)
    Reject { & $prepare -ReleaseDirectory $release -PrivateKeyPath $private } 'product metadata' 'wrong executable product version rejected'
    [IO.File]::Delete($wrongVersionSetup)
    Fixture-Version $version
    $unexpected = Join-Path $portable 'account.session'
    [IO.File]::WriteAllText($unexpected, 'synthetic session sentinel')
    Reject { & $prepare -ReleaseDirectory $release -PrivateKeyPath $private } 'Unexpected portable input' 'unexpected portable files rejected'
    Check ([IO.File]::ReadAllText($unexpected) -ceq 'synthetic session sentinel') 'rejected input remains unchanged'
    [IO.File]::Delete($unexpected)
    & $prepare -ReleaseDirectory $release -PrivateKeyPath $private | Out-Null
    $bytes = [IO.File]::ReadAllBytes($manifestPath)
    $signature = [Convert]::FromBase64String([IO.File]::ReadAllText($signaturePath).Trim())
    $manifest = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json -AsHashtable
    Check ($bytes.Length -le 64KB -and $signature.Length -eq 64 -and -not ($bytes[0] -eq 0xef -and $bytes[1] -eq 0xbb)) 'bounded UTF-8 manifest and 64-byte P1363 signature'
    Check ((@($manifest.Keys) | Sort-Object) -join ',' -ceq 'appSha256,channel,installed,portable,schema,target,version') 'manifest has exact schema fields'
    Check ($manifest.schema -eq 1 -and $manifest.version -ceq $version -and $manifest.target -ceq 'windows-x64' -and $manifest.channel -ceq $(if ($version.Contains('-')) {'beta'} else {'stable'})) 'version channel and target match source metadata'
    foreach ($entry in @(@{name='installed';file=$setup}, @{name='portable';file=$bundle})) {
        $payload = $manifest[$entry.name]
        Check ($payload.name -ceq (Split-Path $entry.file -Leaf) -and $payload.url -ceq "https://github.com/MerryGrim/UnrealManager/releases/download/v$version/$($payload.name)" -and $payload.size -eq (Get-Item -LiteralPath $entry.file).Length -and $payload.sha256 -ceq (Get-FileHash -LiteralPath $entry.file -Algorithm SHA256).Hash.ToLowerInvariant()) ($entry.name + ' payload URL size and hash match exact artifact')
    }
    $publicBytes = [Convert]::FromBase64String((Get-Content -LiteralPath $public -Raw | ConvertFrom-Json).key)
    $key = [Security.Cryptography.CngKey]::Import($publicBytes, [Security.Cryptography.CngKeyBlobFormat]::EccPublicBlob)
    $verifier = [Security.Cryptography.ECDsaCng]::new($key)
    try {
        Check ($verifier.VerifyData($bytes, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)) 'independent public-key signature verification'
        $changed = [byte[]]$bytes.Clone(); $changed[0] = $changed[0] -bxor 1
        Check (-not $verifier.VerifyData($changed, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)) 'modified manifest fails signature verification'
        $changedSignature = [byte[]]$signature.Clone(); $changedSignature[0] = $changedSignature[0] -bxor 1
        Check (-not $verifier.VerifyData($bytes, $changedSignature, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)) 'modified signature rejected'
    } finally { $verifier.Dispose(); $key.Dispose() }
    $compressed = [IO.File]::OpenRead($bundle)
    $gzip = [IO.Compression.GZipStream]::new($compressed, [IO.Compression.CompressionMode]::Decompress)
    $reader = [IO.StreamReader]::new($gzip, [Text.UTF8Encoding]::new($false, $true))
    try { $decoded = $reader.ReadToEnd() | ConvertFrom-Json -AsHashtable } finally { $reader.Dispose(); $gzip.Dispose(); $compressed.Dispose() }
    $names = @('unreal-manager.exe', 'README.txt', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD-PARTY-LICENSES.txt')
    Check ($decoded.schema -eq 1 -and $decoded.version -ceq $version -and $decoded.files.Count -eq 5 -and (@($decoded.files | ForEach-Object {$_.name}) -join '|') -ceq ($names -join '|')) 'gzip bundle contains exactly the five allowed files, excludes marker'
    foreach ($file in $decoded.files) {
        Check ((Hash ([Convert]::FromBase64String($file.data))) -ceq (Get-FileHash -LiteralPath (Join-Path $portable $file.name) -Algorithm SHA256).Hash.ToLowerInvariant()) ('bundle preserves exact bytes of ' + $file.name)
    }
    Check ($manifest.appSha256 -ceq (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()) 'signed application hash matches bundled executable'
    $outputHashes = @($outputs | ForEach-Object { (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash })
    Reject { & $prepare -ReleaseDirectory $release -PrivateKeyPath $private } 'already exists' 'existing signed outputs never overwritten'
    Check ((@($outputs | ForEach-Object { (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash }) -join '|') -ceq ($outputHashes -join '|')) 'previous output bytes preserved after refusal'
} catch { $failure = $_.Exception.Message; throw } finally {
    $env:TEMP = $previousTemp
    $env:TMP = $previousTmp
    $report = [ordered]@{passed=($null -eq $failure);checks=$checks.ToArray();failure=$failure;evidence='Read-only copies of an existing native app PE, synthetic legal files and ephemeral DPAPI signing keys only; the Setup input is a fixture copy, no binary was launched and no real installer or app-update transition was exercised.'}
    $report | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $root 'report.json') -Encoding utf8NoBOM
    Write-Host "Signing regression report: $(Join-Path $root 'report.json')"
}
