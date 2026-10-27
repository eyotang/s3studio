#!/usr/bin/env bash
set -euo pipefail
# Run from the repository root. Both paths must be inside this checkout.
deb=$(realpath "${1:?Usage: build-omarchy.sh INPUT.deb OUTPUT_DIRECTORY}")
mkdir -p "${2:?Missing output directory}"
output=$(realpath "$2")
root=$(pwd -P)
[[ "$deb" == "$root/"* && "$output" == "$root/"* ]] || { echo 'Paths must be inside the checkout' >&2; exit 1; }
docker run --rm --platform linux/amd64 \
    -v "$root:/work" -w /work \
    archlinux:base-devel \
    bash scripts/arch/build-in-container.sh "${deb#"$root/"}" "${output#"$root/"}"
