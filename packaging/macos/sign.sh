#!/usr/bin/env bash
set -euo pipefail
bundle=$1
identifier=$2
expected_fingerprint=$3
: "${S3_STUDIO_MACOS_SIGNING_P12:?The PKCS#12 signing identity is required}"
: "${S3_STUDIO_MACOS_SIGNING_PASSWORD:?The PKCS#12 signing password is required}"

signing_dir=$(mktemp -d "${TMPDIR:-/tmp}/s3-studio-signing.XXXXXX")
certificate="$signing_dir/certificate.der"
trap 'rm -rf -- "$signing_dir"' EXIT

p12="$signing_dir/signing.p12"
printf '%s' "$S3_STUDIO_MACOS_SIGNING_P12" | base64 -D > "$p12"
openssl pkcs12 -in "$p12" -clcerts -nokeys -passin env:S3_STUDIO_MACOS_SIGNING_PASSWORD |
    openssl x509 -outform DER > "$certificate"
fingerprint=$(shasum -a 256 < "$certificate" | awk '{print $1}')
if [[ $fingerprint != "$expected_fingerprint" ]]; then
    printf '%s\n' 'The macOS release signing certificate differs from the pinned certificate.' >&2
    exit 1
fi
identity=$(shasum -a 1 < "$certificate" | awk '{print $1}' | tr '[:lower:]' '[:upper:]')
requirement="identifier \"$identifier\" and certificate leaf = H\"$identity\""

csreq -r "=$requirement" -b "$signing_dir/requirement.bin"
printf '%s' "$S3_STUDIO_MACOS_SIGNING_PASSWORD" > "$signing_dir/password"
rcodesign sign --timestamp-url none --p12-file "$p12" --p12-password-file "$signing_dir/password" \
    --code-requirements-file "$signing_dir/requirement.bin" "$bundle"
codesign --verify --strict --all-architectures -R "=$requirement" "$bundle"
