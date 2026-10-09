# Build libwordcraft.so for Android ABIs into app/src/main/jniLibs.
# Requires: rustup Android targets, cargo-ndk, ANDROID_NDK_HOME (or Android Studio NDK).
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if (-not $Root) { $Root = (Resolve-Path "$PSScriptRoot\..").Path }
Set-Location $Root

$out = Join-Path $PSScriptRoot "app\src\main\jniLibs"
New-Item -ItemType Directory -Force -Path $out | Out-Null

$release = $args -contains "--release"
$profile = if ($release) { "--release" } else { "" }

$label = if ($release) { "release" } else { "debug" }
Write-Host "Building wordcraft-android -> $out ($label)"
$cargoArgs = @(
    "ndk",
    "-t", "arm64-v8a",
    "-t", "x86_64",
    "-o", $out,
    "build",
    "-p", "wordcraft-android"
)
if ($release) { $cargoArgs += "--release" }

& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "Native libs ready under $out"
