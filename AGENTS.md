<!--
AI-ONLY DOCUMENT. This file exists to give an AI agent the COMPLETE operating picture for this repo. Optimize for completeness and precision for the agent, not for human readability. Humans read README.md instead. Do not remove detail to make this nicer, err toward more explicit, not less. FORMAT: machine-read, not a formatted human doc. Do NOT hard-wrap lines to a column width for readability; put each rule/point on ONE line, however long.
-->
# AGENTS.md

Working brief for an AI coding agent, not documentation for people (the README covers that): the rules, invariants and gotchas needed to change this project correctly without rediscovering them.

## Hard rules
- Commit, push, and publish only when the user says to ship; a mid-work commit is never the deliverable, because the user tests interactively first.
- Commit messages are short single-line conventional ones (`feat:`, `fix:`, `chore:`, ...), never with a `Co-Authored-By` trailer and never with a verbose body.
- Release flow, in this exact order: ask whether this shipment gets tests and write them only if the user says yes -> bump `version` in `Cargo.toml` -> `cargo fmt --check` clean, `cargo clippy-all` clean and `cargo test` green, which is also what refreshes `Cargo.lock` with the new version -> `cargo +1.88 msrv` clean, the only thing that proves the `rust-version` floor in `Cargo.toml` is real -> one commit -> `git push origin main` -> `cargo publish` (dry-run first, publishing is irreversible) -> tag only after publish succeeds with `git tag vX.Y.Z && git push origin --tags`; a tag must never point at a version that failed to publish, and the bump comes first because `cargo publish` fails on a `Cargo.lock` that still holds the old version.
- Tests are proposed at ship time and never before: the first step of the release flow is to ask the user, in plain words, whether this shipment gets tests, and they are written only on a yes, so the decision is always theirs but the question is never forgotten.
- Never write a test for behaviour that has not shipped yet, because code that is not in the last release tag is still being designed, and a test pinning a shape that is about to change is how a suite starts lying.
- A test may only assert something the README or `--help` promises, or a pure-logic invariant (parsing, generation, path resolution, validation); never the shape of a private function and never the specific diff that was just made, since those rot on the next refactor and teach nothing about whether the program works.
- Removing a promise from the README removes its tests in the same commit.
- A test may only write inside a temp directory it deletes, never a real config, data, cache or content directory and never a fixed path, so a machine is left exactly as it was before the suite ran.
- Never drive the interface to test it: build it, say what changed and what to look at, and let the user run it, because they see the screen instantly while an agent driving a pty or a tmux pane is slow and wrong about what it looks like; logic that is not visual can still be checked directly from `tests/`.
- Never `cargo install` to test: run the release binary at `./target/release/essh` directly, because installing replaces the binary on PATH with a work-in-progress build; install only when the user asks.
- `main` is protected: no force-push and no history rewrite, so a mistake is fixed with a forward commit.
- No em-dashes anywhere (code, comments, README, `--help`, crate description, commit messages, prose), because they read as AI-generated text; use `-` instead.
- Fix the root cause, and if a workaround must ship say the word "workaround" out loud so a silent patch never passes as a real fix; the same goes for lints, where an `#[allow]` is never the answer and the code it points at gets fixed or deleted.
- `TODO-LIST.md` (gitignored) holds one-line ideas, and the line is deleted when the idea ships.
- Tests never touch the real `~/.ssh`: use `App::empty()` and the `sshcfg` `*_in(path, ...)` helpers, which keep everything on temp files and in-memory data.
- Do not grow the CLI. It holds connect, `ls`, `cp` and `update` only, all things faster to type than to click. Anything a user would have to look up belongs in the TUI, which is the whole point of the tool.

