#!/usr/bin/env bash
set -euo pipefail
mkdir -p artifacts
src-tauri/target/release/s3-studio --preview > artifacts/native-startup-linux.log 2>&1 &
native_pid=$!
trap 'kill "$native_pid" 2>/dev/null || true' EXIT
for attempt in {1..30}; do
  if ! kill -0 "$native_pid" 2>/dev/null; then
    cat artifacts/native-startup-linux.log
    exit 1
  fi
  if xwininfo -root -tree | grep -F '"S3 Studio"' > artifacts/native-window-linux.txt; then
    cat artifacts/native-window-linux.txt
    exit 0
  fi
  sleep 0.5
done
cat artifacts/native-startup-linux.log
printf 'S3 Studio did not open a native window.\n' >&2
exit 1
