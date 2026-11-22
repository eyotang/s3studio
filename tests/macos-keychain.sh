#!/usr/bin/env bash
set -euo pipefail
fixture=$(mktemp -d "${TMPDIR:-/tmp}/s3-studio-keychain-test.XXXXXX")
keychain="$fixture/fixture.keychain-db"
cleanup() {
    if [[ -f $keychain ]]; then
        security delete-keychain "$keychain" >/dev/null 2>&1 || true
    fi
    rm -rf -- "$fixture"
}
trap cleanup EXIT

cat > "$fixture/openssl.cnf" <<'EOF'
[req]
distinguished_name = subject
prompt = no
x509_extensions = code_signing
[subject]
CN = S3 Studio Keychain Test
[code_signing]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
openssl req -new -x509 -newkey rsa:2048 -nodes -sha256 -days 2 -config "$fixture/openssl.cnf" \
    -keyout "$fixture/signing-key.pem" -out "$fixture/certificate.pem" 2> "$fixture/openssl.log"
S3_STUDIO_MACOS_SIGNING_PASSWORD=$(openssl rand -hex 32)
export S3_STUDIO_MACOS_SIGNING_PASSWORD
openssl pkcs12 -export -inkey "$fixture/signing-key.pem" -in "$fixture/certificate.pem" \
    -passout env:S3_STUDIO_MACOS_SIGNING_PASSWORD \
    -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1 -out "$fixture/signing.p12"
S3_STUDIO_MACOS_SIGNING_P12=$(base64 < "$fixture/signing.p12" | tr -d '\n')
export S3_STUDIO_MACOS_SIGNING_P12
fingerprint=$(openssl x509 -in "$fixture/certificate.pem" -outform DER | shasum -a 256 | awk '{print $1}')

bundle="$fixture/Keychain Probe.app"
identifier=com.s3studio.desktop.keychain-test
mkdir -p "$bundle/Contents/MacOS"
executable="$bundle/Contents/MacOS/probe"
password=$(openssl rand -hex 32)
for build in 1 2; do
    cat > "$bundle/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>$identifier</string>
<key>CFBundleExecutable</key><string>probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>$build</string>
</dict></plist>
EOF
    xcrun clang -Wno-deprecated-declarations -D "PROBE_BUILD=$build" tests/macos-keychain-probe.c \
        -framework Security -framework CoreFoundation -o "$fixture/probe-$build"
    mv -f "$fixture/probe-$build" "$executable"
    bash packaging/macos/sign.sh "$bundle" "$identifier" "$fingerprint"
    codesign -d -r- "$bundle" 2>&1 | sed -n 's/^designated => //p' > "$fixture/requirement-$build"
    test -s "$fixture/requirement-$build"
    "$executable" "$keychain" "$password"
done
cmp "$fixture/requirement-1" "$fixture/requirement-2"
printf '%s\n' 'Build 2 read the credential created by build 1 without user interaction.'
