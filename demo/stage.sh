#!/usr/bin/env bash
# A staged ~/.ssh for the README screenshots and the demo GIF: fake hosts, fake
# keys, fake history, fake tunnels. Nothing here touches your real ~/.ssh, your
# real agent or your real state dir - every path is redirected into ./home, XDG
# variables included.
#
#   ./stage.sh up     build the fixtures and start the helpers
#   ./stage.sh run    launch essh against them (this is what you screenshot)
#   ./stage.sh shell  a shell where `essh` is this build, for the CLI shots
#   ./stage.sh down   unmount anything under the stage, then delete it
#
# The shell it opens wears the same invented `user@host` prompt as every other
# crate's rig, and no tape sets a VHS theme, so every frame across the projects
# is the same terminal.
#
# Every address is from a range reserved for documentation (RFC 5737, RFC 3849)
# or loopback, every name is example.com (RFC 2606), and every key is generated
# here and thrown away. There is nothing real in it to leak.
#
# One exception, and it is opt-in: a tunnel and a mount cannot be faked, because
# both need a server that actually speaks ssh. Point the `raspi` entry at a box
# you own and those two work:
#
#   cp demo/.env.example demo/.env      # then fill it in
#
# or export ESSH_DEMO_HOST / ESSH_DEMO_USER / ESSH_DEMO_KEY / ESSH_DEMO_PORT
# yourself; the file is only a convenience, and it is gitignored.
#
# The Hosts list renders `User@HostName:Port`, so whatever you put in .env is on
# screen: ESSH_DEMO_USER and ESSH_DEMO_HOST both reach a frame, and a login shown
# in the GIF prints that machine's banner and prompt too. Keep them to a login
# and an address you are happy to publish - a LAN address and your own name are
# usually fine, a public IP is not.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# `demo/.env` if you made one. Sourced, not parsed, so it is a shell file like
# any other - and gitignored, because it names a machine of yours.
if [ -f "$HERE/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$HERE/.env"
  set +a
fi
STAGE="$HERE/home"
# Everything the rig creates lives inside the stage, so `down` takes all of it
# with one guarded delete and nothing is left in demo/ to gitignore.
PIDS="$STAGE/.pids"
BIN="$STAGE/.bin"
ESSH="$HERE/../target/release/essh"

# Written by `up`, required by `down`. See the guard further down.
MARKER=".essh-demo-stage"

# The hosts that answer get a loopback address each, so the list shows a real
# mix of up and down instead of a wall of red. Ports under 1024 need root, so
# the config carries a Port line; swap any HostName below to a 192.0.2.x address
# if you would rather have a prettier address than a green dot.
PORT=2222
UP_ADDRS=(127.0.0.2 127.0.0.3 127.0.0.4 127.0.0.5 127.0.0.6 127.0.0.7)

# `raspi` is the one entry that can be a real machine - see the header. Unset,
# it is another loopback socket like the rest, and the mount and tunnel wizards
# will report that ssh refused them.
if [ -n "${ESSH_DEMO_HOST:-}" ]; then
  RASPI_HOST="$ESSH_DEMO_HOST"
  RASPI_PORT="${ESSH_DEMO_PORT:-22}"
  # Not $USER: the alias's login shows in the Hosts list, and every frame is
  # the same for everyone. Set ESSH_DEMO_USER if yours is not `pi`.
  RASPI_USER="${ESSH_DEMO_USER:-pi}"
  # ssh expands `~` in an IdentityFile itself, but only for a value it reads
  # from a config file - which is exactly where this one ends up.
  RASPI_KEY="${ESSH_DEMO_KEY:-~/.ssh/id_ed25519}"
else
  RASPI_HOST="${UP_ADDRS[4]}"
  RASPI_PORT="$PORT"
  RASPI_USER="pi"
  RASPI_KEY="~/.ssh/id_rsa"
fi

# The complete environment anything staged runs in. Used with `env -i`, so this
# is not "the real environment plus overrides" - it is everything there is.
# Overriding is not enough: HOME alone would send the history and the tunnel
# state back to the real state dir, and no override list can be trusted to have
# thought of every variable a tool reads.
env_for_stage() {
  echo "HOME=$STAGE" \
       "XDG_CONFIG_HOME=$STAGE/.config" \
       "XDG_DATA_HOME=$STAGE/.local/share" \
       "XDG_STATE_HOME=$STAGE/.local/state" \
       "XDG_CACHE_HOME=$STAGE/.cache" \
       "SSH_AUTH_SOCK=$STAGE/agent.sock" \
       "SSH_AGENT_PID=" \
       "PATH=$BIN:/usr/local/bin:/usr/bin:/bin" \
       "TERM=${TERM:-xterm-256color}" \
       "COLORTERM=truecolor" \
       "LANG=C.UTF-8"
}

write_config() {
  mkdir -p "$STAGE/.ssh"
  cat > "$STAGE/.ssh/config" <<EOF
# ~/.ssh/config - the file essh reads, and the only place it writes hosts.

Host web01
    HostName ${UP_ADDRS[0]}
    Port $PORT
    User deploy

Host web02
    HostName ${UP_ADDRS[1]}
    Port $PORT
    User deploy

Host bastion
    HostName ${UP_ADDRS[2]}
    Port $PORT
    User admin

Host db-primary
    HostName ${UP_ADDRS[3]}
    Port $PORT
    User postgres
    ProxyJump bastion

Host raspi
    HostName $RASPI_HOST
    Port $RASPI_PORT
    User $RASPI_USER
    IdentityFile $RASPI_KEY

Host nas
    HostName ${UP_ADDRS[5]}
    Port $PORT
    User admin

Host staging
    HostName 198.51.100.24
    User deploy

Host vpn-gw
    HostName 2001:db8::1
    User admin

# One key shared by the web fleet. essh keeps this grouping: add another host
# with the same IdentityFile and the alias joins this line instead of growing a
# duplicate key block.
Host web01 web02
    IdentityFile ~/.ssh/id_fleet
EOF
  chmod 700 "$STAGE/.ssh"
  chmod 600 "$STAGE/.ssh/config"
}

write_keys() {
  # -q -N '' is an unencrypted key; id_fleet gets a passphrase so the Keys tab
  # has something to show in the passphrase column.
  ssh-keygen -q -t ed25519 -N '' -C 'you@example.com'     -f "$STAGE/.ssh/id_ed25519"
  ssh-keygen -q -t rsa -b 4096 -N '' -C 'legacy@example.com' -f "$STAGE/.ssh/id_rsa"
  ssh-keygen -q -t ed25519 -N 'demo' -C 'fleet@example.com' -f "$STAGE/.ssh/id_fleet"
}

start_agent() {
  # An agent of its own, so the screenshot shows staged keys and never yours.
  rm -f "$STAGE/agent.sock"
  eval "$(ssh-agent -a "$STAGE/agent.sock" -s)" > /dev/null
  echo "$SSH_AGENT_PID" >> "$PIDS"
  SSH_AUTH_SOCK="$STAGE/agent.sock" ssh-add -q "$STAGE/.ssh/id_ed25519"
}

start_listeners() {
  # A socket that listens and never accepts is enough for essh's probe, which
  # only asks whether the TCP handshake completes. Nothing is ever read or sent.
  # Their output goes to /dev/null: a background child holding this script's
  # stdout open makes `./stage.sh up | anything` hang forever.
  for addr in "${UP_ADDRS[@]}"; do
    python3 -c "
import socket, time
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('$addr', $PORT))
s.listen(16)
time.sleep(86400)
" > /dev/null 2>&1 &
    echo $! >> "$PIDS"
  done
  # The demo tape opens a forward on 5432 and shows ssh refusing it, so the rig
  # is what makes that true: without a listener here the beat only happens on a
  # machine that already runs postgres, which is the opposite of reproducible.
  # A bind that fails because the port really is taken is the same picture.
  python3 -c "
import socket, time
s = socket.socket()
s.bind(('127.0.0.1', 5432))
s.listen(16)
time.sleep(86400)
" > /dev/null 2>&1 &
  echo $! >> "$PIDS"
}

