# easyssh (`essh`)

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

## Browse and connect

![The host list, a new machine added as raspi with the form, its key installed with ssh-copy-id and the password typed that once, then a login on the key alone](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/connect.gif)

Legend: green answered on its ssh port · red did not · `○` still checking · `◆` behind a jump

```bash
essh                 # the toolbox, over your ~/.ssh/config
essh raspi           # straight in, like ssh
essh raspi uptime    # anything after the host goes to ssh
```

`c` adds a host and `e` edits one, written into `~/.ssh/config` itself after a backup. `Y` on the Keys tab installs your key on it with `ssh-copy-id`, and from then on Enter is all it takes.

## Keep the forwards you use

![A port forward made from raspi's row, the form building the ssh -N -L line as it is typed, then running on the Tunnels tab and switched off and back on](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/tunnels.gif)

`t` on a host opens a forward in the background (`T` the other way), and the form shows the `ssh -N` line as you type. For a web UI one port cannot carry, pick `-D` under Direction to get a SOCKS proxy through the host.

## Mount a remote folder

![The mount form on raspi showing the sshfs command it will run, the mount appearing on the Mounts tab, raspi's home listed from the shell like any folder, then unmounted and mounted again with Enter](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/mount.gif)

`m` on a host shows the `sshfs` line it will run, and the folder shows up like any other, `~/sshfs/raspi` here. `↵` unmounts it and mounts it again later, so there is no `fusermount -u` to forget.

## Share your clipboard image over ssh

`P` on a host makes every `essh <host>` login carry this machine's clipboard image there, so a program on the host that reads the clipboard gets it, until you press `P` again. The host and its root can read that image while you are connected, so enable it only where you trust them.

## Manage keys

![Keys tab showing the agent and passphrase columns](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/keys.png)

Whether the agent holds a key and whether it asks for a passphrase, at a glance. `c` makes one, `y` copies it, `Y` installs it on a host.

## Your defaults, not mine

![Settings tab, grouped into behaviour and defaults](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/settings.png)

Where mounts land, host order, what runs a login and more. `↵` changes one and `d` puts it back.

## Commands

```bash
essh ls [-v]                       # list hosts from ~/.ssh/config (-v adds target, key, jump)
essh cp <src> <dst>                # scp with alias:path shorthand and auto -r for dirs
essh host add box --hostname 198.51.100.7 --user deploy   # add a host (edit, rm)
essh tunnel add -D 1080 raspi --name router               # keep a forward and start it
essh tunnel off router             # stop it and keep its line (on, rm, ls)
essh mount add raspi:~/notes       # keep a mount and mount it (off, on, rm, ls)
essh key new laptop                # ssh-keygen with safe defaults (ls, install)
essh check raspi                   # can this machine log in without typing, and why not
essh -- mount                      # connect to a host whose alias is also a command
```

`essh <command> --help` has each command's details, and `?` in the toolbox lists every key.

Everything the tabs do has a command, and nothing asks but ssh's own tools, so a script or an agent can drive it: every `ls` takes `--json`, `-n` prints the steps and changes nothing, and a failure exits 1.

## Tab completion

Tab completes your config's aliases after `essh`, and local paths, `alias:` and the remote side's own folders after `essh cp`. Turn it on once:

```bash
essh completions bash --add    # writes the loader into ~/.bashrc
essh completions zsh  --add    # ~/.zshrc
essh completions fish --add    # ~/.config/fish/config.fish
```

Open a new shell afterwards. Without `--add` it only prints the script, so `source <(essh completions bash)` works as well.

A remote path is listed over ssh without a prompt, so it completes on hosts you reach with a key or the agent and stays quiet on the rest.

## Where it keeps things

```
~/.ssh/config                     your hosts, edited in place after a backup
~/.config/easyssh/tunnels         one `-L spec host` line per forward
~/.config/easyssh/mounts          one `local <- host:path` line per mount
~/.config/easyssh/paste           the hosts that get your clipboard image
~/.config/easyssh/settings        `key = value` lines
~/.local/state/easyssh/history    where you connected, to order the list
```

Everything under `~/.config/easyssh` can live in your dotfiles.

## Notes

- It never stores your keys or passwords: the real `ssh` tools and your agent do the work.
- Yanking needs `wl-copy`, `xclip`, `xsel` or `pbcopy` on PATH; it names the one it used and installs nothing.
- Pasting images needs `wl-paste`, `xclip` or `pngpaste` here, and on the host Linux, `~/.local/bin` ahead of `/usr/bin` in its `PATH`, and `python3`, `perl`, `socat` or an `nc` with `-U`.

## Compatibility

Linux. Tunnel and mount tracking read `/proc`, and unmounting shells out to
`fusermount`, so macOS and BSD need a different implementation first.

## License

AGPL-3.0-only
