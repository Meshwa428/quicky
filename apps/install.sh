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

# The editors' own symbolic icons (Annotate's tools); hicolor is every icon
# theme's fallback, so they show whatever theme is active.
icons="$HOME/.local/share/icons/hicolor/scalable/actions"
mkdir -p "$icons"
install -m644 icons/*.svg "$icons/"
gtk-update-icon-cache -q -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
