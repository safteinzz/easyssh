# easyssh

> **Canonical:** [gitlab.com/safteinzz/easyssh](https://gitlab.com/safteinzz/easyssh) · **Mirror:** [github.com/safteinzz/easyssh](https://github.com/safteinzz/easyssh)

<!-- desc:start -->
stop typing flags, make ssh easy - hosts, keys, tunnels, mounts and file copies in one CLI + TUI
<!-- desc:end -->

## Install

```bash
cargo install easyssh
essh self check   # is a newer release out?
essh self update  # install the latest
```

No cargo yet? Rust installs the same way on every distro: [rustup.rs](https://rustup.rs).

![essh: finding a host, mounting it, opening a port forward, changing a default and connecting](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/demo.gif)

## Browse and connect

Bare `essh` opens a toolbox over your `~/.ssh/config`. A dot says whether each
host's ssh port answers, `/` filters as you type, and the panel answers the rest.

![Hosts tab: the list, the up/down dots and the detail panel](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/hosts.png)

## The command, built for you

Every wizard shows the exact command it will run, live, as you type - here the
`sshfs` invocation nobody remembers.

![Mount wizard over the Hosts tab, showing the live sshfs command](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/wizard-mount.png)

## Keep the forwards you use

A port forward is a background `ssh -N` with no window and nothing to close. Here
every one you open is kept, and each row says what it actually does - `raspi:80 →
8080` - without your having to name it. `↵` turns one on and off, `e` rewrites it,
`d` stops it and deletes it once stopped, and the list is
`~/.config/easyssh/tunnels` as `-L spec host` lines you can keep in your dotfiles.
Type a name in the wizard only when you want the row to say *why* it exists.

![Tunnels tab with a forward selected and what it reaches](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/tunnels.png)

## Manage keys

Type, fingerprint, comment, whether the agent already holds it and whether it
will ask for a passphrase. `c` makes one, `y` copies it to a host.

![Keys tab showing the agent and passphrase columns](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/keys.png)

## Mount a remote folder

`sshfs` makes a remote directory a local one, and then you forget the
`fusermount -u` on the way out. `d` does it, and cleans the mountpoint up.

![Mounts tab with a mount selected, its source, access and options](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/mounts.png)

## Your defaults, not mine

Where mounts land, whether ssh ports get checked, host order, what runs a login,
the remote sftp-server path. `↵` changes one, `d` puts it back, and it saves to
`~/.config/easyssh/settings` as `key = value` lines you can keep in your dotfiles.

![Settings tab, grouped into behaviour and defaults](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/settings.png)

## Commands

The handful of things faster to type than to click.

```sh
essh                 # open the toolbox (TUI)
essh <host> [args]   # connect - any unknown word is an ssh destination
essh ls [-v]         # list hosts from ~/.ssh/config (-v adds target, key, jump)
essh cp <src> <dst>  # scp with alias:path shorthand and auto -r for dirs
```

## Notes

- Yanking needs `wl-copy`, `xclip`, `xsel` or `pbcopy` on PATH; it names the one
  it used and installs nothing.
- Where you connect is remembered in `~/.local/state/easyssh/history`, purely to
  order the list.
- A smart frontend, not a reimplementation - it never stores your keys or
  passwords; the real `ssh` tools and your agent do the work.

## Compatibility

Linux. Tunnel and mount tracking read `/proc`, and unmounting shells out to
`fusermount`, so macOS and BSD need a different implementation first.

## License

AGPL-3.0-only
