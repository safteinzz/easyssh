//! `essh check <host>`: can this machine log in there without anybody typing,
//! and if not, why. It runs the login probe the TUI runs after a failed
//! connect, so the word it prints is the diagnosis the TUI would act on.

use super::fail;
use crate::tui::connect::{self, Diagnosis};

#[derive(clap::Args)]
pub struct Args {
    /// The host to log in to, a config alias or `user@host`
    #[arg(value_name = "HOST",
          add = clap_complete::ArgValueCompleter::new(crate::completion::host_names))]
    host: String,
}

pub fn run(args: Args) {
    let Some(out) = connect::probe(&args.host) else {
        fail("could not run `ssh`, so it is not installed");
    };
    if out.status.success() {
        println!("ok");
        return;
    }
    let text = String::from_utf8_lossy(&out.stderr);
    let host = &args.host;
    let (word, next) = match connect::classify(host, &text) {
        Some(Diagnosis::KeyChanged { target }) => (
            "key-changed",
            format!("its host key changed: only if you trust that, `ssh-keygen -R {target}`"),
        ),
        Some(Diagnosis::TooManyKeys) => (
            "too-many-keys",
            format!(
                "the agent offered every key before the right one: `essh host edit {host} --key PATH` and `IdentitiesOnly yes` in its block"
            ),
        ),
        Some(Diagnosis::KeysOnly) => (
            "keys-only",
            format!(
                "it takes keys only and none offered is authorized: `essh key install KEY {host} --via HOST`"
            ),
        ),
        Some(Diagnosis::Refused) => ("refused", "nothing listens on its ssh port".to_string()),
        Some(Diagnosis::Unresolved) => (
            "unresolved",
            format!("its HostName does not resolve: `essh host edit {host} --hostname ADDR`"),
        ),
        Some(Diagnosis::Unreachable) => (
            "unreachable",
            "no answer: it is off, on another network, or behind a firewall".to_string(),
        ),
        None if text.contains("Permission denied") => (
            "password",
            format!(
                "it wants a password, which a check cannot type: `essh key install KEY {host}` once makes it keys"
            ),
        ),
        None => (
            "failed",
            "ssh failed for a reason not listed here".to_string(),
        ),
    };
    println!("{word}");
    let said = connect::said(&text);
    if !said.is_empty() {
        eprintln!("{said}");
    }
    eprintln!("essh: {next}");
    std::process::exit(1);
}
