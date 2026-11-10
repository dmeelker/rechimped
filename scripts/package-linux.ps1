# Builds the Linux (x86_64, glibc) release binary in Docker and packages it with the icon, desktop file,
# README and install.sh as dist\rechimped-<version>-linux-x86_64.tar.gz (unpack, then run ./install.sh).
# Needs Docker Desktop running. The cargo registry and the Linux target dir live in Docker volumes,
# so rebuilds are incremental and the Windows target dir is left alone.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = (Select-String -Path (Join-Path $root 'rechimped\Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "rechimped-$version-linux-x86_64"

docker build -t rechimped-linux-build (Join-Path $root 'linux')
if ($LASTEXITCODE) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force (Join-Path $root 'dist') | Out-Null
# Packed inside the container, so the tarball carries Unix file modes.
docker run --rm `
    -v "${root}:/src" `
    -v rechimped-cargo-registry:/usr/local/cargo/registry `
    -v rechimped-linux-target:/target `
    -e CARGO_TARGET_DIR=/target `
    -e NAME=$name `
    rechimped-linux-build `
    sh -euc '
        cargo build --release --locked -p rechimped
        stage=/tmp/$NAME
        rm -rf "$stage" && mkdir -p "$stage"
        install -m755 /target/release/rechimped /src/linux/install.sh "$stage"
        install -m644 /src/rechimped/assets/rechimped.png /src/linux/rechimped.desktop /src/README.md "$stage"
        tar -czf "/src/dist/$NAME.tar.gz" --owner=0 --group=0 -C /tmp "$NAME"
    '
if ($LASTEXITCODE) { exit $LASTEXITCODE }
"Wrote $(Join-Path $root "dist\$name.tar.gz")"
