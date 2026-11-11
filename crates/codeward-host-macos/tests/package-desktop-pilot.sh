#!/bin/sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
    printf '%s\n' 'The desktop pilot package requires macOS.' >&2
    exit 1
fi
case "$(uname -m)" in
    arm64) target=aarch64-apple-darwin ;;
    x86_64) target=x86_64-apple-darwin ;;
    *) printf '%s\n' 'Unsupported macOS build architecture.' >&2; exit 1 ;;
esac

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../../.." && pwd)
cd "$root"
cargo build --release --locked --offline --target "$target" --target-dir "$root/target" \
    -p codeward-host-macos --features desktop-pilot --bin codeward-desktop-pilot

output=$(mktemp -d "$root/target/codeward-pilot.XXXXXX")
app="$output/Codeward Synthetic Pilot.app"
contents="$app/Contents"
plist="$contents/Info.plist"
config="$root/crates/codeward-host-macos/tauri.conf.json"
mkdir -p "$contents/MacOS"
install -m 755 "$root/target/$target/release/codeward-desktop-pilot" "$contents/MacOS/"
/usr/bin/plutil -create xml1 "$plist"
/usr/bin/plutil -insert CFBundleIdentifier -string "$(/usr/bin/plutil -extract identifier raw "$config")" "$plist"
/usr/bin/plutil -insert CFBundleName -string "$(/usr/bin/plutil -extract productName raw "$config")" "$plist"
/usr/bin/plutil -insert CFBundleShortVersionString -string "$(/usr/bin/plutil -extract version raw "$config")" "$plist"
/usr/bin/plutil -insert CFBundleVersion -string 1 "$plist"
/usr/bin/plutil -insert CFBundlePackageType -string APPL "$plist"
/usr/bin/plutil -insert CFBundleExecutable -string codeward-desktop-pilot "$plist"
/usr/bin/plutil -lint "$plist" >&2
/usr/bin/codesign --sign - "$app"
/usr/bin/codesign --verify --strict "$app"
printf '%s\n' "$app"
