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
# Local ad-hoc signature only. Public distribution needs Developer ID/notarization.
codesign --force --sign - "$app"
codesign --verify --strict "$app"
printf 'App pronto: %s/dist/RXS.app\n' "$PWD"
