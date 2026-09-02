#!/usr/bin/env bash
# Print a hash of every source the committed site/pkg/ bundle is built from.
#
# site/pkg/ is a build artifact checked into git so deploys stay a plain
# nginx COPY. That is convenient and silently dangerous: change the engine,
# forget to run build-wasm.sh, and the website keeps serving the old one
# with no error anywhere. build-wasm.sh writes this hash to
# site/pkg/BUILD_STAMP and CI recomputes it, so a stale bundle fails the
# build instead of shipping.
#
# A hash of the *sources* rather than a diff of the output, because wasm
# builds are not byte-reproducible across toolchain versions.
set -euo pipefail
cd "$(dirname "$0")/.."
# The crates plotui-wasm actually compiles in.
find crates/plotui-core/src crates/plotui-bind/src crates/plotui-wasm/src \
     -type f -name '*.rs' -print0 \
  | sort -z \
  | xargs -0 shasum -a 256 \
  | shasum -a 256 \
  | cut -d' ' -f1