seed_history() {
  # `<epoch> <count> <alias>`. The ages are what the Hosts tab renders as
  # "12m ago" and what it sorts on, so pick them to tell a story.
  local state="$STAGE/.local/state/easyssh"
  mkdir -p "$state"
  local now; now=$(date +%s)
  {
    echo "$((now - 720))     41 web01"
    echo "$((now - 10800))   18 db-primary"
    echo "$((now - 172800))   7 raspi"
    echo "$((now - 518400))  23 bastion"
    echo "$((now - 2073600))  4 nas"
    echo "$((now - 5443200))  2 staging"
  } > "$state/history"
}

seed_tunnels() {
  # essh tracks tunnels in a TSV and keeps a row only while /proc/<pid> exists,
  # so a placeholder process per row is a complete, harmless stand-in.
  local state="$STAGE/.local/state/easyssh"
  mkdir -p "$state/tunnels"
  : > "$state/tunnels.tsv"
  local i=0
  while read -r kind spec host; do
    sleep 86400 > /dev/null 2>&1 &
    local pid=$!
    echo "$pid" >> "$PIDS"
    local log="$state/tunnels/stage-$i.log"
    : > "$log"
    printf '%s\t%s\t%s\t%s\t%s\n' "$pid" "$kind" "$spec" "$host" "$log" >> "$state/tunnels.tsv"
    i=$((i + 1))
  done <<'ROWS'
L 8080:localhost:80 web01
L 5432:localhost:5432 db-primary
R 9000:localhost:3000 bastion
ROWS
}

up() {
  down_quiet
  mkdir -p "$STAGE"
  # Stamp it before anything else, so a later `down` can prove this tree is ours.
  : > "$STAGE/$MARKER"
  : > "$PIDS"
  write_config
  write_keys
  # A shell rc that sources something out of the real home would error on it
  # here; an empty stand-in keeps the staged shell quiet.
  mkdir -p "$STAGE/.cargo" && : > "$STAGE/.cargo/env"
  start_agent
  start_listeners
  seed_history
  seed_tunnels
  echo "staged in $STAGE"
  echo
  echo "  ./stage.sh run    open the toolbox against it"
  echo "  ./stage.sh shell  a shell where essh is this build"
  echo "  ./stage.sh down   tear it all down"
  echo
  if [ -n "${ESSH_DEMO_HOST:-}" ]; then
    echo "raspi points at your ESSH_DEMO_HOST, so mounts and tunnels work."
  else
    echo "ESSH_DEMO_HOST is unset, so every host is fake and the mount and"
    echo "tunnel scenes will fail. See the header of this script."
  fi
}

