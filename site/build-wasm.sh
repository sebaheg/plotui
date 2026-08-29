#!/usr/bin/env bash
# Rebuild the WASM engine for the examples page and drop it in site/pkg/,
# which is committed to git so deploys stay a plain nginx COPY.
# Requires: wasm-pack, rustup target wasm32-unknown-unknown.
set -euo pipefail
cd "$(dirname "$0")/.."
wasm-pack build crates/plotui-wasm --target web --release --no-pack --no-typescript \
  --out-dir ../../site/pkg
rm -f site/pkg/.gitignore # wasm-pack writes a '*' .gitignore; pkg/ is committed
ls -lh site/pkg/plotui_wasm_bg.wasm
