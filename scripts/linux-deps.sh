#!/bin/sh
# Installs the system libraries GPUI needs to build on Debian and Ubuntu.
set -eu
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  pkg-config libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-dev libx11-xcb-dev libxcb1-dev libvulkan-dev libasound2-dev libzstd-dev
