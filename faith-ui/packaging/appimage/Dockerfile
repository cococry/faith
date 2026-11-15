FROM ubuntu:22.04 AS builder

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/opt/cargo \
    RUSTUP_HOME=/opt/rustup \
    PATH=/opt/cargo/bin:$PATH

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl build-essential pkg-config cmake ninja-build clang libclang-dev \
    libx11-dev libx11-xcb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
    libssl-dev libwayland-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev \
    libfreetype6-dev libharfbuzz-dev xkb-data desktop-file-utils \
    xvfb xauth libgl1-mesa-dri procps file \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 --fail --silent --show-error https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable

RUN apt-get update && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/*

RUN apt-get update && apt-get install -y --no-install-recommends glslang-tools spirv-tools \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /source
COPY . .
WORKDIR /source/faith-ui
ENV FAITH_DIST_DIR=/dist
ENTRYPOINT ["./packaging/appimage/build.sh"]
