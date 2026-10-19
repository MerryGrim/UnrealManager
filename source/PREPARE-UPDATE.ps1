#requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ReleaseDirectory,
    [string]$PortableDirectory,
    [string]$PrivateKeyPath = (Join-Path (Split-Path $PSScriptRoot -Parent) '.release-secrets/update-key.dpapi')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'Release signing requires Windows and PowerShell 7.' }

$umFiles = @('unreal-manager.exe', 'README.txt', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD-PARTY-LICENSES.txt')
$umMaximumFile = 64MB
$umMaximumUnpacked = 96MB
$umUtf8 = [Text.UTF8Encoding]::new($false, $true)
$umRepository = 'MerryGrim/UnrealManager'

function Assert-PlainPath([string]$Path, [switch]$File, [switch]$Directory) {
    $item = Get-Item -LiteralPath $Path -Force
    if (($File -and $item.PSIsContainer) -or ($Directory -and -not $item.PSIsContainer)) { throw "Unexpected input type: $Path" }
    $part = $item
    while ($null -ne $part) {
        if (($part.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Linked release inputs are not allowed: $Path" }
        $part = if ($part -is [IO.FileInfo]) { $part.Directory } else { $part.Parent }
    }
}
function Get-Sha256([byte[]]$Bytes) { [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($Bytes)).ToLowerInvariant() }
function Assert-Version([string]$Path, [string]$Version, [switch]$Application) {
    $info = [Diagnostics.FileVersionInfo]::GetVersionInfo($Path)
    if (([string]$info.ProductVersion).Trim() -cne $Version -or ([string]$info.ProductName).Trim() -cne 'Unreal Manager') {
        throw "Executable product metadata does not match Unreal Manager $Version`: $Path"
    }
    if ($Application) {
        $stream = [IO.File]::OpenRead($Path)
        $reader = [IO.BinaryReader]::new($stream)
        try {
            if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5A4D) { throw 'Application is not a Windows executable.' }
            [void]$stream.Seek(0x3c, [IO.SeekOrigin]::Begin)
            $offset = $reader.ReadUInt32()
            if ($offset -lt 64 -or $offset + 24 -gt $stream.Length) { throw 'Application has an invalid PE header.' }
            [void]$stream.Seek($offset, [IO.SeekOrigin]::Begin)
            if ($reader.ReadUInt32() -ne 0x00004550 -or $reader.ReadUInt16() -ne 0x8664) { throw 'Application must be a Windows x64 executable.' }
        } finally { $reader.Dispose(); $stream.Dispose() }
    }
}

$umSource = $PSScriptRoot
$umPackage = Get-Content -LiteralPath (Join-Path $umSource 'package.json') -Raw | ConvertFrom-Json -AsHashtable
$umVersion = [string]$umPackage.version
if ($umVersion -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$') { throw 'Release version must be SemVer without build metadata.' }
if ($Matches[4]) {
    foreach ($identifier in $Matches[4].Split('.')) {
        if ($identifier -cmatch '^0[0-9]+$') { throw 'Numeric SemVer prerelease identifiers cannot have leading zeroes.' }
    }
}
$umChannel = if ($umVersion.Contains('-')) { 'beta' } else { 'stable' }
$umTauri = Get-Content -LiteralPath (Join-Path $umSource 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json -AsHashtable
$umLock = Get-Content -LiteralPath (Join-Path $umSource 'package-lock.json') -Raw | ConvertFrom-Json -AsHashtable
$umCargoText = [IO.File]::ReadAllText((Join-Path $umSource 'src-tauri/Cargo.toml'))
$umCargoVersion = [regex]::Match($umCargoText, '(?m)^version\s*=\s*"([^"]+)"\s*$')
if (-not $umCargoVersion.Success -or $umCargoVersion.Groups[1].Value -cne $umVersion -or $umTauri.version -cne $umVersion -or $umLock.version -cne $umVersion -or $umLock.packages[''].version -cne $umVersion) {
    throw 'package.json, package-lock.json, Cargo.toml and tauri.conf.json versions must agree before signing.'
}

$umRelease = [IO.Path]::GetFullPath($ReleaseDirectory)
if (-not $PortableDirectory) { $PortableDirectory = Join-Path $umRelease 'UnrealManager-Portable' }
$umPortable = [IO.Path]::GetFullPath($PortableDirectory)
Assert-PlainPath $umRelease -Directory
Assert-PlainPath $umPortable -Directory
$umSetupName = "UnrealManager-$umVersion-Windows-x64-Setup.exe"
$umBundleName = "UnrealManager-$umVersion-Windows-x64-Portable.umupdate"
$umSetup = Join-Path $umRelease $umSetupName
$umOutputs = @((Join-Path $umRelease $umBundleName), (Join-Path $umRelease 'update-manifest.json'), (Join-Path $umRelease 'update-manifest.sig'))
foreach ($path in $umOutputs) { if (Test-Path -LiteralPath $path) { throw 'Update output already exists. Choose a fresh release directory; previous outputs are preserved.' } }
Assert-PlainPath $umSetup -File
Assert-Version $umSetup $umVersion
if ((Get-Item -LiteralPath $umSetup).Length -le 0 -or (Get-Item -LiteralPath $umSetup).Length -gt $umMaximumFile) { throw 'Installer must be nonempty and at most 64 MiB.' }
foreach ($item in @(Get-ChildItem -LiteralPath $umPortable -Force)) {
    if ($item.PSIsContainer -or $item.Name -cnotin ($umFiles + @('portable.marker', 'RELEASE-INFO.json'))) { throw "Unexpected portable input; use a clean distribution folder: $($item.Name)" }
    Assert-PlainPath $item.FullName -File
}

$umBundleFiles = @()
$umInputHashes = [ordered]@{}
$umTotal = 0L
foreach ($name in $umFiles) {
    $path = Join-Path $umPortable $name
    Assert-PlainPath $path -File
    $size = (Get-Item -LiteralPath $path).Length
    if ($size -le 0 -or $size -gt $umMaximumFile) { throw "Portable input must be nonempty and at most 64 MiB: $name" }
    $umTotal += $size
    if ($umTotal -gt $umMaximumUnpacked) { throw 'Portable files exceed the 96 MiB update limit.' }
    if ($name -ceq 'unreal-manager.exe') { Assert-Version $path $umVersion -Application }
    $bytes = [IO.File]::ReadAllBytes($path)
    $umInputHashes[$name] = Get-Sha256 $bytes
    $umBundleFiles += [ordered]@{name=$name;data=[Convert]::ToBase64String($bytes)}
}
$umBundleJson = [ordered]@{schema=1;version=$umVersion;files=$umBundleFiles} | ConvertTo-Json -Depth 5 -Compress
$umBundleBytes = $umUtf8.GetBytes($umBundleJson)
if ($umBundleBytes.Length -gt $umMaximumUnpacked) { throw 'Serialized portable bundle exceeds the 96 MiB update limit.' }
$umCompressed = [IO.MemoryStream]::new()
$umGzip = [IO.Compression.GZipStream]::new($umCompressed, [IO.Compression.CompressionLevel]::SmallestSize, $true)
try { $umGzip.Write($umBundleBytes, 0, $umBundleBytes.Length) } finally { $umGzip.Dispose() }
try { $umPayloadBytes = $umCompressed.ToArray() } finally { $umCompressed.Dispose() }
if ($umPayloadBytes.Length -eq 0 -or $umPayloadBytes.Length -gt $umMaximumFile) { throw 'Compressed portable bundle exceeds the 64 MiB update limit.' }

$umSetupHash = (Get-FileHash -LiteralPath $umSetup -Algorithm SHA256).Hash.ToLowerInvariant()
$umManifest = [ordered]@{
    schema=1;version=$umVersion;channel=$umChannel;target='windows-x64';appSha256=$umInputHashes['unreal-manager.exe']
    installed=[ordered]@{name=$umSetupName;url="https://github.com/$umRepository/releases/download/v$umVersion/$umSetupName";size=(Get-Item -LiteralPath $umSetup).Length;sha256=$umSetupHash}
    portable=[ordered]@{name=$umBundleName;url="https://github.com/$umRepository/releases/download/v$umVersion/$umBundleName";size=$umPayloadBytes.Length;sha256=(Get-Sha256 $umPayloadBytes)}
}
$umManifestBytes = $umUtf8.GetBytes(($umManifest | ConvertTo-Json -Depth 4) + "`n")
if ($umManifestBytes.Length -gt 64KB) { throw 'Manifest exceeds the application limit.' }

Add-Type -AssemblyName System.Security.Cryptography.ProtectedData
$umPublicPath = Join-Path $umSource 'src-tauri/update-public-key.json'
Assert-PlainPath $umPublicPath -File
Assert-PlainPath $PrivateKeyPath -File
$umPublicConfig = Get-Content -LiteralPath $umPublicPath -Raw | ConvertFrom-Json -AsHashtable
if ($umPublicConfig.algorithm -cne 'ecdsa-p256-sha256-p1363') { throw 'The tracked update key uses an unsupported algorithm.' }
$umExpectedPublic = [Convert]::FromBase64String($umPublicConfig.key)
if ([Convert]::ToBase64String($umExpectedPublic) -cne $umPublicConfig.key) { throw 'The tracked public key must use canonical base64 without whitespace.' }
$umPrivate = $null
$umKey = $null
$umSigner = $null
$umVerifier = $null
$umPublicKey = $null
try {
    $umPrivate = [Security.Cryptography.ProtectedData]::Unprotect([IO.File]::ReadAllBytes([IO.Path]::GetFullPath($PrivateKeyPath)), $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
    $umKey = [Security.Cryptography.CngKey]::Import($umPrivate, [Security.Cryptography.CngKeyBlobFormat]::EccPrivateBlob)
    $umActualPublic = $umKey.Export([Security.Cryptography.CngKeyBlobFormat]::EccPublicBlob)
    if ($umActualPublic.Length -ne 72 -or [Convert]::ToBase64String($umActualPublic) -cne [Convert]::ToBase64String($umExpectedPublic)) { throw 'The private signing key does not match the tracked application public key.' }
    $umSigner = [Security.Cryptography.ECDsaCng]::new($umKey)
    if ($umSigner.KeySize -ne 256) { throw 'The signing key must use ECDSA P-256.' }
    $umSignature = $umSigner.SignData($umManifestBytes, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)
    $umPublicKey = [Security.Cryptography.CngKey]::Import($umExpectedPublic, [Security.Cryptography.CngKeyBlobFormat]::EccPublicBlob)
    $umVerifier = [Security.Cryptography.ECDsaCng]::new($umPublicKey)
    if ($umSignature.Length -ne 64 -or -not $umVerifier.VerifyData($umManifestBytes, $umSignature, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)) { throw 'Generated manifest signature failed independent public-key verification.' }
} finally {
    if ($null -ne $umPrivate) { [Array]::Clear($umPrivate, 0, $umPrivate.Length) }
    if ($null -ne $umVerifier) { $umVerifier.Dispose() }
    if ($null -ne $umPublicKey) { $umPublicKey.Dispose() }
    if ($null -ne $umSigner) { $umSigner.Dispose() }
    if ($null -ne $umKey) { $umKey.Dispose() }
}

# Validate inputs again before creating any release outputs.
foreach ($name in $umFiles) {
    if ((Get-FileHash -LiteralPath (Join-Path $umPortable $name) -Algorithm SHA256).Hash.ToLowerInvariant() -cne $umInputHashes[$name]) { throw 'A portable input changed while preparing the update.' }
}
if ((Get-FileHash -LiteralPath $umSetup -Algorithm SHA256).Hash.ToLowerInvariant() -cne $umSetupHash) { throw 'The installer changed while preparing the update.' }
$umOutputBytes = @($umPayloadBytes, $umManifestBytes, $umUtf8.GetBytes([Convert]::ToBase64String($umSignature) + "`n"))
$umStreams = @()
$umCreated = @()
try {
    # Reserve every output with CreateNew before writing; a concurrent or old
    # release output can never be truncated by this tool.
    foreach ($path in $umOutputs) {
        $umStreams += [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        $umCreated += $path
    }
    for ($index=0; $index -lt $umOutputs.Length; $index++) {
        $umStreams[$index].Write($umOutputBytes[$index], 0, $umOutputBytes[$index].Length)
        $umStreams[$index].Flush($true)
    }
} catch {
    foreach ($stream in $umStreams) { $stream.Dispose() }
    foreach ($path in $umCreated) { [IO.File]::Delete($path) }
    throw
} finally { foreach ($stream in $umStreams) { $stream.Dispose() } }

Write-Host "Signed update prepared for $umVersion ($umChannel). Publish all three outputs beside the matching installer."
Get-FileHash -Algorithm SHA256 -LiteralPath $umOutputs | Select-Object @{Name='File';Expression={Split-Path $_.Path -Leaf}}, Hash
