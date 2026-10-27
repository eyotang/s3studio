#!/usr/bin/env bash
set -euo pipefail
deb=$(realpath "$1")
output=$(realpath "$2")
version=$(sed -n 's/^  "version": "\([0-9][0-9.]*\)",/\1/p' package.json)
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
work=$(mktemp -d)
hash=$(sha256sum "$deb" | cut -d ' ' -f1)
sed -e "s/@VERSION@/$version/g" -e "s/@SHA256@/$hash/g" packaging/arch/PKGBUILD.in > "$work/PKGBUILD"
cp "$deb" "$work/s3-studio.deb"
cp packaging/arch/s3-studio.desktop "$work/"
cp src-tauri/icons/128x128.png "$work/icon.png"
source "$work/PKGBUILD"
pacman -Syu --noconfirm --needed "${depends[@]}"
useradd --create-home builder
chown -R builder:builder "$work"
cd "$work"
runuser -u builder -- makepkg --cleanbuild --noconfirm
package_file="$work/s3-studio-$version-1-x86_64.pkg.tar.zst"
test -s "$package_file"
pacman -U --noconfirm "$package_file"
# Check dependencies before installing test-only tools, which could mask missing libraries.
ldd /usr/bin/s3-studio | tee "$output/omarchy-dependencies.log"
if grep -q 'not found' "$output/omarchy-dependencies.log"; then exit 1; fi
cmp /usr/bin/s3-studio "$work/src/payload/usr/bin/s3-studio"
pacman -Qkk s3-studio
pacman -S --noconfirm --needed desktop-file-utils xorg-server-xvfb xorg-xauth xorg-xwininfo weston vulkan-swrast ttf-dejavu
desktop-file-validate /usr/share/applications/com.s3studio.desktop.desktop
runuser -u builder -- dbus-run-session -- xvfb-run -a bash /work/scripts/smoke-linux.sh /usr/bin/s3-studio
runuser -u builder -- dbus-run-session -- bash /work/scripts/arch/smoke-wayland.sh
cp artifacts/* "$output/"
pacman -R --noconfirm s3-studio
test ! -e /usr/bin/s3-studio
test ! -e /usr/share/applications/com.s3studio.desktop.desktop
cp "$package_file" "$output/"
printf 'Arch package installation, dependencies, X11, Wayland and uninstall passed.\n'
