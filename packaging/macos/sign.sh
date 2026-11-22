#!/usr/bin/env bash
set -euo pipefail
bundle=$1
identifier=$2
expected_fingerprint=$3
: "${S3_STUDIO_MACOS_SIGNING_P12:?The PKCS#12 signing identity is required}"
: "${S3_STUDIO_MACOS_SIGNING_PASSWORD:?The PKCS#12 signing password is required}"

signing_dir=$(mktemp -d "${TMPDIR:-/tmp}/s3-studio-signing.XXXXXX")
keychain="$signing_dir/signing.keychain-db"
certificate="$signing_dir/certificate.der"
temporary_trust=false
remove_trust() {
    if [[ $temporary_trust == true ]]; then
        sudo -n security remove-trusted-cert -d "$certificate" || return
        temporary_trust=false
    fi
}
cleanup() {
    remove_trust || true
    if [[ -f $keychain ]]; then
        security delete-keychain "$keychain" >/dev/null 2>&1 || true
    fi
    rm -rf -- "$signing_dir"
}
trap cleanup EXIT

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

keychain_password=$(openssl rand -hex 32)
security create-keychain -p "$keychain_password" "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$p12" -k "$keychain" -P "$S3_STUDIO_MACOS_SIGNING_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
# The signing command needs a trusted identity on the build machine. Verification
# uses the pinned certificate, so remove the runner's trust entry before testing.
if [[ ${RUNNER_ENVIRONMENT:-} == github-hosted ]] &&
    ! security find-identity -v -p codesigning "$keychain" | grep -Fq "$identity"; then
    sudo -n security add-trusted-cert -d -r trustRoot -p codeSign "$certificate"
    temporary_trust=true
fi
codesign --force --sign "$identity" --keychain "$keychain" --timestamp=none \
    -r="designated => $requirement" "$bundle"

remove_trust
security delete-keychain "$keychain"
codesign --verify --strict --all-architectures -R="$requirement" "$bundle"
