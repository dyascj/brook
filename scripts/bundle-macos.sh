#!/bin/sh
# Builds dist/Brook.app, then dist/Brook.zip (the first download) and
# dist/Brook-macos.app.tar.gz (what the in-app updater installs).
#
#   UNIVERSAL=1     build arm64 and x86_64 and join them (needs both targets)
#   SIGN_IDENTITY   codesign identity; ad hoc ("-") by default
set -eu
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

if [ "${UNIVERSAL:-0}" = 1 ]; then
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked -p brook --target "$target"
  done
  mkdir -p target/universal
  lipo -create -output target/universal/brook \
    target/aarch64-apple-darwin/release/brook target/x86_64-apple-darwin/release/brook
  BINARY=target/universal/brook
else
  cargo build --release --locked -p brook
  BINARY=target/release/brook
fi

APP=dist/Brook.app
rm -rf dist
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/Brook"
sed "s/__VERSION__/$VERSION/g" assets/macos/Info.plist > "$APP/Contents/Info.plist"
cp assets/macos/AppIcon.icns "$APP/Contents/Resources/"
printf 'APPL????' > "$APP/Contents/PkgInfo"
codesign --force --deep --sign "${SIGN_IDENTITY:--}" "$APP"
(cd dist && ditto -c -k --keepParent Brook.app Brook.zip && tar -czf Brook-macos.app.tar.gz Brook.app)
echo "built $APP ($VERSION)"
