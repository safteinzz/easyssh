# CLAUDE.md

## Overview
`easyssh` is a Rust CLI + TUI that makes ssh simple: one tool instead of
`ssh` + `ssh-keygen` + `ssh-copy-id` + `scp` + `sshfs`. Crate name `easyssh`,
binary name `essh` (ssh + one letter — muscle memory, no dangerous typo of real
`ssh`). License AGPL-3.0-only.

We are a **smart frontend, not a reimplementation**: everything shells out to the
real system tools, so ssh-agent, keys, and `~/.ssh/config` all "just work."

## Philosophy (why the split is the way it is)
The whole point is "I never remember the commands." So the **CLI holds only what
is faster to type than to click** (connect, ls, cp). Everything you'd otherwise
have to *look up* — new keys, copying keys, port forwards, mounts, adding a host
— lives in the **TUI**, where there is nothing to memorize. Don't grow the CLI
just because; new "I forget this" features go in the TUI.

## Tech stack
- Rust 2021 edition.
- `clap` (derive). Bare `essh` → TUI; unknown first arg → ssh connect
  (`external_subcommand`).
- `ratatui` 0.29 + `crossterm` 0.28 for the TUI.
- `colored` (CLI output), `anyhow` (config/key engines), `dirs` (locate ~/.ssh
  and the state dir).

## Layout
- `src/main.rs` — clap `Cli`/`Cmd`, dispatch. No-args → `tui::run()`.
- `src/sshcfg.rs` — parse `~/.ssh/config` (+ `Include`, basic `*` glob) into
  `Host`s; `add_host` appends a block after backing the file up (never rewrites).
- `src/keys.rs` — discover `~/.ssh` keypairs (type, comment, SHA256 fingerprint
  via `ssh-keygen -lf`).
- `src/tunnels.rs` — open/list/kill port forwards. Spawns detached `ssh -N -L/-R`
  (own process group, stdio null), tracks pid+spec in a TSV state file, prunes
  dead ones by `/proc/<pid>` existence (**Linux-only** liveness/kill).
- `src/commands/{ls,cp,connect}.rs` — the thin CLI verbs.
- `src/tui.rs` — the toolbox: Hosts/Keys/Tunnels tabs + modal wizards for
  add-host, keygen, copy-id, mount, forward, expose. Interactive children run
  *suspended* (leave alt-screen → run → restore) so they can prompt.

## CLI surface
- `essh` — TUI.
- `essh <host> [ssh args…]` — connect (exec's ssh).
- `essh ls [-v]` — list config hosts (replaces the classic `grep '^Host'` alias).
- `essh cp <src…> <dst>` — scp with `alias:path` shorthand + auto `-r` for dirs.

## Build / test / lint
- Build: `cargo build` / `cargo build --release`. Run: `./target/release/essh`.
- Install the `essh` command: `cargo install --path .`.
- Lint: `cargo clippy` — keep warning-clean.
- Test: `cargo test` — headless `TestBackend` render checks in `tui.rs`.

## Conventions
- Commits: short conventional tags (`feat:`, `fix:`). **Never** add
  `Co-Authored-By` trailers.
- TUI keys (vim-first, arrows always work too): `j/k` ≡ `↑↓` move in the list;
  `h/l` ≡ `←→` ≡ `Tab`/`BackTab` switch view; the `Ctrl`-chords navigate as well.
  In a **wizard** plain `h/j/k/l` are *typed text*, so field navigation is
  `Ctrl-j/k`, `Ctrl-↑↓`, or `Tab` (the "submenu" rule). `Enter` = connect /
  next-field / submit; `Esc` or `Ctrl-c` cancels; `q`/`Ctrl-c` quits; `?` help.
- Action-key parity: `n` = new/create in **every** view (lazygit convention),
  never a per-view synonym. Keep verbs consistent across views (`d` delete,
  `r` refresh, `c` copy…) rather than picking a different letter per screen.
- Config writes are append-only + timestamped backup; never reformat the file.

## Gotchas
- Tunnel tracking reads `/proc` and kills via `kill` — Linux-first. macOS/BSD
  liveness needs a different check before it's portable.
- Background tunnels assume key/agent auth — a detached `ssh -N` can't answer a
  password prompt.
- Test infra: driving the full TUI through `script`/pty is unreliable (alt-screen
  isn't captured); verify rendering with `TestBackend`, not a pty.
