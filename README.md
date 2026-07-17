# easyssh

**Make ssh easy.** One command - `essh` - for connecting, keys, tunnels, mounts
and copies. It shells out to the real ssh tools, so your config, agent and keys
just work.

```sh
cargo install easyssh          # installs `essh`

essh                 # open the toolbox (TUI)
essh raspi           # connect - any unknown word is an ssh destination
essh ls              # list hosts from ~/.ssh/config
essh cp file raspi:~ # scp, but the alias is enough (auto -r for dirs)
```

## The toolbox (bare `essh`)

Tabs for **Hosts**, **Keys** and **Tunnels**, with wizards for the commands
nobody remembers:

- **Hosts** - `n` add · `m` mount (sshfs) · `t`/`T` forward/expose a port
- **Keys** - `n` new ed25519 · `c` copy to a host (ssh-copy-id)
- **Tunnels** - see live port-forwards and `d` kill them

Vim keys (`hjkl`, arrows too); inside a wizard, fields move with `Ctrl-j/k`.

Run `essh --help` for the rest.

## License

AGPL-3.0-only
