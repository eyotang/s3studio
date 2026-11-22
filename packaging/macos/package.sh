#!/usr/bin/env bash
set -euo pipefail
binary=$1
work=$2
output=$3
version=${S3_STUDIO_VERSION:?}
name=${S3_STUDIO_NAME:?}
app="$work/${name}.app"

lipo "$binary" -verify_arch arm64 x86_64
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m755 "$binary" "$app/Contents/MacOS/s3-studio"
install -m644 src-nsh/icons/icon.icns "$app/Contents/Resources/s3-studio.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>${S3_STUDIO_ID:?}</string>
    <key>CFBundleName</key><string>$name</string>
    <key>CFBundleDisplayName</key><string>$name</string>
    <key>CFBundleExecutable</key><string>s3-studio</string>
    <key>CFBundleIconFile</key><string>s3-studio.icns</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>${S3_STUDIO_MACOS_MINIMUM:?}</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist"
if [[ ${S3_STUDIO_REQUIRE_MACOS_SIGNATURE:-false} == true || -n ${S3_STUDIO_MACOS_SIGNING_P12:-} ]]; then
    bash packaging/macos/sign.sh "$app" "$S3_STUDIO_ID" "${S3_STUDIO_MACOS_CERTIFICATE_SHA256:?}"
else
    codesign --force --sign - "$app"
    codesign --verify --strict --all-architectures "$app"
fi
ln -s /Applications "$work/Applications"
hdiutil create -ov -format UDZO -volname "$name" -srcfolder "$work" \
    "$output/S3-Studio_${version}_macOS_universal.dmg"
