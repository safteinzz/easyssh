#!/usr/bin/env bash
# A staged ~/.ssh for the README pictures: fake hosts, fake
# keys, fake history, saved tunnels. Nothing here touches your real ~/.ssh, your
# real agent or your real state dir - every path is redirected into ./home, XDG
# variables included.
#
#   ./stage.sh up     build the fixtures and start the helpers
#   ./stage.sh fresh  the same, but raspi is a new machine: not in the config,
#                     never visited, and taking a password until it has a key
#   ./stage.sh run    launch essh against them (this is what you screenshot)
#   ./stage.sh shell  a shell where `essh` is this build, for the CLI shots
#   ./stage.sh down   unmount anything under the stage, then delete it
#
# The shell it opens wears the same invented `user@host` prompt as every other
# crate's rig, and every tape sets the same VHS theme and font, so every frame
# across the projects is the same terminal.
#
# Every address is from a range reserved for documentation (RFC 5737, RFC 3849)
# or loopback, every name is example.com (RFC 2606), and every key is generated
# here and thrown away. There is nothing real in it to leak.
#
# A login, a tunnel and a mount need a server that really speaks ssh, so `raspi`
# is one: a throwaway sshd in a podman container on 127.0.0.6, with an invented
# user, hostname and home, removed on `down`. The first `up` builds its image,
# which needs the network once.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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

# `raspi` is the container, on its own loopback address, so no listener below
# takes that socket.
RASPI_HOST="${UP_ADDRS[4]}"
CENGINE="$(command -v podman || true)"
CONTAINER="easyssh-demo-sshd"
IMAGE="localhost/easyssh-demo-sshd"

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
  local raspi=""
  [ -n "${FRESH:-}" ] || raspi="Host raspi
    HostName $RASPI_HOST
    Port $PORT
    User pi
    IdentityFile ~/.ssh/id_ed25519
"
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

${raspi}
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
    [ "$addr" = "$RASPI_HOST" ] && continue
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
}

# ssh takes its config, its known_hosts and every `~` from the passwd home,
# never from $HOME, so without this every `ssh raspi` essh starts would read the
# renderer's own ~/.ssh. The wrapper hands ssh a copy of the staged config with
# `~/` spelled out as the stage, made fresh each time so a host essh just added
# is in it. It sits first on the staged PATH, where essh and sshfs both find it,
# and `exec` leaves ssh's own argv for the tunnel check to read.
write_ssh_wrapper() {
  mkdir -p "$BIN" "$STAGE/.sshd"
  cat > "$BIN/ssh" <<EOF
#!/bin/sh
cfg="$STAGE/.sshd/ssh_config.\$\$"
sed "s|~/|$STAGE/|g" "$STAGE/.ssh/config" > "\$cfg"
exec /usr/bin/ssh -F "\$cfg" -o UserKnownHostsFile="$STAGE/.ssh/known_hosts" -o GlobalKnownHostsFile=/dev/null "\$@"
EOF
  chmod +x "$BIN/ssh"
}

