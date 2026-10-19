param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\BUILD-ENV.ps1" -PackagingOnly
$umVersion = (Get-Content -LiteralPath "$PSScriptRoot\package.json" -Raw | ConvertFrom-Json).version
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path (Split-Path $PSScriptRoot -Parent) "Releases\$umVersion\UnrealManager-Portable"
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath (Join-Path $OutputDirectory 'Data')) {
    throw 'This portable folder contains user data. Choose a clean output folder.'
}
$umExecutable = Join-Path $env:CARGO_TARGET_DIR 'release\unreal-manager.exe'
if (-not (Test-Path -LiteralPath $umExecutable -PathType Leaf)) { throw "Release executable is missing: $umExecutable" }
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
Copy-Item -LiteralPath $umExecutable -Destination $OutputDirectory
Copy-Item -LiteralPath "$PSScriptRoot\installer\README.txt" -Destination $OutputDirectory
Copy-Item -LiteralPath "$PSScriptRoot\..\LICENSE","$PSScriptRoot\..\THIRD_PARTY_NOTICES.md" -Destination $OutputDirectory
Set-Content -LiteralPath (Join-Path $OutputDirectory 'portable.marker') -Value 'Unreal Manager portable mode' -Encoding utf8
Write-Host "Portable ready: $OutputDirectory"
Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $OutputDirectory 'unreal-manager.exe')
