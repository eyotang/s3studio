#!/usr/bin/env bash
set -euo pipefail
binary=$1
work=$2
output=$3
version=${S3_STUDIO_VERSION:?}
name="S3-Studio_$version"
identifier=${S3_STUDIO_ID:?}
test "$(uname -m)" = x86_64
test -x "$binary"

# Stage the application and desktop entry once for both Linux package formats.
deb="$work/deb"
install -Dm755 "$binary" "$deb/usr/bin/s3-studio"
install -Dm644 packaging/s3-studio.desktop "$deb/usr/share/applications/$identifier.desktop"
install -Dm644 src-nsh/icons/128x128.png "$deb/usr/share/icons/hicolor/128x128/apps/$identifier.png"
install -Dm644 src-nsh/icons/32x32.png "$deb/usr/share/icons/hicolor/32x32/apps/$identifier.png"
install -Dm644 src-nsh/icons/128x128@2x.png "$deb/usr/share/icons/hicolor/256x256/apps/$identifier.png"
mkdir "$deb/DEBIAN"
cat > "$deb/DEBIAN/control" <<EOF
Package: s3-studio
Version: $version
Section: net
Priority: optional
Architecture: amd64
Maintainer: S3 Studio
Homepage: https://github.com/eyotang/s3studio
Depends: libc6 (>= 2.35), libgcc-s1, libdbus-1-3, libfontconfig1, libvulkan1, libwayland-client0, libxcb1, libxkbcommon0, libxkbcommon-x11-0
Recommends: xdg-desktop-portal, xdg-utils
Description: Native graphical client for S3-compatible object storage
EOF
dpkg-deb --build --root-owner-group "$deb" "$output/${name}_ubuntu22.04_amd64.deb"

download_tool() {
    local repo=$1 tag=$2 asset=$3 digest=$4
    gh release download "$tag" --repo "$repo" --pattern "$asset" --dir "$work"
    printf '%s  %s\n' "$digest" "$work/$asset" | sha256sum -c
    chmod +x "$work/$asset"
}
download_tool linuxdeploy/linuxdeploy 1-alpha-20251107-1 linuxdeploy-x86_64.AppImage \
    c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
download_tool AppImage/appimagetool 1.9.1 appimagetool-x86_64.AppImage \
    ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
download_tool AppImage/type2-runtime continuous runtime-x86_64 \
    617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46

export APPIMAGE_EXTRACT_AND_RUN=1
export ARCH=x86_64
NO_STRIP=1 "$work/linuxdeploy-x86_64.AppImage" --appdir "$work/AppDir" \
    --executable "$deb/usr/bin/s3-studio" \
    --desktop-file "$deb/usr/share/applications/$identifier.desktop" \
    --icon-file "$deb/usr/share/icons/hicolor/128x128/apps/$identifier.png"
"$work/appimagetool-x86_64.AppImage" --runtime-file "$work/runtime-x86_64" \
    "$work/AppDir" "$output/${name}_linux_x86_64.AppImage"
