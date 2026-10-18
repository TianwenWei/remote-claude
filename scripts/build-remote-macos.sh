#!/bin/sh
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
if [ "$(uname -s)" != Darwin ]; then
  echo "This build script requires macOS." >&2
  exit 1
fi
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
  . "$HOME/.cargo/env"
fi
exec ./node_modules/.bin/tauri build --features remote-vps --config src-tauri/tauri.remote.conf.json --bundles app "$@"
