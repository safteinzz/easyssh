# easyssh

> **Canonical:** [gitlab.com/safteinzz/easyssh](https://gitlab.com/safteinzz/easyssh) · **Mirror:** [github.com/safteinzz/easyssh](https://github.com/safteinzz/easyssh)

<!-- desc:start -->
stop typing flags, make ssh easy - hosts, keys, tunnels, mounts and file copies in one CLI + TUI
<!-- desc:end -->

```sh
cargo install easyssh          # installs the `essh` command
```

## Browse and connect

Bare `essh` opens a toolbox over your `~/.ssh/config`: tabs for Hosts, Keys,
Tunnels and Mounts. Arrow to a host and press Enter to connect - no more digging
for the name.

![Hosts tab listing config hosts with their user@host targets](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/hosts.png)

## The command, built for you

The whole point: you stop memorizing flags. Every wizard shows the exact command
it will run, live, as you type - here the local port forward nobody remembers.

![Port-forward wizard showing the live ssh -N -L command it will run](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/wizard.png)

## Manage keys

See your keypairs at a glance - type, fingerprint, comment - and generate new
ones (`c`) or copy one to a host (`y`, via `ssh-copy-id`). New keys default to
ed25519, with rsa a keypress away for older servers.

![Keys tab with a keypair list and the ed25519-vs-rsa new-key picker](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/keys.png)

## Or just from the shell

The handful of things faster to type than to click stay in the CLI. `essh ls`
is your old `grep '^Host' ~/.ssh/config` alias, built in.

![essh ls -v listing hosts and where each connects](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/ls.png)

## CLI

```sh
essh                 # open the toolbox (TUI)
essh <host> [args]   # connect - any unknown word is an ssh destination
essh ls [-v]         # list hosts from ~/.ssh/config
essh cp <src> <dst>  # scp with alias:path shorthand and auto -r for dirs
essh update          # update to the latest release on crates.io
```

## In the toolbox

Tabs: Hosts, Keys, Tunnels, Mounts. Vim keys and arrows both move (`j/k`, `h/l`);
`?` shows the full cheat-sheet with the real command behind every action.

- **Hosts** - `Enter` connect, `c` new, `e` edit, `d` delete, `m` mount (sshfs),
  `t`/`T` forward/expose a port (`ssh -L`/`-R`), `R` clear a changed host key.
- **Keys** - `c` new (`ssh-keygen`), `y` copy to a host (`ssh-copy-id`).
- **Tunnels** - live port-forwards you can see and `d` kill (raw ssh can't).
- **Mounts** - active sshfs mounts, `d` to unmount.

Port forwards are spawned detached and verified, mountpoints are cleaned up on
unmount, and a changed host key pops a modal offering the `ssh-keygen -R` fix
instead of a cryptic failure.

## Notes

- Linux-first: tunnel and mount tracking read `/proc`.
- A smart frontend, not a reimplementation - it never stores your keys or
  passwords; the real `ssh` tools and your agent do the work.

## License

AGPL-3.0-only.
