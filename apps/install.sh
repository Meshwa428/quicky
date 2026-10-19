#!/usr/bin/env bash
# Builds the Quicky editor apps and installs them for the current user:
# binaries in ~/.local/bin, launcher entries in ~/.local/share/applications.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release

bin="$HOME/.local/bin" apps="$HOME/.local/share/applications"
mkdir -p "$bin" "$apps"
for exe in target/release/quicky-*; do
  [[ -x $exe && ! -d $exe ]] || continue
  install -m755 "$exe" "$bin/"
  echo "installed $(basename "$exe")"
done
install -m644 desktop/*.desktop "$apps/"
update-desktop-database "$apps" 2>/dev/null || true