## Invariants and gotchas
- When adding a host with an IdentityFile: if exactly one shared block (a `Host a b c` line with 2+ concrete aliases) already sets that same key, append the new alias to that block and give the new host a block without its own IdentityFile; otherwise write a self-contained block. Never join a one-alias block (that is a normal per-host block, not a group) and never touch a block when zero or several match, so the tool only ever extends a group the user already made shared.
- When listing hosts: one alias may be defined across several `Host` blocks (a per-host block for HostName/User plus a shared `Host a b c` block adding a common IdentityFile); `merge_hosts` collapses them into a single entry with ssh's first-value-wins per field, so never emit one entry per block.
- When editing or deleting a host: it only works when the alias is the sole name on its `Host` line, and a shared `Host a b` block is refused because one alias cannot be rewritten in isolation. Every config write backs up first to `config.bak.<epoch>` and rewrites only the touched block, never reformatting the file.
- When touching tunnels: they assume key or agent auth, since a detached `ssh -N` cannot answer a password prompt. After spawning, the code waits about a second and checks `/proc/<pid>`, so a dead-on-arrival tunnel returns ssh's stderr instead of leaving a phantom entry.
- When touching tunnel or mount liveness and kill: it reads `/proc` and shells out to `kill` and `fusermount`, so it is Linux-only and macOS or BSD need a different check first.
- When generating an RSA key: pass `-b 4096`, because `ssh-keygen -t rsa` defaults to 3072 and RSA is only chosen for legacy hosts that will be kept for years. ed25519 has one fixed size, so it takes no `-b`.
- When labelling anything user-facing: write `meaning (real command)`, such as `mount a remote folder locally (sshfs)` or `fix "host key changed" (ssh-keygen -R)`. Command-only is unreadable to a beginner, and verb-only teaches nothing and leaves them stuck without the tool.
- When an action needs a value the app already knows, such as a config host or a key, open a picker instead of a text field, because retyping a known alias is slower and typo-prone. The add/edit-host IdentityFile field is typed OR picked: `Ctrl-o` on it opens a picker of `~/.ssh` keys (via `PickerAction::FillField`) while a typed path still works for anything custom.
- When mounting: refuse with an install hint if `sshfs` is not on PATH, default the mountpoint to `./sshfs/<host>` so leftovers stay grouped, and expand a typed `~` with `sshcfg::expand_tilde` (children are spawned without a shell, so `~/dir` would otherwise become a directory literally named `~`), and `remove_dir` the mountpoint after a successful unmount. `remove_dir` only removes an empty directory, so a mount placed over a dir with real content is never deleted.
- When adding a TUI action: `c` creates in every view (`n` is deliberately unbound), `y` copies or yanks a key to a host (`c` is not copy, since in vim `c` means change), `d` deletes, kills or unmounts, `r` refreshes and `e` edits. Destructive persistent actions such as deleting a host get a yes/no confirm gate, while trivially redone ones such as kill and unmount act at once.
- TUI navigation: `j/k` and the arrows move, `h/l`, the arrows and Tab switch view, and the Ctrl-chords navigate too. Inside a wizard `h/j/k/l` are typed text, so fields move with `Ctrl-j/k`, `Ctrl-arrows` or Tab. A `Kind::Choice` field is the exception: it holds no text, so `h/l` and the arrows (with or without Ctrl) cycle its answer and you leave it with Tab, Enter or the vertical keys.
- Wizard fields are `Kind::Text` or `Kind::Choice` (cycled in place), and `shown_when` hides one until another field's choice selects it. Hidden fields keep their slot so field indices stay stable; `visible`, `step` and `on_last_field` are what keep navigation and Enter from landing on them.
- A wizard box sizes to its *wrapped* rows, not its line count: a sudo mount's preview is wider than the box, and counting lines clipped the key hints off. `centered` takes a width *percentage*, so measure against `area.width * PROMPT_PCT / 100 - 2`, and note `wrapped_line_count` counts real columns (leading spaces included), so do not switch it back to `split_whitespace`.
- When mounting as root: sshfs's channel has no tty, so `sudo` cannot prompt, and the wizard offers exactly one sudo mode - `-o sftp_server=sudo <path>`, which needs a NOPASSWD sudoers line scoped to the sftp server. Do not add a "send the password" mode: every way of feeding one from here (piping it to `sudo -S`, priming the ppid ticket) puts the password in a command line, where `ps` shows it to every user on both machines. Hiding it would take owning the SSH client, not a flag.
- Status messages are transient: set them with `set_status` and they clear after `STATUS_TTL`, so a stale result never reads as the current state. The event loop polls while a message is showing and blocks otherwise, so an idle TUI costs nothing.
- Failures that the user must act on get a modal (`Confirm` with a title and an offered fix), not a status that flashes past: a busy unmount offers `fusermount -u -z`, and a connect that exits 255 is probed with `changed_host_key` and, if the key changed, offers `ssh-keygen -R`. The probe runs only after a failed connect, so the success path costs nothing.
- Wizards show a live `command_preview` rebuilt from the current fields, so the user sees the exact `ssh-keygen`/`sshfs`/`ssh -L` command as they type. Keep the preview's value resolution identical to `submit_prompt`, or the preview lies.
- Modal boxes size to their wrapped content (`wrapped_line_count`) and inset text with block `Padding`, so a long message is never cut off and wrapped lines never butt the border. Height must count the borders (2) on top of padding and the button row, or the answer buttons get clipped and the modal looks unanswerable; `confirm_buttons_are_not_cut_off` asserts they render.
- A `Confirm` shows selectable `Yes`/`No` buttons and starts on No, so a reflex Enter never fires a destructive action. `y`/`n` still answer directly, `h/l`, the arrows and Tab move, Esc cancels, and any other key is ignored so a stray press cannot dismiss the question.
- The help overlay must fit inside its box, and `help_overlay_is_not_cut_off` asserts the last line renders, so grow the height in `render_help` when adding lines.
- Interactive children (connect, keygen, copy-id, mount) run suspended: leave the alt-screen, run them so they can prompt for passwords, then restore. Tunnels are detached and skip this.
- When suspending the TUI: call `show_cursor()` after leaving the alt-screen, because the draw loop hides the cursor and leaving the alt-screen does not restore it, so the child would otherwise run with an invisible cursor.
- When testing the TUI: driving it through `script` or a pty is unreliable because the alt-screen is not captured, so assert rendering with ratatui's `TestBackend` instead.

