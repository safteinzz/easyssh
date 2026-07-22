# AGENTS.md

Working brief for an AI coding agent, not documentation for people (README covers that). Rules, invariants and gotchas needed to change this project correctly without rediscovering them.

## Hard rules
- Commit only when the user says ship. Commits go in once the changes are tested, which is normally right when they are about to ship, never as a mid-work checkpoint.
- Release order, exactly: `cargo clippy` clean and `cargo test` green, bump `version` in Cargo.toml, `cargo build` so Cargo.lock's version matches, one commit, `git push origin main`, `cargo publish` (dry-run first, publishing is irreversible), then tag only after publish succeeds with `git tag -a vX.Y.Z -m "..." && git push origin vX.Y.Z`. A tag must never point at a version that failed to publish. `cargo publish` fails on a dirty Cargo.lock, which is why the rebuild precedes the commit.
- `main` is protected: no force-push and no history rewrite, so fix mistakes with a forward commit.
- No em-dashes anywhere (code, comments, README, --help, crate description, commit messages); they read as AI-generated. Use `-`.
- Fix the root cause. If a workaround must ship, say the word "workaround" out loud. Never silence a lint with `#[allow]`; fix or delete the code it points at.
- Every bug fix gets a test, because throwaway manual checks let regressions back in. If something genuinely cannot be tested (terminal lifecycle state, for example), say so instead of skipping it silently.
- Tests never touch the real `~/.ssh`: use `App::empty()` and the `sshcfg` `*_in(path, ...)` helpers, which keep everything on temp files and in-memory data.
- Do not grow the CLI. It holds connect, `ls`, `cp` and `update` only, all things faster to type than to click. Anything a user would have to look up belongs in the TUI, which is the whole point of the tool.

## Invariants and gotchas
- When editing or deleting a host: it only works when the alias is the sole name on its `Host` line, and a shared `Host a b` block is refused because one alias cannot be rewritten in isolation. Every config write backs up first to `config.bak.<epoch>` and rewrites only the touched block, never reformatting the file.
- When touching tunnels: they assume key or agent auth, since a detached `ssh -N` cannot answer a password prompt. After spawning, the code waits about a second and checks `/proc/<pid>`, so a dead-on-arrival tunnel returns ssh's stderr instead of leaving a phantom entry.
- When touching tunnel or mount liveness and kill: it reads `/proc` and shells out to `kill` and `fusermount`, so it is Linux-only and macOS or BSD need a different check first.
- When generating an RSA key: pass `-b 4096`, because `ssh-keygen -t rsa` defaults to 3072 and RSA is only chosen for legacy hosts that will be kept for years. ed25519 has one fixed size, so it takes no `-b`.
- When labelling anything user-facing: write `meaning (real command)`, such as `mount a remote folder locally (sshfs)` or `fix "host key changed" (ssh-keygen -R)`. Command-only is unreadable to a beginner, and verb-only teaches nothing and leaves them stuck without the tool.
- When an action needs a value the app already knows, such as a config host or a key, open a picker instead of a text field, because retyping a known alias is slower and typo-prone.
- When adding a TUI action: `c` creates in every view (`n` is deliberately unbound), `y` copies or yanks a key to a host (`c` is not copy, since in vim `c` means change), `d` deletes, kills or unmounts, `r` refreshes and `e` edits. Destructive persistent actions such as deleting a host get a yes/no confirm gate, while trivially redone ones such as kill and unmount act at once.
- TUI navigation: `j/k` and the arrows move, `h/l`, the arrows and Tab switch view, and the Ctrl-chords navigate too. Inside a wizard `h/j/k/l` are typed text, so fields move with `Ctrl-j/k`, `Ctrl-arrows` or Tab.
- Status messages are transient: set them with `set_status` and they clear after `STATUS_TTL`, so a stale result never reads as the current state. The event loop polls while a message is showing and blocks otherwise, so an idle TUI costs nothing.
- The help overlay must fit inside its box, and `help_overlay_is_not_cut_off` asserts the last line renders, so grow the height in `render_help` when adding lines.
- Interactive children (connect, keygen, copy-id, mount) run suspended: leave the alt-screen, run them so they can prompt for passwords, then restore. Tunnels are detached and skip this.
- When suspending the TUI: call `show_cursor()` after leaving the alt-screen, because the draw loop hides the cursor and leaving the alt-screen does not restore it, so the child would otherwise run with an invisible cursor.
- When testing the TUI: driving it through `script` or a pty is unreliable because the alt-screen is not captured, so assert rendering with ratatui's `TestBackend` instead.

## Build / test
- `cargo build` and `cargo build --release`, binary at `target/release/essh`.
- `cargo clippy`, kept warning-clean.
- `cargo test`, covering `sshcfg` parser and block-surgery tests on temp files plus `TestBackend` render and keymap tests in `tui.rs`.
- `cargo install --path .` installs the `essh` command.

## Overview
`easyssh` is a Rust CLI and TUI that makes ssh simple: one binary `essh` in place of `ssh`, `ssh-keygen`, `ssh-copy-id`, `scp` and `sshfs`, plus hand-editing `~/.ssh/config`. It is a smart frontend rather than a reimplementation, so every action shells out to the real tools and ssh-agent, keys and existing config keep working. Bare `essh` opens a ratatui toolbox with Hosts, Keys, Tunnels and Mounts tabs and their wizards, while the CLI keeps connect, `ls`, `cp` and `update`. Crate `easyssh`, binary `essh`, repo `git@gitlab.com:safteinzz/easyssh.git`, published on crates.io under AGPL-3.0-only.

## Self-repair
If anything here contradicts the code, the code wins; fix AGENTS.md in the same session you notice the drift.
