#!/bin/sh
# Linux: puts MindRune's desktop entry and icons in the user's data folder, so the desktop
# shows the icon for the window and lists the app in its launcher.
#
#   scripts/install-desktop-entry.sh [BINARY]     install, starting BINARY
#   scripts/install-desktop-entry.sh --uninstall  take it all away again
#
# Without BINARY: the `mindrune` on the PATH, else this checkout's release build, else its
# debug build.
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
name=mindrune
entry=$data/applications/$name.desktop
icons=$data/icons/hicolor
sizes="16 32 48 64 128 256 512"

# The desktop's caches, where it keeps them; it also notices the files on its own.
refresh() {
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q "$data/applications" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f "$icons" 2>/dev/null || true
    fi
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$entry" "$icons/scalable/apps/$name.svg"
    for size in $sizes; do
        rm -f "$icons/${size}x$size/apps/$name.png"
    done
    refresh
    exit 0
fi

if [ $# -gt 0 ]; then
    binary=$1
elif command -v "$name" >/dev/null 2>&1; then
    binary=$(command -v "$name")
elif [ -x "$repo/target/release/$name" ]; then
    binary=$repo/target/release/$name
else
    binary=$repo/target/debug/$name
fi
if [ ! -f "$binary" ] || [ ! -x "$binary" ]; then
    echo "install-desktop-entry: not a program: $binary (build it first: cargo build -p $name)" >&2
    exit 1
fi
binary=$(CDPATH= cd -- "$(dirname -- "$binary")" && pwd)/$(basename -- "$binary")

for size in $sizes; do
    install -D -m 644 "$repo/assets/icon/$name-$size.png" "$icons/${size}x$size/apps/$name.png"
done
install -D -m 644 "$repo/assets/icon/$name.svg" "$icons/scalable/apps/$name.svg"

# An Exec line quotes the path, with a backslash before " ` $ and \, and writes % twice.
exec_path=$(printf '%s' "$binary" | sed -e 's/[\\"`$]/\\&/g' -e 's/%/%%/g')
mkdir -p "$data/applications"
while IFS= read -r line; do
    case $line in
        Exec=*) printf 'Exec="%s"\n' "$exec_path" ;;
        *) printf '%s\n' "$line" ;;
    esac
done <"$repo/assets/linux/$name.desktop" >"$entry"
refresh
