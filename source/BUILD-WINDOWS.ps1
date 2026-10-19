param([ValidateSet('both','debug','release')][string]$Configuration='both', [switch]$SkipDependencies)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
. "$PSScriptRoot\BUILD-ENV.ps1"
if (-not $SkipDependencies) {
    & npm.cmd ci
    if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed' }
}
$umModes = if ($Configuration -eq 'both') { @('debug','release') } else { @($Configuration) }
foreach ($umMode in $umModes) {
    $umArgs = @('run','tauri','--','build','--no-bundle')
    if ($umMode -eq 'debug') { $umArgs += '--debug' }
    $umArgs += @('--','--locked')
    & npm.cmd @umArgs
    if ($LASTEXITCODE -ne 0) { throw "$umMode build failed" }
}
Write-Host "Build complete: $env:CARGO_TARGET_DIR"

