#!/usr/bin/env bash
# Rebuild the WASM engine for the examples page and drop it in site/pkg/,
# which is committed to git so deploys stay a plain nginx COPY.
# Requires: wasm-pack, rustup target wasm32-unknown-unknown.
set -euo pipefail
cd "$(dirname "$0")/.."
wasm-pack build crates/plotui-wasm --target web --release --no-pack --no-typescript \
  --out-dir ../../site/pkg
rm -f site/pkg/.gitignore # wasm-pack writes a '*' .gitignore; pkg/ is committed
# Stamp what this was built from, so CI can tell a stale bundle from a fresh
# one (see site/wasm-stamp.sh) and the page can log which engine it loaded.
./site/wasm-stamp.sh > site/pkg/BUILD_STAMP
echo "build stamp: $(cat site/pkg/BUILD_STAMP)"
ls -lh site/pkg/plotui_wasm_bg.wasm