# ---------------------------------------------------------------------------
# the teardown guard - identical in every crate's rig
# ---------------------------------------------------------------------------
# A rig is a convenience script with a recursive delete in it, run half
# attentively while thinking about something else, against a path some scenario
# may have mounted a remote filesystem onto. Both halves of that have already
# happened in this workflow: a stage path that pointed somewhere real and was
# deleted because the script trusted its own variable, and an sshfs mount inside
# a staged home torn down with `rm -rf`, which walked through the mountpoint and
# deleted the dotfiles on the machine at the far end. So the delete is proved
# rather than trusted.
refuse() { echo "REFUSING to delete $STAGE: $1" >&2; exit 1; }

assert_safe_to_delete() {
  case "$STAGE" in
    /*) ;;
    *) refuse "the stage path must be absolute" ;;
  esac
  # Resolve symlinks first: a link pointing the stage at something real must not
  # let a delete through on the strength of a harmless-looking path.
  local real
  real="$(cd "$STAGE" && pwd -P)" || refuse "cannot resolve the path"
  case "$real" in
    / | /home | /root | /usr | /etc | /var | /opt | /srv | /boot | /tmp)
      refuse "that is a system directory" ;;
  esac
  [ "$real" = "$HOME" ] && refuse "that is your home directory"
  case "$HOME/" in
    "$real"/*) refuse "your home directory is inside it" ;;
  esac
  # The real gate: only ever delete a tree this script built and stamped.
  [ -f "$real/$MARKER" ] || refuse "no \`$MARKER\` in it, so this script did not build it"
  # Unmount anything under it, longest path first, then check again: a recursive
  # delete walks straight through a mountpoint and removes the far side.
  local mp
  while read -r mp; do
    [ -n "$mp" ] || continue
    echo "unmounting $mp"
    fusermount -u "$mp" 2> /dev/null || umount "$mp" 2> /dev/null || true
  done < <(awk -v s="$real/" '$2 ~ "^"s {print length($2), $2}' /proc/mounts |
             sort -rn | cut -d' ' -f2-)
  if awk -v s="$real/" '$2 ~ "^"s {found=1} END {exit !found}' /proc/mounts; then
    refuse "something is still mounted under it; unmount it by hand and rerun"
  fi
}

down_quiet() {
  if [ -f "$PIDS" ]; then
    while read -r pid; do
      [ -n "$pid" ] && kill "$pid" 2> /dev/null || true
    done < "$PIDS"
  fi
  [ -d "$STAGE" ] || return 0
  assert_safe_to_delete
  # --one-file-system as a second net, in case the mount check was wrong.
  rm -rf --one-file-system "$STAGE"
}

# ---------------------------------------------------------------------------
# the shell in frame - identical in every crate's rig
# ---------------------------------------------------------------------------
# The prompt is invented, and deliberately not the renderer's own. Sourcing a
# real ~/.bashrc paints a different picture on every machine that regenerates
# the assets, which defeats the point of keeping the rig in the repo: these
# images are a build output, and a build output that depends on whose machine
# ran it is not reproducible. A username is not a leak, but `user@host` is the
# same for everyone, and it is the same string in all six rigs so the frames
# match. No tape sets a theme either, so every frame is VHS's default black.
write_demorc() {
  cat > "$STAGE/.demorc" <<'EOF'
PS1='\[\e[38;5;114m\]user@host\[\e[0m\]:\[\e[38;5;110m\]\w\[\e[0m\]\$ '
unset PROMPT_COMMAND
HISTFILE=
clear
EOF
}

# A shell that finds this build as `essh`, so a CLI screenshot shows the command
# you actually type rather than a path into target/release. It starts in the
# staged home, so the `~` in the prompt is the fixture and not your files.
open_shell() {
  mkdir -p "$BIN"
  ln -sf "$(cd "$(dirname "$ESSH")" && pwd)/essh" "$BIN/essh"
  write_demorc
  (cd "$STAGE" && env -i $(env_for_stage) \
    bash --noprofile --rcfile "$STAGE/.demorc" -i)
}

case "${1:-up}" in
  up)    up ;;
  # Run from inside the staged home: a mountpoint defaults to `./sshfs/<host>`
  # relative to the working directory, and running from here would put this
  # script's own path on screen.
  run)   (cd "$STAGE" && env -i $(env_for_stage) "$ESSH") ;;
  shell) open_shell ;;
  ls)    env -i $(env_for_stage) "$ESSH" ls -v ;;
  down)  down_quiet; echo "torn down" ;;
  *)     echo "usage: $0 [up|run|shell|ls|down]" >&2; exit 2 ;;
esac
