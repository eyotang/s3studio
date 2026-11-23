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
if [[ ${S3_STUDIO_REQUIRE_MACOS_SIGNATURE:-false} == true ]]; then
    : "${S3_STUDIO_MACOS_TEAM_ID:?Official macOS releases require an Apple Developer ID team}"
fi
if [[ ${S3_STUDIO_REQUIRE_MACOS_SIGNATURE:-false} == true || -n ${S3_STUDIO_MACOS_SIGNING_P12:-} ]]; then
    bash packaging/macos/sign.sh "$app" "$S3_STUDIO_ID" "${S3_STUDIO_MACOS_CERTIFICATE_SHA256:?}"
    # Independently reject an unsigned or incorrectly signed app before making a DMG.
    # Do not rely solely on a child shell's exit status (notably Bash 3 on macOS).
    codesign --verify --strict --all-architectures "$app"
    if [[ ${S3_STUDIO_REQUIRE_MACOS_SIGNATURE:-false} == true ]]; then
        requirement="identifier \"$S3_STUDIO_ID\" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"$S3_STUDIO_MACOS_TEAM_ID\""
        codesign --verify --strict --all-architectures -R "=$requirement" "$app"
        for arch in arm64 x86_64; do
            details=$(codesign -d --arch "$arch" --verbose=4 "$app" 2>&1)
            printf '%s\n' "$details" | grep -Fx "TeamIdentifier=$S3_STUDIO_MACOS_TEAM_ID"
        done
    fi
else
    codesign --force --sign - "$app"
    codesign --verify --strict --all-architectures "$app"
fi
ln -s /Applications "$work/Applications"
payload_kib=$(du -sk "$work" | awk '{print $1}')
volume_kib=$((payload_kib + 64 * 1024))
hdiutil create -ov -format UDZO -fs 'HFS+' -size "${volume_kib}k" -volname "$name" -srcfolder "$work" \
    "$output/S3-Studio_${version}_macOS_universal.dmg"
