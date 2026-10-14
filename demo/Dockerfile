# The tape renderer: vhs's own image, which already carries ttyd, chromium,
# ffmpeg and the JetBrainsMono NF font every tape sets, plus what the stage runs
# inside the frame. render.sh builds it and runs every tape in it, so rendering
# needs podman or docker and nothing else on the machine.
FROM ghcr.io/charmbracelet/vhs:v0.12.1@sha256:3afed459e6d7860f6bb8b9a22ae29d5927d628dac6e30f32a250113406a348b9

# Debian as it was at this moment, from snapshot.debian.org, so every package and
# every library it pulls in is the same on any machine on any day. The normal
# mirror keeps only the current version, which would make a pinned version fail
# to install after the next point release. Move the date to update.
ARG SNAPSHOT=20261006T000000Z

# The ssh client, keygen, agent and ssh-copy-id essh drives; sshfs (with fuse3's
# `fusermount`) for the mount; python3 for the stage's listeners; and the sshd
# that answers as `raspi`.
RUN rm -f /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list \
 && for suite in trixie trixie-updates; do \
      echo "deb http://snapshot.debian.org/archive/debian/$SNAPSHOT $suite main"; \
    done > /etc/apt/sources.list \
 && echo "deb http://snapshot.debian.org/archive/debian-security/$SNAPSHOT trixie-security main" \
      >> /etc/apt/sources.list \
 && apt-get -o Acquire::Check-Valid-Until=false update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      openssh-client openssh-server sshfs python3 \
 && rm -rf /var/lib/apt/lists/*

# `raspi`: an invented user and a home with something in it for the mount to
# show. render.sh starts its sshd as root before a tape, on the address and port
# stage.sh gives raspi, with a host key made for that run in /run/raspi. The
# stage, which runs as you, writes the key it may log in with beside it, which
# is why StrictModes is off. Passwords are on so `stage.sh fresh` can show the
# first login; with a key installed the key is tried first.
RUN useradd -M -d /home/pi -s /bin/bash pi && echo 'pi:raspberry' | chpasswd \
 && mkdir -p /home/pi/.ssh /home/pi/backups /home/pi/photos /home/pi/scripts /run/sshd \
 && printf 'grocery run saturday\n' > /home/pi/notes.md \
 && printf 'services:\n  pihole:\n    image: pihole/pihole\n' > /home/pi/docker-compose.yml \
 && chown -R pi:pi /home/pi && chmod 700 /home/pi/.ssh \
 && : > /etc/motd \
 && printf '%s\n' 'ListenAddress 127.0.0.6:2222' 'HostKey /run/raspi/host_key' \
      'PermitRootLogin no' 'UsePAM no' 'PasswordAuthentication yes' \
      'KbdInteractiveAuthentication no' 'StrictModes no' \
      'AuthorizedKeysFile .ssh/authorized_keys /run/raspi/authorized_keys' \
      'AllowTcpForwarding yes' 'Subsystem sftp internal-sftp' \
      'PrintMotd no' 'PrintLastLog no' > /etc/ssh/sshd_config
