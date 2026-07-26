#!/usr/bin/env bash
# Build the core engine for the browser and generate JS bindings into src/pkg.
#
# The browser build drops the image decoders (`--no-default-features`). The page
# decodes the file itself with the platform's own decoders, so every format the
# browser can read is supported, and the payload falls from ~393KB to ~150KB
# gzipped.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/web-ui/src/pkg"

# wasm-bindgen-cli has to match the wasm-bindgen crate exactly or it refuses to
# run, so take the version from the lockfile rather than pinning it here and
# letting the two drift.
VERSION="$(grep -A2 '^name = "wasm-bindgen"$' "$ROOT/Cargo.lock" \
  | grep '^version' | head -1 | sed 's/.*"\(.*\)".*/\1/')"

if [ -z "$VERSION" ]; then
  echo "could not read the wasm-bindgen version from Cargo.lock" >&2
  exit 1
fi

installed=""
if command -v wasm-bindgen >/dev/null 2>&1; then
  installed="$(wasm-bindgen --version | awk '{print $2}')"
fi

if [ "$installed" != "$VERSION" ]; then
  echo "installing wasm-bindgen-cli $VERSION (found '${installed:-none}')"
  cargo install wasm-bindgen-cli --version "$VERSION" --locked
fi

rustup target add wasm32-unknown-unknown

cargo build \
  --manifest-path "$ROOT/Cargo.toml" \
  -p png2svg-core \
  --target wasm32-unknown-unknown \
  --release \
  --no-default-features

rm -rf "$OUT"
wasm-bindgen \
  --target web \
  --out-dir "$OUT" \
  "$ROOT/target/wasm32-unknown-unknown/release/png2svg_core.wasm"

echo "wrote $OUT"
ls -la "$OUT"
