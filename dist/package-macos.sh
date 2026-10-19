#!/bin/sh
# Gera dist/EmuZip.app e dist/EmuZip.dmg a partir do build release.
set -e
cd "$(dirname "$0")/.."
cargo build --release
APP=dist/EmuZip.app
rm -rf "$APP" dist/EmuZip.dmg
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp dist/EmuZip.icns "$APP/Contents/Resources/EmuZip.icns"
cp target/release/EmuZip "$APP/Contents/MacOS/EmuZip"
cp target/release/emuzipctl "$APP/Contents/MacOS/emuzipctl"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>EmuZip</string>
<key>CFBundleDisplayName</key><string>EmuZip</string>
<key>CFBundleExecutable</key><string>EmuZip</string>
<key>CFBundleIdentifier</key><string>br.com.vidyacode.emuzip</string>
<key>CFBundleShortVersionString</key><string>0.3.0</string>
<key>CFBundleIconFile</key><string>EmuZip</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --deep -s - "$APP"
hdiutil create -volname EmuZip -srcfolder "$APP" -ov -format UDZO dist/EmuZip.dmg
ls -lh dist/EmuZip.dmg
