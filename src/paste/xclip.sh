#!/bin/sh
# essh image-paste stand-in for xclip, written by `essh`; `P` on the host's row in `essh` removes it.
#
# Claude Code reads a pasted image by running `xclip -selection clipboard -t TARGETS -o`
# and then `xclip -selection clipboard -t image/png -o`. While you are connected
# with `essh`, those two are answered from the clipboard of the machine you
# connected from, through a socket `ssh -R` forwards into $dir. Every other call,
# and these two when no connection answers, goes to the real xclip later in PATH.

dir="${HOME:-/nonexistent}/.cache/easyssh/paste"

# Sets `want` to `targets` or `png` when the arguments are exactly one of the
# two reads above (in any order, abbreviated the way xclip allows), else empty.
parse() {
    want= sel=primary target= out=
    while [ $# -gt 0 ]; do
        case "$1" in
        -se | -sel | -sele | -selec | -select | -selecti | -selectio | -selection)
            [ $# -ge 2 ] || return
            sel=$2
            shift
            ;;
        -t | -ta | -tar | -targ | -targe | -target)
            [ $# -ge 2 ] || return
            target=$2
            shift
            ;;
        -o | -ou | -out) out=1 ;;
        *) return ;;
        esac
        shift
    done
    case "$sel" in c*) ;; *) return ;; esac
    [ -n "$out" ] || return
    case "$target" in
    TARGETS) want=targets ;;
    image/png) want=png ;;
    esac
}

# Writes the answer of the socket $1 to the request $2 on stdout. Returns 2 when
# nothing listens there any more, which is a socket left by a connection that
# has ended, and 1 when there is no way to ask.
ask() {
    if command -v python3 >/dev/null 2>&1; then
        python3 -c '
import errno, socket, sys
s = socket.socket(socket.AF_UNIX)
s.settimeout(15)
try:
    s.connect(sys.argv[1])
except OSError as e:
    sys.exit(2 if e.errno in (errno.ECONNREFUSED, errno.ENOENT) else 1)
s.sendall(sys.argv[2].encode() + b"\n")
out = sys.stdout.buffer
while True:
    b = s.recv(65536)
    if not b:
        break
    out.write(b)
' "$1" "$2"
    elif command -v perl >/dev/null 2>&1; then
        perl -MIO::Socket::UNIX -e '
alarm 15;
my $s = IO::Socket::UNIX->new(Peer => $ARGV[0])
    or exit(($!{ECONNREFUSED} || $!{ENOENT}) ? 2 : 1);
print $s "$ARGV[1]\n";
binmode STDOUT;
my $b;
print $b while sysread($s, $b, 65536);
' "$1" "$2"
    elif command -v socat >/dev/null 2>&1; then
        printf '%s\n' "$2" | socat -t 15 - "UNIX-CONNECT:$1" 2>/dev/null || return 1
    elif command -v nc >/dev/null 2>&1 && nc -h 2>&1 | grep -q -- '-U'; then
        printf '%s\n' "$2" | nc -U "$1" 2>/dev/null || return 1
    else
        return 1
    fi
}

# The first xclip in PATH that is not this script. Only grep's "no match"
# counts as not ours, because a grep that fails would make this exec itself.
real() {
    old_ifs=$IFS
    IFS=:
    for d in $PATH; do
        IFS=$old_ifs
        [ -n "$d" ] || d=.
        [ -x "$d/xclip" ] || continue
        grep -q 'essh image-paste stand-in' "$d/xclip" 2>/dev/null
        if [ $? -eq 1 ]; then
            printf '%s\n' "$d/xclip"
            return 0
        fi
    done
    IFS=$old_ifs
    return 1
}

parse "$@"
if [ -n "$want" ] && [ -d "$dir" ]; then
    tmp=$(mktemp "${TMPDIR:-/tmp}/essh-paste.XXXXXX") || tmp=
    if [ -n "$tmp" ]; then
        # Newest first, because the newest connection is the one you are typing in.
        for name in $(ls -1t "$dir" 2>/dev/null); do
            case "$name" in *.sock) ;; *) continue ;; esac
            ask "$dir/$name" "$want" >"$tmp"
            case $? in
            2) rm -f "$dir/$name" ;;
            0) if [ -s "$tmp" ]; then
                cat "$tmp"
                rm -f "$tmp"
                exit 0
            fi ;;
            esac
        done
        rm -f "$tmp"
    fi
fi

# Set by the exec below, so a copy of this script found as the real xclip
# cannot hand the call back and forth forever.
if [ -z "${ESSH_XCLIP_PASSED:-}" ] && xclip=$(real); then
    ESSH_XCLIP_PASSED=1 exec "$xclip" "$@"
fi
echo "xclip: command not found" >&2
exit 127
