# AGENTS.md

## Hard rules
- **Commit only when the user says ship.** Commits go in once the changes are tested, which is normally right when they're about to ship; never as a mid-work checkpoint.
- **Release order, exactly:** `cargo clippy` clean + `cargo test` green -> bump `version` in Cargo.toml -> `cargo build` (so Cargo.lock's version matches) -> one commit -> `git push origin main` -> `cargo publish` (dry-run first; it is irreversible) -> **tag only after publish succeeds** (`git tag -a vX.Y.Z -m "..." && git push origin vX.Y.Z`), so a failed publish never leaves a dangling tag. (`cargo publish` fails on a dirty Cargo.lock, which is why the rebuild precedes the commit.)
- `main` is protected: no force-push, no history rewrite.
- **No em-dashes** anywhere (code, comments, README, --help, crate description, commits); they read as AI-generated. Use `-`.
- **Fix the root cause.** If a workaround must ship, say "workaround" out loud. Never `#[allow]` a lint away; fix or delete the code it points at.
- **Every bug fix gets a test**; throwaway checks let regressions back in.
- **Never test against the real `~/.ssh`.** `App::empty()` and the `sshcfg` `*_in(path, ...)` helpers keep tests on temp files and in-memory data.
- **Don't grow the CLI.** Only connect / `ls` / `cp` (faster to type than to click) belong there; anything you'd have to look up goes in the TUI. That split is the whole point ("I never remember the commands").

## Invariants and gotchas
- When editing or deleting a host: it only works if the alias is the sole name on its `Host` line; a shared `Host a b` block is refused, since one alias can't be rewritten in isolation. Every config write backs up first (`config.bak.<epoch>`) and rewrites only the touched block, never reformatting the file.
- When touching tunnels: they assume key/agent auth (a detached `ssh -N` can't answer a password prompt); after spawning we wait ~1s and check `/proc/<pid>`, so a dead-on-arrival tunnel returns ssh's stderr instead of leaving a phantom entry.
- When touching tunnel/mount liveness or kill: it reads `/proc` and shells `kill`/`fusermount`, so it is Linux-only; macOS/BSD need a different check first.
- When adding a TUI action: `n` = new/create in every view (lazygit convention), and verbs stay consistent across views (`d` delete/kill/unmount, `r` refresh, `c` copy, `e` edit). Destructive persistent actions (delete host) get a yes/no confirm gate; trivially redone ones (kill, unmount) act at once.
- TUI keys: `j/k` and arrows move, `h/l`/arrows/Tab switch view, Ctrl-chords also navigate. Inside a wizard `h/j/k/l` are typed text, so fields move with `Ctrl-j/k`, `Ctrl-arrows`, or `Tab`.
- Interactive children (connect, keygen, copy-id, mount) run suspended: leave the alt-screen, run so they can prompt for passwords, then restore. Tunnels are detached and skip this.
- When testing the TUI: driving it through `script`/pty is unreliable (the alt-screen isn't captured); assert rendering with ratatui `TestBackend`, not a pty.

## Build / test
- `cargo build` / `cargo build --release` (binary at `target/release/essh`).
- `cargo clippy` - keep warning-clean.
- `cargo test` - `sshcfg` parser/surgery tests on temp files, plus `TestBackend` render and keymap tests in `tui.rs`.
- `cargo install --path .` installs the `essh` command.

## Overview
`easyssh` is a Rust CLI + TUI that makes ssh simple: one binary `essh` in place of `ssh` + `ssh-keygen` + `ssh-copy-id` + `scp` + `sshfs` and hand-editing `~/.ssh/config`. It is a smart frontend, not a reimplementation: every action shells out to the real tools, so ssh-agent, keys, and existing config just work. Bare `essh` opens a ratatui toolbox (Hosts / Keys / Tunnels / Mounts tabs with wizards); the CLI keeps connect, `ls`, `cp`, and `update` (self-update via `cargo install easyssh --force`). Crate `easyssh`, binary `essh`, repo `git@gitlab.com:safteinzz/easyssh.git`, on crates.io, AGPL-3.0-only.

## Self-repair
If anything here contradicts the code, the code wins; fix AGENTS.md in the same session you notice the drift.
