#!/usr/bin/env bash
# Build a <10 MB AppImage that uses the system WebKitGTK instead of
# linuxdeploy's vendored copy (~70 MB). Players need libwebkit2gtk-4.1-0.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="${1:-$root/src-tauri/target/release/arnold-client-mods}"
out="${2:-$root/src-tauri/target/release/bundle/appimage/Arnold-Client-Mods-slim.AppImage}"

if [[ ! -x "$bin" ]]; then
  echo "missing binary: $bin" >&2
  exit 1
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
appdir="$workdir/AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps"

cp "$bin" "$appdir/usr/bin/arnold-client-mods"
chmod +x "$appdir/usr/bin/arnold-client-mods"

icon="$root/src-tauri/icons/128x128@2x.png"
cp "$icon" "$appdir/usr/share/icons/hicolor/256x256/apps/arnold-client-mods.png"
cp "$icon" "$appdir/arnold-client-mods.png"

cat > "$appdir/arnold-client-mods.desktop" <<'EOF'
[Desktop Entry]
Name=Arnold Client Mods
Exec=arnold-client-mods
Icon=arnold-client-mods
Type=Application
Categories=Utility;
EOF
cp "$appdir/arnold-client-mods.desktop" "$appdir/usr/share/applications/"

cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
export PATH="$HERE/usr/bin:$PATH"
exec "$HERE/usr/bin/arnold-client-mods" "$@"
EOF
chmod +x "$appdir/AppRun"

tool="$workdir/appimagetool"
url="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
curl -fsSL -o "$tool" "$url"
chmod +x "$tool"

mkdir -p "$(dirname "$out")"
# CI/cloud images often lack FUSE. Extract appimagetool instead of mounting it.
(
  cd "$workdir"
  APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --appimage-extract >/dev/null
)
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$workdir/squashfs-root/AppRun" "$appdir" "$out"
chmod +x "$out"
size=$(wc -c < "$out")
echo "slim AppImage $out ($size bytes)"
if [[ "$size" -gt 10485760 ]]; then
  echo "FAIL: slim AppImage exceeds 10 MB" >&2
  exit 1
fi
