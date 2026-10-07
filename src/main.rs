//! easyssh - an ssh toolbox that makes ssh simple. Binary: `essh`.
//!
//! One tool that swallows `ssh`, `scp`, `ssh-keygen`, `ssh-copy-id` and `sshfs`
//! so you never dig through man pages or your own notes again.
//!
//! This file is the clap `Cmd` enum and the dispatch match; what you can run is
//! `essh --help`, which renders from the manifest, those doc comments and
//! `AFTER`, and is the only copy of that list.
//!
//! The TUI is where a person finds things; the CLI holds the same actions for
//! a script or an agent, one noun per tab (`host`, `tunnel`, `mount`, `key`),
//! so nothing the toolbox does needs a terminal to drive it.

mod clip;
mod commands;
mod completion;
mod history;
mod keys;
mod mounts;
mod paste;
mod reach;
mod settings;
mod sshcfg;
mod tui;
mod tunnels;

use clap::{CommandFactory, Parser, Subcommand};

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
  essh                    open the toolbox (TUI): hosts, keys, tunnels, mounts, adding hosts, image paste
  essh <host>             connect (any unknown word is an ssh destination, e.g. `essh raspi`)
  essh <host> 'uptime'    run one command over ssh and come straight back
  essh <host> [ssh args]  anything else goes straight to ssh (`essh raspi -p 2222`)
  essh -- <host>          connect to a host whose alias is also a command (`essh -- mount`)";

/// The rest of the block: what a script can expect, then where to look next.
const AFTER: &str = concat!(
    "\
What it prints:
  on, off   a kept tunnel or mount is running or not (`state` in --json)
  check     ok · password · keys-only · key-changed · too-many-keys · refused
            · unresolved · unreachable · failed

A connect hands the terminal to ssh, so stdout, stderr and the exit code are
ssh's own. Everything else prints its result on stdout and its errors on stderr,
exiting 1 on a failure: the `ls` commands print a table for people, or with
--json an array whose field names are fixed; `check` prints one word and exits 0
only for ok; a command that changes something prints one line per step, and -n
prints the same lines and changes nothing; `on` and `off` take --all for every
kept one. Nothing asks except ssh's own tools:
a password for a connect, a mount or `key install`, and a passphrase for
`key new` without --no-passphrase.
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
    after_help = AFTER,
    add = clap_complete::engine::SubcommandCandidates::new(completion::hosts)
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// List every host in ~/.ssh/config, or a folder on one  [HOST:PATH]
    ///   -v      also show where each alias connects
    ///   --json  every host and its settings, for a script
    #[command(verbatim_doc_comment)]
    Ls(commands::ls::Args),
    /// Copy files over scp: alias:path shorthand, auto -r for directories  <PATH> <PATH>...
    ///   essh cp notes.md raspi:~      one or more sources, then the destination
    #[command(verbatim_doc_comment)]
    Cp(commands::cp::Args),
    /// Add, change or delete a host in ~/.ssh/config, backing the file up first  <COMMAND>
    ///   host add <ALIAS>   a new block: --hostname, --user, --port, --key, --jump
    ///   host edit <ALIAS>  change only the flags given, and "" removes that line
    ///   host rm <ALIAS>    delete its block
    #[command(verbatim_doc_comment, subcommand)]
    Host(commands::host::Cmd),
    /// Keep port forwards and turn them on and off, like the Tunnels tab  <COMMAND>
    ///   tunnel ls                                          every forward you keep (--json)
    ///   tunnel add <-L <SPEC>|-R <SPEC>|-D <SPEC>> <HOST>  keep it and start it (--name)
    ///   tunnel on [NAME]                                   start it, NAME as `tunnel ls` prints it
    ///   tunnel off [NAME]                                  stop it and keep its line
    ///   tunnel rm <NAME>                                   stop it and delete its line
    #[command(verbatim_doc_comment, subcommand)]
    Tunnel(commands::tunnel::Cmd),
    /// Keep sshfs mounts and mount and unmount them, like the Mounts tab  <COMMAND>
    ///   mount ls                         every mount you keep (--json)
    ///   mount add <HOST[:PATH]> [LOCAL]  keep it and mount it (--sudo to mount as root)
    ///   mount on [LOCAL]                 mount it, LOCAL as `mount ls` prints it
    ///   mount off [LOCAL]                unmount it and keep its line (--lazy when busy)
    ///   mount rm <LOCAL>                 unmount it and delete its line
    #[command(verbatim_doc_comment, subcommand)]
    Mount(commands::mount::Cmd),
    /// List, make and install the keys in ~/.ssh  <COMMAND>
    ///   key ls                    every keypair and the hosts using it (--json)
    ///   key new [NAME]            make one with ssh-keygen (-t rsa, -C, --no-passphrase)
    ///   key install <KEY> <HOST>  ssh-copy-id, or --via HOST for one that takes keys only
    #[command(verbatim_doc_comment, subcommand)]
    Key(commands::key::Cmd),
    /// Say whether this machine can log in to a host without typing, and why not  <HOST>
    Check(commands::check::Args),
    /// Print the script that turns on Tab completion  <bash|zsh|fish>
    ///   --add   add the line that loads it to your shell's startup file
    #[command(verbatim_doc_comment)]
    Completions(commands::completions::Args),
    /// Manage easyssh itself: `self update` reinstalls, `self check` looks for a newer release
    #[command(name = "self", subcommand)]
    Selfie(commands::selfcmd::Cmd),
    /// Any other word is an ssh destination, passed straight to ssh
    #[command(external_subcommand)]
    Connect(Vec<String>),
}

fn main() {
    completion::handle(Cli::command);
    // `essh -- <host>` is a connect whatever the word is, so a host named like
    // one of the commands can still be reached.
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("--") {
        commands::connect::run(argv[2..].to_vec());
        return;
    }
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
        Some(Cmd::Host(cmd)) => commands::host::run(cmd),
        Some(Cmd::Tunnel(cmd)) => commands::tunnel::run(cmd),
        Some(Cmd::Mount(cmd)) => commands::mount::run(cmd),
        Some(Cmd::Key(cmd)) => commands::key::run(cmd),
        Some(Cmd::Check(args)) => commands::check::run(args),
        Some(Cmd::Completions(args)) => commands::completions::run(args, Cli::command()),
        Some(Cmd::Selfie(cmd)) => commands::selfcmd::run(cmd),
        Some(Cmd::Connect(args)) => commands::connect::run(args),
    }
}
