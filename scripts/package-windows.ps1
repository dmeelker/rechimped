# Builds the Windows release binary and packages it as dist\rechimped-<version>-windows-x86_64.zip.
# Close a running Rechimped first: a running exe blocks the linker.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = (Select-String -Path (Join-Path $root 'rechimped\Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "rechimped-$version-windows-x86_64"

cargo build --release --locked -p rechimped --manifest-path (Join-Path $root 'Cargo.toml')
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$dist = Join-Path $root 'dist'
$stage = Join-Path $dist $name
Remove-Item -Recurse -Force $stage, "$stage.zip" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
# The icon is embedded in the exe, so the exe, the readme and the license are all there is.
Copy-Item (Join-Path $root 'target\release\rechimped.exe'), (Join-Path $root 'UNLICENSE') $stage
Copy-Item (Join-Path $root 'docs\release-readme.txt') (Join-Path $stage 'README.txt')
Compress-Archive -Path $stage -DestinationPath "$stage.zip"
Remove-Item -Recurse -Force $stage
"Wrote $stage.zip"
