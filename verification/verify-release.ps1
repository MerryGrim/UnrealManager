[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ReleaseDirectory,
    [Parameter(Mandatory = $true)][string]$ExpectedExecutable,
    [string]$Version = '0.2.0-beta.6',
    [string]$OutputDirectory
)

# Read package inputs only. Never install, launch, sign, modify or delete them.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$releaseRoot = (Resolve-Path -LiteralPath $ReleaseDirectory).ProviderPath
$expectedExe = (Resolve-Path -LiteralPath $ExpectedExecutable).ProviderPath
$outputBase = if ($OutputDirectory) { [IO.Path]::GetFullPath($OutputDirectory) } else { Join-Path $PSScriptRoot 'current' }
$stamp = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH-mm-ss-fffZ')
$runDirectory = Join-Path $outputBase ("release-validation-$stamp-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
[void](New-Item -ItemType Directory -Path $runDirectory)
$checks = [Collections.Generic.List[object]]::new()
$inventory = [Collections.Generic.List[object]]::new()
$archive = $null
$archivePath = Join-Path $releaseRoot "UnrealManager-$Version-Windows-x64-Portable.zip"
$installerPath = Join-Path $releaseRoot "UnrealManager-$Version-Windows-x64-Setup.exe"
$metadata = [ordered]@{}
$errorMessage = $null

function Require-Check([bool]$Condition, [string]$Name) {
    $checks.Add([PSCustomObject]@{ Name = $Name; Passed = $Condition })
    if (-not $Condition) { throw "Release validation failed: $Name" }
}

function Get-PortableExecutableInfo([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5A4D) { throw 'Missing MZ executable header' }
        [void]$stream.Seek(0x3C, [IO.SeekOrigin]::Begin)
        $peOffset = $reader.ReadUInt32()
        if ($peOffset -lt 64 -or $peOffset + 24 -gt $stream.Length) { throw 'Invalid PE header offset' }
        [void]$stream.Seek($peOffset, [IO.SeekOrigin]::Begin)
        if ($reader.ReadUInt32() -ne 0x00004550) { throw 'Missing PE executable header' }
        $machine = $reader.ReadUInt16()
    } finally { $reader.Dispose(); $stream.Dispose() }
    $versionInfo = [Diagnostics.FileVersionInfo]::GetVersionInfo($Path)
    return [PSCustomObject]@{
        Machine = ('0x{0:X4}' -f $machine)
        ProductVersion = $versionInfo.ProductVersion
        FileVersion = $versionInfo.FileVersion
        ProductName = $versionInfo.ProductName
        Length = (Get-Item -LiteralPath $Path).Length
        Sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    }
}

