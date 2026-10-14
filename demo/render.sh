#!/usr/bin/env bash
# Render the README assets in a container, so a machine needs podman or docker
# and nothing else: no vhs, no ttyd, no sshfs, no font, and the same frames on
# every machine that runs it.
#
#   ./render.sh              every tape
#   ./render.sh mount        one tape
#
# Each tape stages the world it needs (`./stage.sh up` or `fresh`) in a
# container of its own, which tears the stage down once the tape is recorded.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

ENGINE="$(command -v podman || command -v docker || true)"
[ -n "$ENGINE" ] || { echo "render.sh needs podman or docker" >&2; exit 1; }
IMAGE=localhost/easyssh-render

TAPES=("$@")
[ ${#TAPES[@]} -gt 0 ] || TAPES=(shots connect tunnels mount)

(cd .. && cargo build --release)
# The Dockerfile is the whole build context: nothing in this folder is copied in.
"$ENGINE" build -q -t "$IMAGE" - < Dockerfile > /dev/null

# The container starts as root, which only raspi's sshd needs, and the tape then
# runs as you through `setpriv`: the images it writes stay yours, and the staged
# shell's prompt ends in `$`. podman maps you to yourself with keep-id, and its
# root to nobody on the host.
if [ "$(basename "$ENGINE")" = docker ]; then
  USER_ARGS=(--user 0:0)
else
  USER_ARGS=(--userns=keep-id --user 0:0)
fi

# No network: every host lives on the container's own loopback. sshfs needs
# /dev/fuse and SYS_ADMIN to mount inside it, which is all of `--privileged`
# the mount tape uses.
for t in "${TAPES[@]}"; do
  echo "── $t.tape"
  "$ENGINE" run --rm --network none --hostname raspi "${USER_ARGS[@]}" \
    --device /dev/fuse --cap-add SYS_ADMIN \
    -e RENDER_UID="$(id -u)" -e RENDER_GID="$(id -g)" \
    -v "$(cd .. && pwd):/work/easyssh:Z" -w /work/easyssh/demo \
    --entrypoint bash "$IMAGE" -c '
      set -e
      mkdir -p /run/raspi
      ssh-keygen -q -t ed25519 -N "" -C raspi -f /run/raspi/host_key
      : > /run/raspi/authorized_keys
      chown "$RENDER_UID:$RENDER_GID" /run/raspi /run/raspi/authorized_keys
      /usr/sbin/sshd
      # ssh refuses to run for a uid with no passwd entry; podman adds one, docker does not.
      getent passwd "$RENDER_UID" > /dev/null || useradd -o -u "$RENDER_UID" -M -d /tmp render
      exec setpriv --reuid="$RENDER_UID" --regid="$RENDER_GID" --clear-groups \
        env HOME=/tmp bash -c "vhs $1.tape > /dev/null; s=\$?; ./stage.sh down > /dev/null; exit \$s"
    ' render "$t"
done
echo "done: see ../readme-assets/"
