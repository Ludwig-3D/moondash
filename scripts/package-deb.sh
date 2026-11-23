#!/bin/sh
set -eu

binary=${1:-target/release/moondash}
output=${2:-dist-artifacts}
suite=${3:-}
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml)
arch=$(dpkg --print-architecture)
stem="moondash_${version}${suite:+.$suite}_${arch}"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
install -D -m 0755 "$binary" "$stage/usr/bin/moondash"
install -D -m 0644 ui/fonts/LICENSE-MDI "$stage/usr/share/doc/moondash/LICENSE-MDI"
install -D -m 0644 LICENSE "$stage/usr/share/doc/moondash/LICENSE-MIT"
install -D -m 0644 /usr/share/common-licenses/GPL-3 "$stage/usr/share/doc/moondash/LICENSE-GPL-3"
cat > "$stage/usr/share/doc/moondash/copyright" <<'COPYRIGHT'
Moondash source code is available under the MIT License.
This binary incorporates Slint under GPL-3.0-only and is distributed under GPL-3.0-only.
Source and build instructions: https://github.com/Ludwig-3D/moondash
Copyright notices and license texts are in LICENSE-MIT and LICENSE-GPL-3.
COPYRIGHT
install -D -m 0644 images/logo.png "$stage/usr/share/pixmaps/moondash.png"
install -d "$stage/DEBIAN" "$stage/usr/share/applications" "$output"

cat > "$stage/usr/share/applications/moondash.desktop" <<'DESKTOP'
[Desktop Entry]
Name=Moondash
Comment=Klipper touch interface for Moonraker
Exec=moondash
Icon=moondash
Terminal=false
Type=Application
Categories=Utility;
DESKTOP

cat > "$stage/DEBIAN/control" <<CONTROL
Package: moondash
Version: ${version}
Architecture: ${arch}
Maintainer: Ludwig-3D
Section: utils
Priority: optional
Depends: libc6, libgcc-s1, libfontconfig1, libwayland-client0, libxkbcommon0
Recommends: fonts-dejavu-core, libvulkan1, mesa-vulkan-drivers, libegl1, libgl1-mesa-dri
Description: Native touchscreen interface for Klipper and Moonraker
CONTROL

dpkg-deb --root-owner-group --build "$stage" "$output/$stem.deb"
install -m 0755 "$binary" "$output/$stem"
