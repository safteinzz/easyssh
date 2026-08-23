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
for the name. The list leads with the host you used last, a dot says whether its
ssh port is answering, `/` filters as you type, and the panel beside it answers
the rest: where it connects, which key it uses, what it jumps through, and any
tunnel or mount you have open on it.

![Hosts tab listing config hosts with their user@host targets](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/hosts.png)

## The command, built for you

The whole point: you stop memorizing flags. Every wizard shows the exact command
it will run, live, as you type - here the `sshfs` invocation nobody remembers,
built out of two fields and a choice.

![Mount wizard over the Hosts tab, showing the live sshfs command it will run](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/wizard-mount.png)

## See the forwards you opened

A port forward is a background `ssh -N` with no window, no output and nothing to
close. Raw ssh gives you no way to find it again short of hunting the pid. The
Tunnels tab lists every one essh opened, checks it is still alive, and kills it
with `d`.

![Tunnels tab listing live port forwards with their pid, direction and ports](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/tunnels.png)

## Manage keys

See your keypairs at a glance - type, fingerprint, comment, whether ssh-agent
already holds it and whether it will ask for a passphrase - and generate new ones
(`c`) or copy one to a host (`y`, via `ssh-copy-id`). New keys default to
ed25519, with rsa a keypress away for older servers.

![Keys tab with a keypair list and the ed25519-vs-rsa new-key picker](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/keys.png)

## Mount a remote folder

`sshfs` puts a remote directory in your file manager like any other folder, and
then you forget the `fusermount -u` on the way out. The Mounts tab shows what is
mounted, which host and path it came from, whether it is read-only and which uid
the files show up as - and `d` unmounts it and cleans the mountpoint up.

![Mounts tab with an sshfs mount selected and its source, access and options](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/mounts.png)

## Your defaults, not mine

Six things easyssh assumes before you type anything, and all six are yours to
change: `↵` cycles or types a new value, `d` puts it back. It saves as you go to
`~/.config/easyssh/settings`, a plain `key = value` file you can edit by hand or
keep in your dotfiles.

![Settings tab with the six settings and the detail panel explaining one](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/settings.png)

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
essh self update     # reinstall the latest release from crates.io
essh self check      # is there a newer release?
```

## In the toolbox

Tabs: Hosts, Keys, Tunnels, Mounts, Settings. Vim keys and arrows both move (`j/k`, `h/l`);
`/` filters the list you are on, `?` shows the full cheat-sheet with the real
command behind every action.

- **Hosts** - `Enter` connect, `c` new, `e` edit, `d` delete, `y` yank
  `ssh <host>` to the clipboard, `m` mount (sshfs), `t`/`T` forward/expose a port
  (`ssh -L`/`-R`), `R` clear a changed host key, `r` reload and re-check ports.
- **Keys** - `c` new (`ssh-keygen`), `y` copy to a host (`ssh-copy-id`), `Y` yank
  the public key to the clipboard.
- **Tunnels** - live port-forwards you can see and `d` kill (raw ssh can't).
- **Mounts** - active sshfs mounts, `d` to unmount; the panel beside them says
  which host and remote path each one came from, read-only or not, and the uid
  the files show up as locally.
- **Settings** - `↵` changes one, `d` puts it back: where mounts land, whether
  ssh ports are checked and how long that waits, host order, what runs a login
  (`kitten ssh`, say) and the remote sftp-server path a sudo mount needs.

Adding or editing a host covers `ProxyJump` too, and paste a `user@host:port`
into the alias field and it splits itself across the rest. Port forwards are
spawned detached and verified, mountpoints are cleaned up on unmount, and a
changed host key pops a modal offering the `ssh-keygen -R` fix instead of a
cryptic failure.

## Notes

- Linux-first: tunnel and mount tracking read `/proc`.
- Yanking needs a clipboard helper on PATH (`wl-copy`, `xclip`, `xsel` or
  `pbcopy`); it names the one it used and never installs anything.
- Which hosts you connect to is remembered in `~/.local/state/easyssh/history`,
  purely to order the list. Delete it and you lose an ordering, nothing else.
- Settings live in `~/.config/easyssh/settings` as `key = value` lines. Edit it
  by hand or from the tab; delete a line to go back to its default.
- A smart frontend, not a reimplementation - it never stores your keys or
  passwords; the real `ssh` tools and your agent do the work.

## License

AGPL-3.0-only.
