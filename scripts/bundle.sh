#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
export MACOSX_DEPLOYMENT_TARGET=13.0
cargo build --release --locked
app="dist/RXS.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" target/RXS.iconset
cp target/release/rxs "$app/Contents/MacOS/rxs"
cp packaging/Info.plist "$app/Contents/Info.plist"
target/release/rxs --write-icon target/icon-1024.png
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" target/icon-1024.png --out "target/RXS.iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" target/icon-1024.png --out "target/RXS.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns target/RXS.iconset -o "$app/Contents/Resources/RXS.icns"
plutil -lint "$app/Contents/Info.plist"
# A Developer ID identity gives the app a stable identity across updates, which
# macOS needs in order to preserve privacy permissions such as Screen Recording.
# Keep ad-hoc signing available for local builds only.
if [[ -n "${RXS_CODESIGN_IDENTITY:-}" ]]; then
    codesign --force --options runtime --timestamp --sign "$RXS_CODESIGN_IDENTITY" "$app"
else
    codesign --force --sign - "$app"
    printf 'Aviso: assinatura ad-hoc; permissões do macOS não persistem após recompilar ou atualizar.\n' >&2
fi
codesign --verify --strict "$app"
printf 'App pronto: %s/dist/RXS.app\n' "$PWD"
