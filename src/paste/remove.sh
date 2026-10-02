# Removes the xclip stand-in `essh` installed, and only that. Run by `sh` with
# this file on stdin; every line on stdout that starts with `essh-paste:` is
# read by essh.
say() { printf 'essh-paste: %s\n' "$*"; }

dest="${HOME:?}/.local/bin/xclip"
dir="$HOME/.cache/easyssh/paste"

if [ ! -e "$dest" ]; then
    say "absent"
elif grep -q '@MARKER@' "$dest" 2>/dev/null; then
    rm -f "$dest" || exit 5
    say "removed"
else
    say "not-ours"
fi

rm -f "$dir"/*.sock 2>/dev/null
rmdir "$dir" 2>/dev/null
exit 0
