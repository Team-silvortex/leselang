#!/bin/sh
# Build a local demo bundle for Finder/GUI launch; no installation or signing.
set -eu
host_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$host_dir/../.."
cargo build --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean
bundle="$PWD/target/LeseLang AppKit Host.app"
mkdir -p "$bundle/Contents/MacOS"
cp target/lean/leselang-appkit-host "$bundle/Contents/MacOS/host"
cat > "$bundle/Contents/MacOS/demo" <<'LAUNCHER'
#!/bin/sh
exec "$(dirname -- "$0")/host" --demo --hold
LAUNCHER
chmod +x "$bundle/Contents/MacOS/demo"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>org.leselang.appkit-host.demo</string>
<key>CFBundleName</key><string>LeseLang AppKit Host</string>
<key>CFBundleExecutable</key><string>demo</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf 'Demo bundle: %s\n' "$bundle"