try {
    Require-Check (Test-Path -LiteralPath $archivePath -PathType Leaf) 'versioned portable ZIP exists'
    Require-Check (Test-Path -LiteralPath $installerPath -PathType Leaf) 'versioned installer exists'
    $metadata['ExpectedExecutable'] = Get-PortableExecutableInfo $expectedExe
    Require-Check ($metadata['ExpectedExecutable'].Machine -eq '0x8664') 'expected application PE is AMD64'
    Require-Check ($metadata['ExpectedExecutable'].ProductVersion -eq $Version) 'expected application product version matches release'

    $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
    $allowed = @('unreal-manager.exe', 'portable.marker', 'README.txt', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD-PARTY-LICENSES.txt', 'RELEASE-INFO.json')
    $required = @('unreal-manager.exe', 'portable.marker', 'README.txt', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD-PARTY-LICENSES.txt')
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $files = @{}
    $totalBytes = 0L
    foreach ($entry in $archive.Entries) {
        $name = $entry.FullName.Replace('\', '/')
        Require-Check ($names.Add($name)) "ZIP entry is unique: $name"
        Require-Check ($name.StartsWith('UnrealManager-Portable/', [StringComparison]::Ordinal)) "ZIP entry stays inside portable root: $name"
        Require-Check (-not ($name.Split('/') | Where-Object { $_ -in @('.', '..') -or $_.Contains(':') })) "ZIP entry has no traversal or alternate stream: $name"
        $unixMode = ($entry.ExternalAttributes -shr 16) -band 0xF000
        Require-Check ($unixMode -ne 0xA000) "ZIP entry is not a symbolic link: $name"
        if ($name -eq 'UnrealManager-Portable/') { continue }
        $relative = $name.Substring('UnrealManager-Portable/'.Length)
        Require-Check ($allowed -ccontains $relative) "ZIP entry is an approved runtime or legal file: $relative"
        Require-Check ($entry.Length -gt 0 -and $entry.Length -lt 256MB) "ZIP entry has bounded nonempty content: $relative"
        $files[$relative] = $entry
        $totalBytes += $entry.Length
        $inventory.Add([PSCustomObject]@{ Path = $name; Bytes = $entry.Length; CompressedBytes = $entry.CompressedLength })
    }
    Require-Check ($totalBytes -lt 300MB) 'portable uncompressed size is bounded'
    foreach ($name in $required) { Require-Check ($files.ContainsKey($name)) "portable contains $name" }
    Require-Check ($files.Count -ge 6 -and $files.Count -le 7) 'portable has only approved files, no profiles, caches, tools or source tree'

    $markerReader = [IO.StreamReader]::new($files['portable.marker'].Open())
    try { $marker = $markerReader.ReadToEnd() } finally { $markerReader.Dispose() }
    Require-Check ($marker -match 'Unreal Manager portable mode') 'portable marker has expected content'
    foreach ($legalFile in @('LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD-PARTY-LICENSES.txt')) {
        Require-Check ($files[$legalFile].Length -gt 100) "$legalFile is not an empty placeholder"
    }
    if ($files.ContainsKey('RELEASE-INFO.json')) {
        $infoReader = [IO.StreamReader]::new($files['RELEASE-INFO.json'].Open())
        try { $releaseInfo = $infoReader.ReadToEnd() | ConvertFrom-Json } finally { $infoReader.Dispose() }
        $metadata['PublicReleaseInfo'] = $releaseInfo
    }
    $extractedExe = Join-Path $runDirectory 'portable-embedded-unreal-manager.exe'
    $inputStream = $files['unreal-manager.exe'].Open()
    $outputStream = [IO.File]::Create($extractedExe)
    try { $inputStream.CopyTo($outputStream) } finally { $inputStream.Dispose(); $outputStream.Dispose() }
    $metadata['PortableExecutable'] = Get-PortableExecutableInfo $extractedExe
    Require-Check ($metadata['PortableExecutable'].Machine -eq '0x8664') 'portable executable PE is AMD64'
    Require-Check ($metadata['PortableExecutable'].ProductVersion -eq $Version) 'portable executable product version matches release'
    Require-Check ($metadata['PortableExecutable'].Sha256 -eq $metadata['ExpectedExecutable'].Sha256) 'portable executable bytes equal expected fresh application'

    $metadata['Installer'] = Get-PortableExecutableInfo $installerPath
    Require-Check ($metadata['Installer'].ProductVersion -eq $Version) 'installer product version matches release'
    Require-Check ($metadata['Installer'].Length -gt 1MB) 'installer contains more than a small PE stub'
    $signature = Get-AuthenticodeSignature -LiteralPath $installerPath
    $metadata['InstallerSignatureStatus'] = [string]$signature.Status
    $metadata['InstallerSignerSubject'] = if ($null -ne $signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { $null }
    $metadata['PortableZipSha256'] = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash
} catch {
    $errorMessage = $_.Exception.Message
} finally {
    if ($null -ne $archive) { $archive.Dispose() }
    $report = [ordered]@{
        CheckedAtUtc = [DateTime]::UtcNow.ToString('o'); Version = $Version
        Passed = $null -eq $errorMessage; Error = $errorMessage
        ReleaseDirectory = $releaseRoot; ExpectedExecutablePath = $expectedExe
        PortableZip = $archivePath; Installer = $installerPath
        Metadata = $metadata; Inventory = @($inventory.ToArray()); Checks = @($checks.ToArray())
        InstallerCompressedPayloadCompared = $false
        Scope = 'Static artifact validation only. ZIP executable bytes are compared to the supplied fresh executable; installer PE headers, version, size and hash are checked. Compressed installer payload is not extracted or compared. No installer or application was executed, and no registry or user profile was changed.'
    }
    $reportPath = Join-Path $runDirectory 'release-report.json'
    [IO.File]::WriteAllText($reportPath, ($report | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))
    Write-Output "Release validation report: $reportPath"
}
if ($null -ne $errorMessage) { throw "$errorMessage. See $reportPath" }
Write-Output ('PASS: {0} static release checks. Installer payload comparison is not claimed.' -f $checks.Count)
