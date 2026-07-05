# easyssh

Make ssh easy. One tool — `essh` — instead of memorizing `ssh`, `ssh-keygen`,
`ssh-copy-id`, `scp` and `sshfs` (and hand-editing `~/.ssh/config`).

`essh` is a **smart frontend, not a reimplementation**: it shells out to the real
tools, so ssh-agent, your keys, and your existing `~/.ssh/config` all just work.

## Install

```sh
cargo install easyssh      # installs the `essh` command
```

## The idea

The whole point is *"I never remember the commands."* So the **CLI holds only
what's faster to type than to click**; everything you'd otherwise have to look up
lives in the **TUI**, where there's nothing to memorize.

## CLI

```sh
essh                    # open the toolbox (TUI)
essh <host> [ssh args]  # connect — any unknown word is an ssh destination
essh ls [-v]            # list every host in ~/.ssh/config
essh cp <src…> <dst>    # scp with alias:path shorthand and auto -r
```

`essh ls` replaces the classic `grep '^Host' ~/.ssh/config` alias. `essh cp
notes.md raspi:~` beats `scp notes.md user@host:~` — the config alias is enough,
and directories get `-r` automatically.

## TUI (bare `essh`)

Three tabs — **Hosts**, **Keys**, **Tunnels** — plus wizards for the things
nobody remembers:

- **Hosts** — `↵` connect · `n` new host · `m` mount (sshfs) · `t` forward a
  remote port to you · `T` expose a local port on the host
- **Keys** — `n` new ed25519 key · `c` copy a key to a host (ssh-copy-id)
- **Tunnels** — live port-forwards you can actually see and `d` kill (raw ssh
  can't show you this)

Port forwards are spawned detached and tracked, so `-L port:host:port` becomes
two plain-English questions.

### Keys (vim-first, arrows work too)

- Move: `j/k` ≡ `↑↓` in the list, `h/l` ≡ `←→` switch tabs (Ctrl-chords work too)
- In a wizard, `h/j/k/l` are **typed text** — move between fields with `Ctrl-j/k`,
  `Ctrl-↑↓`, or `Tab`
- `Enter` submit · `Esc`/`Ctrl-c` cancel · `?` help · `q` quit

## Status

Early. Config listing/adding, connect, copy, key gen/copy, tunnels and sshfs
mounts work. Tunnel liveness/kill is currently Linux-only (`/proc`).

## License

AGPL-3.0-only.
