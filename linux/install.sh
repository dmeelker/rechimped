#!/bin/sh
# Installs Rechimped for the current user: run from the unpacked tarball scripts/package-linux.ps1 makes.
# Pass --uninstall to remove it again.
set -eu
cd "$(dirname "$0")"

data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="$HOME/.local/bin/rechimped"
icon="$data/icons/hicolor/256x256/apps/rechimped.png"
desktop="$data/applications/rechimped.desktop"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin" "$icon" "$desktop"
else
    install -Dm755 rechimped "$bin"
    install -Dm644 rechimped.png "$icon"
    # Absolute Exec path: ~/.local/bin is not on every desktop session's PATH.
    mkdir -p "$(dirname "$desktop")"
    sed "s|^Exec=.*|Exec=$bin|" rechimped.desktop > "$desktop"
fi

# Refresh the caches where the tools exist; desktops pick the changes up regardless after a re-login.
command -v update-desktop-database >/dev/null && update-desktop-database "$data/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$data/icons/hicolor" || true

case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) echo "Note: $HOME/.local/bin is not on PATH; the menu entry works, but typing 'rechimped' in a shell won't." ;;
esac
