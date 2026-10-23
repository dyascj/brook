#!/bin/sh
# Installs Brook for the current user:
#   ~/.local/bin/brook
#   ~/.local/share/applications/io.github.dyascj.brook.desktop
#   ~/.local/share/icons/hicolor/512x512/apps/io.github.dyascj.brook.png
# Brook updates itself in place from there. Run with --uninstall to remove it
# (your library in ~/.local/share/brook stays).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin="$HOME/.local/bin"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/512x512/apps"

if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$bin/brook" "$apps/io.github.dyascj.brook.desktop" "$icons/io.github.dyascj.brook.png"
  echo "Brook is uninstalled. Your library is still in ${XDG_DATA_HOME:-$HOME/.local/share}/brook."
  exit 0
fi

mkdir -p "$bin" "$apps" "$icons"
install -m 755 "$here/brook" "$bin/brook"
install -m 644 "$here/brook.png" "$icons/io.github.dyascj.brook.png"
sed "s|^Exec=brook|Exec=$bin/brook|" "$here/io.github.dyascj.brook.desktop" > "$apps/io.github.dyascj.brook.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$apps" >/dev/null 2>&1 || true
echo "Brook is installed. Find it in your app launcher, or run $bin/brook."
