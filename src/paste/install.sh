# Installs the xclip stand-in for `essh`'s image paste. Run by `sh` with this
# file on stdin, so nothing below may read stdin. Every line on stdout that
# starts with `essh-paste:` is read by essh.
say() { printf 'essh-paste: %s\n' "$*"; }

[ "$(uname -s)" = Linux ] || { say "not-linux $(uname -s)"; exit 3; }
[ -n "${HOME:-}" ] || { say "no-home"; exit 3; }

bin="$HOME/.local/bin"
dest="$bin/xclip"
dir="$HOME/.cache/easyssh/paste"

if [ -e "$dest" ] && ! grep -q '@MARKER@' "$dest" 2>/dev/null; then
    say "taken"
    exit 4
fi

mkdir -p "$bin" || exit 5
(umask 077 && mkdir -p "$dir") || exit 5
chmod 700 "$dir" || exit 5

cat >"$dest.essh-new" <<'ESSH_STANDIN_END' || exit 5
@STANDIN@
ESSH_STANDIN_END
chmod 755 "$dest.essh-new" && mv -f "$dest.essh-new" "$dest" || exit 5

say "dir $dir"

# What a fresh login shell runs for `xclip`; anything but $dest means the
# stand-in is never reached.
found=$("${SHELL:-/bin/sh}" -lc 'command -v xclip' </dev/null 2>/dev/null | tail -n 1)
say "resolves $found"
say "bin $dest"

if command -v python3 >/dev/null 2>&1; then
    say "helper python3"
elif command -v perl >/dev/null 2>&1; then
    say "helper perl"
elif command -v socat >/dev/null 2>&1; then
    say "helper socat"
elif command -v nc >/dev/null 2>&1 && nc -h 2>&1 | grep -q -- '-U'; then
    say "helper nc"
else
    say "helper none"
fi