# The sshd behind `raspi`: pubkey only, forwarding and sftp on, and a home with
# something in it for the mount to show. Its host key is made here, so the
# staged known_hosts vouches for it and no host-key prompt reaches a frame.
# `fresh` starts it like a new machine instead: no key installed, and the
# password `raspberry` accepted until `ssh-copy-id` puts one there.
start_sshd() {
  [ -n "$CENGINE" ] || { echo "no podman: raspi will not answer, so login, tunnel and mount fail" >&2; return 0; }
  local ctx="$STAGE/.sshd"
  mkdir -p "$ctx"
  cat > "$ctx/Containerfile" <<'IMG'
FROM docker.io/library/alpine:3.20
RUN apk add --no-cache openssh-server bash \
 && adduser -D -s /bin/bash pi && echo 'pi:raspberry' | chpasswd \
 && mkdir -p /home/pi/.ssh /home/pi/backups /home/pi/photos /home/pi/scripts \
 && printf 'grocery run saturday\n' > /home/pi/notes.md \
 && printf 'services:\n  pihole:\n    image: pihole/pihole\n' > /home/pi/docker-compose.yml \
 && chown -R pi:pi /home/pi && chmod 700 /home/pi/.ssh \
 && printf '%s\n' 'PS1="\u@\h:\w\$ "' > /etc/profile.d/prompt.sh && : > /etc/motd \
 && printf '%s\n' 'Port 22' 'HostKey /etc/ssh/ssh_host_ed25519_key' 'PermitRootLogin no' \
      'PasswordAuthentication no' 'KbdInteractiveAuthentication no' \
      'AuthorizedKeysFile .ssh/authorized_keys' 'AllowTcpForwarding yes' \
      'Subsystem sftp internal-sftp' 'PrintMotd no' > /etc/ssh/sshd_config
CMD ["/usr/sbin/sshd", "-D", "-e"]
IMG
  "$CENGINE" build -q -t "$IMAGE" "$ctx" > /dev/null
  ssh-keygen -q -t ed25519 -N '' -C raspi -f "$ctx/host_key"
  echo "[$RASPI_HOST]:$PORT $(cut -d' ' -f1,2 "$ctx/host_key.pub")" > "$STAGE/.ssh/known_hosts"
  "$CENGINE" rm -f "$CONTAINER" > /dev/null 2>&1 || true
  local keys=(-v "$STAGE/.ssh/id_ed25519.pub:/home/pi/.ssh/authorized_keys:ro")
  local sshd=()
  if [ -n "${FRESH:-}" ]; then
    keys=()
    sshd=(/usr/sbin/sshd -D -e -o PasswordAuthentication=yes)
  fi
  "$CENGINE" run -d --name "$CONTAINER" --hostname raspi \
    -p "$RASPI_HOST:$PORT:22" \
    -v "$ctx/host_key:/etc/ssh/ssh_host_ed25519_key:ro" \
    "${keys[@]}" "$IMAGE" "${sshd[@]}" > /dev/null
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
    [ -n "${FRESH:-}" ] || echo "$((now - 172800))   7 raspi"
    echo "$((now - 518400))  23 bastion"
    echo "$((now - 2073600))  4 nas"
    echo "$((now - 5443200))  2 staging"
  } > "$state/history"
}

seed_tunnels() {
  # A saved forward is a line in a config file, not a process, so the list is
  # real without anything running: three named ones, all off.
  local cfg="$STAGE/.config/easyssh"
  mkdir -p "$cfg"
  cat > "$cfg/tunnels" <<'ROWS'
# easyssh tunnels - the forwards you keep, one `-L spec host` line each, and
# ` = a label` after it where you named one. -L reaches a remote service from
# here, -R exposes a local one over there. The Tunnels tab writes this file;
# `enter` turns a line on and off.

-L 8080:localhost:80 web01 = pihole
-L 5432:localhost:5432 db-primary
-R 9000:localhost:3000 bastion = laptop web
ROWS
}

# essh lists every sshfs mount on the machine, not just the staged ones, so a
# real one would be in frame and a tape's Enter would unmount it. That happened
# once, to a real mount.
assert_no_real_mounts() {
  local real
  real="$(awk -v s="$STAGE/" '$3 == "fuse.sshfs" && index($2, s) != 1 {print $2}' /proc/mounts)"
  if [ -n "$real" ]; then
    echo "stage.sh: an sshfs mount outside the stage is up, and essh would show and touch it:" >&2
    echo "$real" | sed 's/^/  /' >&2
    echo "unmount it (\`fusermount -u <dir>\`) and rerun" >&2
    exit 1
  fi
}

up() {
  down_quiet
  assert_no_real_mounts
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
  write_ssh_wrapper
  start_sshd
  seed_history
  seed_tunnels
  echo "staged in $STAGE"
  echo
  echo "  ./stage.sh run    open the toolbox against it"
  echo "  ./stage.sh shell  a shell where essh is this build"
  echo "  ./stage.sh down   tear it all down"
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
  [ -n "$CENGINE" ] && "$CENGINE" rm -f "$CONTAINER" > /dev/null 2>&1 || true
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
# match. Every tape also sets the same theme and font (Catppuccin Mocha,
# JetBrainsMono NF).
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
  fresh) FRESH=1 up ;;
  # Run from inside the staged home, so a relative path typed into a form
  # resolves in the fixture rather than beside this script.
  run)   assert_no_real_mounts; (cd "$STAGE" && env -i $(env_for_stage) "$ESSH") ;;
  shell) assert_no_real_mounts; open_shell ;;
  ls)    env -i $(env_for_stage) "$ESSH" ls -v ;;
  down)  down_quiet; echo "torn down" ;;
  *)     echo "usage: $0 [up|fresh|run|shell|ls|down]" >&2; exit 2 ;;
esac
