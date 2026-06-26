# syntax=docker/dockerfile:1
FROM debian:bookworm-slim

ARG WASI_SDK=wasi-sdk-33.0-arm64-linux
ARG WASM_TOOLS=wasm-tools-1.252.0-aarch64-linux
ARG RUST_VERSION=1.96.0

RUN <<EOF
set -eux
apt-get update
apt-get install -y --no-install-recommends curl ca-certificates xz-utils make git build-essential
rm -rf /var/lib/apt/lists/*
EOF

RUN <<EOF
set -eux
curl -fsSL "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-33/${WASI_SDK}.tar.gz" | tar xz -C /opt
ln -s "/opt/${WASI_SDK}" /opt/wasi-sdk
curl -fsSL "https://github.com/bytecodealliance/wasm-tools/releases/download/v1.252.0/${WASM_TOOLS}.tar.gz" | tar xz -C /opt
ln -s "/opt/${WASM_TOOLS}/wasm-tools" /usr/local/bin/wasm-tools
EOF

ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo
ENV PATH="/opt/cargo/bin:/opt/wasi-sdk/bin:${PATH}"

RUN <<EOF
set -eux
curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}"
rustup target add wasm32-wasip2
EOF

WORKDIR /work