## Build / lint / test
- `cargo build --release`, binary at `target/release/essh`.
- `cargo clippy-all` is the lint pass, aliased in `.cargo/config.toml` to `clippy --release --all-targets -- -D warnings`; use it rather than a bare `cargo clippy`, which skips `tests/` and `examples/` and only warns where the release flow wants a failure.
- `cargo test`.
- `cargo fmt` formats the crate and `cargo fmt --check` fails when anything has drifted; the whole crate is rustfmt clean, so formatting is never a judgement call and never a review comment.
- `cargo +1.88 msrv` checks the crate against the `rust-version` floor it advertises (alias in `.cargo/config.toml`), and when the code starts needing a newer compiler both that floor and the toolchain in this line move together.
- Tests cover the `sshcfg` parser and its block surgery on temp files, plus `TestBackend` render and keymap tests in `tui/tests.rs`; when something genuinely cannot be tested (terminal lifecycle state, for example), say so rather than skipping it silently.

## Overview
Layout:
- `src/main.rs` - the clap `Cmd` enum and the dispatch match, nothing else.
- `src/commands/<verb>.rs` - one file per CLI command (`connect`, `ls`, `cp`, `selfcmd`), each exposing `run`.
- `src/tui/` - the toolbox: `mod.rs` owns `App`, the event loop and the terminal handling, `input.rs` dispatches keys per view, `wizard.rs` is what a submitted prompt does, `prompt.rs` the field machinery, `picker.rs` and `confirm.rs` their overlays, `mount_spec.rs` the sshfs command a mount wizard builds, `render.rs` the frame, `widgets.rs` the domain-blind furniture, `connect.rs` reading ssh's failure output, `tests.rs` the `TestBackend` tests.
- Domain modules at the top level: `sshcfg` (parsing and editing `~/.ssh/config`), `keys`, `tunnels`, `mounts`.


`easyssh` is a Rust CLI and TUI that makes ssh simple: one binary `essh` in place of `ssh`, `ssh-keygen`, `ssh-copy-id`, `scp` and `sshfs`, plus hand-editing `~/.ssh/config`. It is a smart frontend rather than a reimplementation, so every action shells out to the real tools and ssh-agent, keys and existing config keep working. Bare `essh` opens a ratatui toolbox with Hosts, Keys, Tunnels and Mounts tabs and their wizards, while the CLI keeps connect, `ls`, `cp` and `update`. Crate `easyssh`, binary `essh`, repo `git@gitlab.com:safteinzz/easyssh.git`, published on crates.io under AGPL-3.0-only.

## Self-repair
If anything here contradicts the code, the code wins; fix AGENTS.md in the same session you notice the drift.
