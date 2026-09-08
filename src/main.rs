//! easyssh - an ssh toolbox that makes ssh simple. Binary: `essh`.
//!
//! One tool that swallows `ssh`, `scp`, `ssh-keygen`, `ssh-copy-id` and `sshfs`
//! so you never dig through man pages or your own notes again.
//!
//! This file is the clap `Cmd` enum and the dispatch match; what you can run is
//! `essh --help`, which renders from the manifest, those doc comments and
//! `AFTER`, and is the only copy of that list.
//!
//! The philosophy: the CLI holds only what's faster to type than to click.
//! Everything you'd have to *look up* - new keys, copying keys, port forwards,
//! mounts, adding a host - lives in the TUI, where there's nothing to remember.

mod clip;
mod commands;
mod history;
mod keys;
mod mounts;
mod reach;
mod settings;
mod sshcfg;
mod tui;
mod tunnels;

use clap::{Parser, Subcommand};

/// clap's own layout with one change: `{before-help}` moves from above the
/// description to just under `Usage:`, so the shapes block lands on top of the
/// command list rather than on top of the screen.
const TEMPLATE: &str =
    "{about-with-newline}\n{usage-heading} {usage}\n\n{before-help}{all-args}{after-help}\n";

/// Shown under `essh --help`: the shapes clap cannot list, because a bare word
/// is a destination rather than a subcommand, and then the contract a script
/// needs. Read top to bottom by somebody - or something - looking for the one
/// line that answers "how do I run one command on that box", so that line is in
/// the block rather than in prose below it.
const WAYS: &str = "\x1b[1mWays to run it (not subcommands):\x1b[0m
  essh                    open the toolbox (TUI): hosts, keys, tunnels, mounts, adding hosts
  essh <host>             connect (any unknown word is an ssh destination, e.g. `essh raspi`)
  essh <host> 'uptime'    run one command over ssh and come straight back
  essh <host> [ssh args]  anything else goes straight to ssh (`essh raspi -p 2222`)";

/// The rest of the block: what a script can expect, then where to look next.
const AFTER: &str = concat!(
    "\
A connect becomes ssh itself, so stdout, stderr and the exit code are ssh's own
and essh's words are on stderr; `essh ls` prints a table for people, not data.
Run `essh <command> --help` for a command's details.",
    "\n\n",
    env!("CARGO_PKG_REPOSITORY"),
    "\ncontributors: ",
    env!("CARGO_PKG_AUTHORS"),
);

/// `-V` stays a bare version string for scripts; `--version` spells out the
/// license, where it lives, and who's contributed. Every field comes from
/// Cargo.toml, so none of it can drift from the manifest.
const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\n",
    env!("CARGO_PKG_LICENSE"),
    "  ",
    env!("CARGO_PKG_REPOSITORY"),
    "\ncontributors: ",
    env!("CARGO_PKG_AUTHORS"),
);

#[derive(Parser)]
#[command(
    name = "easyssh",
    bin_name = "essh",
    version,
    long_version = LONG_VERSION,
    about,
    // The shapes come first: this is a bare-first binary, so the command list is
    // the leftovers and putting it on top answers the wrong question first.
    help_template = TEMPLATE,
    before_help = WAYS,
    after_help = AFTER
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// List every host in ~/.ssh/config (your `ssh?` alias, built in)
    ///   -v   also show where each alias connects
    #[command(verbatim_doc_comment)]
    Ls(commands::ls::Args),
    /// Copy files over scp: alias:path shorthand, auto -r for directories  <PATH> <PATH>...
    ///   essh cp notes.md raspi:~      one or more sources, then the destination
    #[command(verbatim_doc_comment)]
    Cp(commands::cp::Args),
    /// Manage easyssh itself: `self update` reinstalls, `self check` looks for a newer release
    #[command(name = "self", subcommand)]
    Selfie(commands::selfcmd::Cmd),
    /// Any other word is an ssh destination, passed straight to ssh
    #[command(external_subcommand)]
    Connect(Vec<String>),
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        // Bare `essh` → the toolbox.
        None => {
            if let Err(e) = tui::run() {
                eprintln!("essh: {e}");
                std::process::exit(1);
            }
        }
        Some(Cmd::Ls(args)) => commands::ls::run(args),
        Some(Cmd::Cp(args)) => commands::cp::run(args),
        Some(Cmd::Selfie(cmd)) => commands::selfcmd::run(cmd),
        Some(Cmd::Connect(args)) => commands::connect::run(args),
    }
}
