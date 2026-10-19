param([switch]$PackagingOnly)
$ErrorActionPreference = 'Stop'
$umProjectRoot = Split-Path $PSScriptRoot -Parent
$umProjectsRoot = Split-Path $umProjectRoot -Parent
$umSharedRoot = if ((Split-Path $umProjectsRoot -Leaf) -eq 'Projects') { Split-Path $umProjectsRoot -Parent } else { $umProjectRoot }
$umToolRoots = @()
if ($env:UM_TOOLS_ROOT) { $umToolRoots += $env:UM_TOOLS_ROOT }
$umToolRoots += @((Join-Path $umProjectRoot 'tools'), (Join-Path $umSharedRoot 'Tools'))
$umToolRoots = @($umToolRoots | Select-Object -Unique)

$umNodeFound = $false
$umRustFound = $false
foreach ($umToolRoot in $umToolRoots) {
    if (-not (Test-Path -LiteralPath $umToolRoot -PathType Container)) { continue }
    $umNode = if (-not $umNodeFound) { Get-ChildItem -LiteralPath $umToolRoot -Directory -Filter 'node-*-win-x64' |
        Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'npm.cmd') } |
        Sort-Object { [version]($_.Name -replace '^node-v|-win-x64$', '') } -Descending |
        Select-Object -First 1 }
    if ($umNode) { $env:Path = $umNode.FullName + ';' + $env:Path; $umNodeFound = $true }
    if ($umRustFound) { continue }
    foreach ($umRustRoot in @((Join-Path $umToolRoot 'Rust'), $umToolRoot)) {
        if (Test-Path -LiteralPath (Join-Path $umRustRoot 'cargo/bin/cargo.exe')) {
            $env:CARGO_HOME = Join-Path $umRustRoot 'cargo'
            $env:RUSTUP_HOME = Join-Path $umRustRoot 'rustup'
            $env:Path = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:Path
            $umRustFound = $true
            break
        }
    }
}

# Scope caches and temporary files to this process, leaving machine/user settings alone.
if (-not $env:npm_config_cache) { $env:npm_config_cache = Join-Path $umSharedRoot 'Cache/npm' }
$umTemp = if ($env:UM_TEMP_DIR) { $env:UM_TEMP_DIR } else { Join-Path $umSharedRoot 'Temp' }
New-Item -ItemType Directory -Path $umTemp -Force | Out-Null
$env:TEMP = $umTemp
$env:TMP = $umTemp
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $PSScriptRoot 'src-tauri/target' }
if (-not [IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
    $env:CARGO_TARGET_DIR = Join-Path $PSScriptRoot $env:CARGO_TARGET_DIR
}
$env:CARGO_TARGET_DIR = [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)

if (-not $PackagingOnly) { foreach ($umCommand in @('node', 'npm.cmd', 'cargo')) {
    if (-not (Get-Command $umCommand -ErrorAction SilentlyContinue)) {
        throw "$umCommand is missing. Install the development tools or set UM_TOOLS_ROOT."
    }
} }
