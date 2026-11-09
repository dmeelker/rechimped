#!/usr/bin/env pwsh
# Builds the release binary for the current OS and zips it with README.md as
# dist/rechimped-<version>-<os>-<arch>.zip. Run on each OS (CI does both).
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

$version = (Select-String -Path "$root/rechimped/Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$os = if ($IsWindows) { 'windows' } else { 'linux' }
$arch = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'aarch64' } else { 'x86_64' }
$exe = if ($IsWindows) { 'rechimped.exe' } else { 'rechimped' }
$name = "rechimped-$version-$os-$arch"

cargo build --release -p rechimped --manifest-path "$root/Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$dist = Join-Path $root 'dist'
$stage = Join-Path $dist $name
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "$root/target/release/$exe" $stage
Copy-Item "$root/README.md" $stage

$zip = Join-Path $dist "$name.zip"
Remove-Item $zip -Force -ErrorAction SilentlyContinue
if ($IsWindows) {
    Compress-Archive -Path $stage -DestinationPath $zip
} else {
    # `zip` keeps the executable bit, Compress-Archive doesn't.
    Push-Location $dist
    zip -r "$name.zip" $name
    Pop-Location
}
Remove-Item $stage -Recurse -Force
Write-Host "Created $zip"
