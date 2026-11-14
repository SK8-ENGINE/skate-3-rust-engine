#!/bin/sh
# Custom Music helper for Linux and macOS (ADD-MY-MUSIC.bat on Windows): pick a music folder, pack it into a music
# bank in Documents/Skate Music (outside the mod, so sharing the mod never shares music), then rebuild the mod's
# playlist from every bank there. Run it by double-clicking (if your file manager runs scripts) or from a terminal:
#     sh add-my-music.sh            (a folder picker opens)
#     sh add-my-music.sh ~/Music    (that folder)
here=$(cd "$(dirname "$0")" && pwd)

say() { printf '\n%s\n' "$1"; }
finish() { printf '\nPress Enter to close.'; read -r _; exit "$1"; }

exe="$here/../../skate3rust"
[ -x "$exe" ] || exe="$here/../../bin/skate3rust"
if [ ! -x "$exe" ]; then
    say "Could not find the game (skate3rust) two folders up from this mod."
    say 'This mod must stay in the game'"'"'s "mods" folder. Move it back there and try again.'
    finish 1
fi

folder=$1
if [ -z "$folder" ]; then
    title='Pick the folder that holds your music (MP3, FLAC, M4A, OGG, WAV ...)'
    if [ "$(uname)" = Darwin ]; then
        folder=$(osascript -e "POSIX path of (choose folder with prompt \"$title\")" 2>/dev/null)
    elif command -v zenity >/dev/null 2>&1; then
        folder=$(zenity --file-selection --directory --title="$title" 2>/dev/null)
    elif command -v kdialog >/dev/null 2>&1; then
        folder=$(kdialog --getexistingdirectory "$HOME" --title "$title" 2>/dev/null)
    else
        printf '%s\nType or paste the folder path, then press Enter: ' "$title"
        read -r folder
    fi
fi
folder=${folder%/}
if [ -z "$folder" ]; then say 'No folder picked. Nothing was changed.'; finish 0; fi
if [ ! -d "$folder" ]; then say "That is not a folder: $folder"; finish 1; fi

docs=$(xdg-user-dir DOCUMENTS 2>/dev/null)
[ -n "$docs" ] && [ "$docs" != "$HOME" ] || docs="$HOME/Documents"
banks="$docs/Skate Music"
mkdir -p "$banks"
name=$(basename "$folder" | tr '\\/:*?"<>|' '_________')
bank="$banks/$name.bank"

say "Packing the music in: $folder"
say "Into: $bank"
say 'This can take a few minutes for a big folder. Each song is listed as it is added.'
if ! "$exe" --pack-music-folder "$folder" "$bank"; then
    say 'Packing did not work (see the lines above). Nothing else was changed.'
    finish 1
fi

mkdir -p "$here/playlists"
set --
for b in "$banks"/*.bank; do [ -f "$b" ] && set -- "$@" "$b"; done
if ! "$exe" --music-playlist "$here/playlists/my-music.json" 'My music' --shuffle "$@"; then
    say 'Making the playlist did not work (see the lines above).'
    finish 1
fi

say "Done. Your playlist has the music of $# folder(s) from $banks."
say 'Now start the game, open the mod menu and turn on Custom Music.'
finish 0
