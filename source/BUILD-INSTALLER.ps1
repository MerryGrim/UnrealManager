param([string]$Compiler, [switch]$SkipBuild)
$ErrorActionPreference='Stop'
Set-Location $PSScriptRoot
. "$PSScriptRoot\BUILD-ENV.ps1" -PackagingOnly:$SkipBuild
if (-not $Compiler) {
    $Compiler = $umToolRoots | ForEach-Object { Join-Path $_ 'InnoSetup7\ISCC.exe' } |
        Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $Compiler) { $Compiler = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source }
}
if (-not $Compiler -or -not (Test-Path -LiteralPath $Compiler)) { throw 'Install Inno Setup 7 and pass -Compiler with the ISCC.exe path.' }
if (-not $SkipBuild) { & "$PSScriptRoot\BUILD-WINDOWS.ps1" -Configuration release }
$umVersion=(Get-Content "$PSScriptRoot\package.json" -Raw | ConvertFrom-Json).version
$umOut=Join-Path (Split-Path $PSScriptRoot -Parent) "Releases\$umVersion"
$umPortable=Join-Path $umOut 'UnrealManager-Portable'
New-Item -ItemType Directory -Path $umPortable -Force | Out-Null
$umExecutable = Join-Path $env:CARGO_TARGET_DIR 'release\unreal-manager.exe'
& "$PSScriptRoot\PACKAGE-PORTABLE.ps1" -OutputDirectory $umPortable
$umBootstrap="$PSScriptRoot\installer\dependencies\MicrosoftEdgeWebview2Setup.exe"
if (-not (Test-Path -LiteralPath $umBootstrap -PathType Leaf)) {
    throw 'Download the official Microsoft WebView2 Evergreen Bootstrapper from https://developer.microsoft.com/microsoft-edge/webview2/ and save it as installer/dependencies/MicrosoftEdgeWebview2Setup.exe. See ../THIRD_PARTY_NOTICES.md.'
}
$umSignature=Get-AuthenticodeSignature -LiteralPath $umBootstrap
if ($umSignature.Status -ne 'Valid' -or $umSignature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') { throw 'WebView2 bootstrapper signature is invalid.' }
& $Compiler "/DAppVersion=$umVersion" "/DOutputRoot=$umOut" "/DAppExecutable=$umExecutable" "$PSScriptRoot\installer\UnrealManager.iss"
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup build failed.' }
Get-FileHash -Algorithm SHA256 -LiteralPath "$umPortable\unreal-manager.exe","$umOut\UnrealManager-$umVersion-Windows-x64-Setup.exe" | Format-Table -AutoSize
