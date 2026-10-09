#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
export MACOSX_DEPLOYMENT_TARGET=13.0
python3 scripts/signing.py preflight
cargo build --release --locked
app="dist/RSX.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" target/RSX.iconset
cp target/release/rsx "$app/Contents/MacOS/rsx"
cp packaging/Info.plist "$app/Contents/Info.plist"
target/release/rsx --write-icon target/icon-1024.png
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" target/icon-1024.png --out "target/RSX.iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" target/icon-1024.png --out "target/RSX.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns target/RSX.iconset -o "$app/Contents/Resources/RSX.icns"
plutil -lint "$app/Contents/Info.plist"
python3 scripts/signing.py sign "$app"
codesign --verify --strict "$app"
printf 'App pronto: %s/dist/RSX.app\n' "$PWD"
