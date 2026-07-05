//! easyssh — an ssh toolbox that makes ssh simple. Binary: `essh`.
//!
//! One tool that swallows `ssh`, `scp`, `ssh-keygen`, `ssh-copy-id` and `sshfs`
//! so you never dig through man pages or your own notes again.
//!
//!   essh                 Launch the TUI: browse hosts, keys, tunnels, mounts
//!   essh <host> [args]   Connect (anything unknown is an ssh destination)
//!   essh ls              List every host in ~/.ssh/config  (-v shows targets)
//!   essh cp <src> <dst>  Copy files (scp, but `host:path` and auto -r)
//!
//! The philosophy: the CLI holds only what's faster to type than to click.
//! Everything you'd have to *look up* — new keys, copying keys, port forwards,
//! mounts, adding a host — lives in the TUI, where there's nothing to remember.

mod commands;
mod keys;
mod sshcfg;
mod tui;
mod tunnels;

use clap::{Parser, Subcommand};

/// Shown under `essh --help`: the one thing the command list can't convey — that
/// the real power is the TUI, and any bare word is just an ssh connect.
const AFTER: &str = "Run `essh` with no arguments to open the toolbox (keys, tunnels, mounts, add host).
Any other word is an ssh destination: `essh raspi`, `essh raspi -p 2222`.";

#[derive(Parser)]
#[command(
    name = "easyssh",
    bin_name = "essh",
    version,
    about = "make ssh easy — hosts, keys, tunnels, mounts and copies in one tool",
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
    /// Copy files over scp with alias:path shorthand and auto -r
    Cp(commands::cp::Args),
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
        Some(Cmd::Connect(args)) => commands::connect::run(args),
    }
}
