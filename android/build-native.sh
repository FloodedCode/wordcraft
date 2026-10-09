#!/usr/bin/env bash
# Build libwordcraft.so for Android ABIs into app/src/main/jniLibs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="$(cd "$(dirname "$0")" && pwd)/app/src/main/jniLibs"
mkdir -p "$OUT"
PROFILE=()
if [[ "${1:-}" == "--release" ]]; then
  PROFILE=(--release)
fi
cargo ndk -t arm64-v8a -t x86_64 -o "$OUT" build -p wordcraft-android "${PROFILE[@]}"
echo "Native libs ready under $OUT"
