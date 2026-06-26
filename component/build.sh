#!/usr/bin/env bash
set -euo pipefail

export CC_wasm32_wasip2=/opt/wasi-sdk/bin/wasm32-wasip2-clang
export AR_wasm32_wasip2=/opt/wasi-sdk/bin/llvm-ar
export CFLAGS_wasm32_wasip2="-Wno-everything"
export RUSTFLAGS="-L /opt/wasi-sdk/share/wasi-sysroot/lib/wasm32-wasip2"

cd /work/component
cargo build --release --target wasm32-wasip2

WASM=target/wasm32-wasip2/release/treesitter.wasm

echo "=== validate + EH-opcode scan ==="
wasm-tools validate "${WASM}"
if wasm-tools print "${WASM}" \
    | grep -qE '\b(try|try_table|catch|catch_all|throw|throw_ref|rethrow|delegate)\b'; then
    echo "FATAL: EH opcodes present (will not run on wazero)" >&2
    exit 1
fi
echo "no EH opcodes"

mkdir -p /work/src/treesitter/assets
cp "${WASM}" /work/src/treesitter/assets/treesitter.wasm
echo "staged: src/treesitter/assets/treesitter.wasm ($(wc -c < "${WASM}") bytes)"
