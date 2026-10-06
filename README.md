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

Legend: green answered on its ssh port, red did not, `○` still checking; `◆` is
a host behind a jump, coloured by whether the jump answered.

```bash
essh                 # the toolbox, over your ~/.ssh/config
essh raspi           # straight in, like ssh
essh raspi uptime    # anything after the host goes to ssh
```

Every host in your `~/.ssh/config`, where it points, whether it answers and when
you last went in, with the rest in the panel. `c` adds a host and `e` edits one,
written into `~/.ssh/config` itself, backed up first; `ctrl-o` in the form picks
a key you already have. `Y` on the Keys tab installs it on the new machine with
`ssh-copy-id`, and from then on Enter is all it takes.

## Keep the forwards you use

![A port forward made from raspi's row, the form building the ssh -N -L line as it is typed, then running on the Tunnels tab and switched off and back on](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/tunnels.gif)

A port forward is a background `ssh -N` with no window and nothing to close.
`t` on a host opens one (`T` the other way, `-R`), and the form shows the line
it will run as you type. Each row says what it does without your naming it,
`↵` turns one off and on, `e` rewrites it, `d` deletes it.

For a web UI that one port cannot carry, such as a router that redirects to its
own address, pick `browse through the host (-D)` under Direction: it runs a
SOCKS proxy that reaches every address the host can. Point the browser, or the
system, at `socks5://localhost:<port>` (1080 unless you change it) with remote
DNS on.

The list is `~/.config/easyssh/tunnels`, one `-L spec host` line each, so it can
live in your dotfiles.

## Mount a remote folder

![The mount form on raspi showing the sshfs command it will run, the mount appearing on the Mounts tab, raspi's home listed from the shell like any folder, then unmounted and mounted again with Enter](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/mount.gif)

`m` on a host shows the `sshfs` line it will run, and the mount lands on the
Mounts tab as an ordinary folder, `~/sshfs/raspi` here. `sshfs` makes a remote directory a local one, and then you forget
the `fusermount -u` on the way out: `↵` does it and mounts it again later, and
`d` deletes it. The mountpoint is cleaned up either way.

The list is `~/.config/easyssh/mounts`, one `local <- host:path` line each.

## Paste images into Claude Code over ssh

Claude Code on a Linux box reads a pasted image with `xclip`, and over ssh
there is no clipboard for it to read, so Ctrl+V only says `No image found in
clipboard`. `P` on a host turns that around: essh puts a small stand-in `xclip`
in `~/.local/bin` there, and every `essh <host>` login after that carries this
machine's clipboard image across the connection (`ssh -R` to a unix socket).
Ctrl+V in Claude Code on the host then pastes what you copied here. Press `P`
again to switch it off and remove the stand-in.

- It needs Linux on the host, `~/.local/bin` ahead of `/usr/bin` in its `PATH`
  (essh checks a login shell and tells you if it is not), and one of `python3`,
  `perl`, `socat` or an `nc` with `-U` there to talk to the socket. Every other
  `xclip` call is passed to the real one.
- Here it reads the clipboard with `wl-paste`, `xclip` or `pngpaste`, whichever
  is installed.
- Only logins made with `essh <host>` or from the toolbox carry it, and only to
  the hosts you enabled, listed in `~/.config/easyssh/paste`.
- While you are connected, the host can read the image on your clipboard
  whenever it asks, and so can anyone who is root there; text on the clipboard
  is never sent. Enable it only on machines you trust with that.

## Manage keys

![Keys tab showing the agent and passphrase columns](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/keys.png)

Type, fingerprint, comment, whether the agent already holds it and whether it
will ask for a passphrase. `c` makes one, `y` copies it, `Y` installs it on a
host.

## Your defaults, not mine

![Settings tab, grouped into behaviour and defaults](https://gitlab.com/safteinzz/easyssh/-/raw/main/readme-assets/settings.png)

Where mounts land, whether ssh ports get checked, host order, what runs a login,
the remote sftp-server path. `↵` changes one, `d` puts it back, and it saves to
`~/.config/easyssh/settings` as `key = value` lines you can keep in your dotfiles.

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
essh completions bash --add        # turn on Tab completion (zsh and fish too)
```

Everything the tabs do has a command, so a script or an agent can drive it
without a terminal: every `ls` takes `--json`, and every command that changes
something takes `-n` to print its steps and change nothing. `essh --help` and
`essh <command> --help` have the rest.

## Keys

| key | does |
| --- | --- |
| `j` `k` / `↑` `↓` | move in the list |
| `h` `l` / `←` `→` / `tab` `shift-tab` | switch tab |
| `/` | filter the list; `↵` keeps it, `esc` drops it |
| `r` | read the tab's data again |
| `esc` | cancel a form or a question, close help or an alert |
| `?` | every key, on every tab |
| `q` / `ctrl-c` | quit; in a form or a box, `ctrl-c` is `esc` |

Each tab's own keys are on its bottom line, and `?` lists them all.

## Tab completion

Tab completes your config's aliases after `essh`, and local paths, `alias:` and the remote side's own folders after `essh cp`. Turn it on once:

```bash
essh completions bash --add    # writes the loader into ~/.bashrc
essh completions zsh  --add    # ~/.zshrc
essh completions fish --add    # ~/.config/fish/config.fish
```

Open a new shell afterwards. Without `--add` it only prints the script, so `source <(essh completions bash)` works as well.

A remote path is listed over ssh without a prompt, so it completes on hosts you reach with a key or the agent and stays quiet on the rest.

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
