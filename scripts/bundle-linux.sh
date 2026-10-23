#!/bin/sh
# Builds dist/Brook-linux-<arch>.tar.gz: a brook/ folder with the binary,
# its desktop entry and icon, and install.sh. The in-app updater installs
# the same archive.
set -eu
cd "$(dirname "$0")/.."
ARCH=$(uname -m)
cargo build --release --locked -p brook

rm -rf dist
mkdir -p dist/brook
cp target/release/brook dist/brook/brook
cp assets/linux/brook.png assets/linux/io.github.dyascj.brook.desktop assets/linux/install.sh LICENSE dist/brook/
tar -czf "dist/Brook-linux-$ARCH.tar.gz" -C dist brook
echo "built dist/Brook-linux-$ARCH.tar.gz"
