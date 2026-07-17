//! easyssh - an ssh toolbox that makes ssh simple. Binary: `essh`.
//!
//! One tool that swallows `ssh`, `scp`, `ssh-keygen`, `ssh-copy-id` and `sshfs`
//! so you never dig through man pages or your own notes again.
//!
//!   essh                 Launch the TUI: browse hosts, keys, tunnels, mounts
//!   essh <host> [args]   Connect (anything unknown is an ssh destination)
//!   essh ls              List every host in ~/.ssh/config  (-v shows targets)
//!   essh cp <src> <dst>  Copy files (scp, but `host:path` and auto -r)
//!   essh update          Update easyssh to the latest release on crates.io
//!
//! The philosophy: the CLI holds only what's faster to type than to click.
//! Everything you'd have to *look up* - new keys, copying keys, port forwards,
//! mounts, adding a host - lives in the TUI, where there's nothing to remember.

mod commands;
mod keys;
mod mounts;
mod sshcfg;
mod tui;
mod tunnels;

use clap::{Parser, Subcommand};

/// Shown under `essh --help`. The command list can't convey the two things that
/// aren't subcommands: bare `essh` opens the TUI, and any bare word connects.
const AFTER: &str = "\
Two more ways to run it (not subcommands):
  essh                 open the toolbox (TUI): hosts, keys, tunnels, mounts
  essh <host> [args]   connect (any unknown word is an ssh destination, e.g. `essh raspi -p 2222`)

The toolbox is where keys, tunnels, mounts, and adding/editing hosts live.
Run `essh <command> --help` for a command's details.";

#[derive(Parser)]
#[command(
    name = "easyssh",
    bin_name = "essh",
    version,
    about = "make ssh easy - hosts, keys, tunnels, mounts and copies in one tool",
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
    /// Copy files over scp: alias:path shorthand, auto -r for directories
    ///   essh cp notes.md raspi:~      one or more sources, then the destination
    #[command(verbatim_doc_comment)]
    Cp(commands::cp::Args),
    /// Update easyssh to the latest release (cargo install easyssh --force)
    ///   -y   skip the confirmation prompt
    #[command(verbatim_doc_comment)]
    Update(commands::update::Args),
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
        Some(Cmd::Update(args)) => commands::update::run(args),
        Some(Cmd::Connect(args)) => commands::connect::run(args),
    }
}
