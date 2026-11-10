# Build environment for the Linux binary; see scripts/package-linux.ps1.
FROM rust:1.99-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        libfontconfig-dev \
        libfreetype-dev \
        libxkbcommon-dev \
        libxkbcommon-x11-dev \
        libwayland-dev \
        libx11-dev \
        libx11-xcb-dev \
        libxcb1-dev \
        libvulkan-dev \
        libasound2-dev \
        libzstd-dev \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add clippy

WORKDIR /src
