//! `essh key`: the Keys tab without the terminal. Listing reads what the tab
//! reads, making a key runs the `ssh-keygen` the new-key form builds, and
//! installing one runs `ssh-copy-id`, or the two-hop install the tab offers for
//! a host that takes keys only.

use super::{DryRun, Json, fail, json_array, json_str, run_tool, step};
use crate::sshcfg::{self, collapse_tilde};
use crate::{keys, tui::mount_spec::shell_join};
use std::path::PathBuf;

#[derive(clap::Subcommand)]
pub enum Cmd {
    /// Every keypair in ~/.ssh, and which hosts use it
    Ls(Json),
    /// Make a keypair in ~/.ssh with ssh-keygen
    New(NewArgs),
    /// Add a public key to a host's authorized_keys
    Install(InstallArgs),
}

#[derive(clap::Args)]
pub struct NewArgs {
    /// The file name in ~/.ssh (default: id_<type>)
    #[arg(value_name = "NAME")]
    name: Option<String>,
    /// The key type: ed25519, or rsa (4096 bits) for a host too old for it
    #[arg(short = 't', value_name = "ed25519|rsa", default_value = "ed25519",
          value_parser = ["ed25519", "rsa"])]
    kind: String,
    /// The comment stored in the public key, usually who and where
    #[arg(short = 'C', value_name = "COMMENT")]
    comment: Option<String>,
    /// Write it unencrypted instead of asking for a passphrase
    #[arg(long)]
    no_passphrase: bool,
    #[command(flatten)]
    dry: DryRun,
}

#[derive(clap::Args)]
pub struct InstallArgs {
    /// A key's name in ~/.ssh as `key ls` prints it, or a path to it
    #[arg(value_name = "KEY")]
    key: String,
    /// The host whose authorized_keys gets it
    #[arg(value_name = "HOST",
          add = clap_complete::ArgValueCompleter::new(crate::completion::host_names))]
    host: String,
    /// Go through a host that can already log in there, for one that takes keys only
    #[arg(long, value_name = "HOST",
          add = clap_complete::ArgValueCompleter::new(crate::completion::host_names))]
    via: Option<String>,
    #[command(flatten)]
    dry: DryRun,
}

pub fn run(cmd: Cmd) {
    match cmd {
        Cmd::Ls(args) => list(args.json),
        Cmd::New(args) => new(args),
        Cmd::Install(args) => install(args),
    }
}

fn list(json: bool) {
    let mut all = keys::list();
    keys::link_hosts(&mut all, &sshcfg::list_hosts());
    if json {
        let rows: Vec<String> = all
            .iter()
            .map(|k| {
                let used: Vec<String> = k.used_by.iter().map(|a| json_str(a)).collect();
                format!(
                    "{{\"name\": {}, \"path\": {}, \"type\": {}, \"bits\": {}, \"fingerprint\": {}, \"comment\": {}, \"agent\": {}, \"passphrase\": {}, \"used_by\": [{}]}}",
                    json_str(&k.name()),
                    json_str(&k.path.to_string_lossy()),
                    json_str(&k.kind),
                    json_str(&k.bits),
                    json_str(&k.fingerprint),
                    json_str(&k.comment),
                    k.agent_loaded,
                    k.encrypted,
                    used.join(", "),
                )
            })
            .collect();
        println!("{}", json_array(&rows));
        return;
    }
    let name_w = all.iter().map(|k| k.name().len()).max().unwrap_or(0);
    for k in &all {
        let mut marks = Vec::new();
        if k.agent_loaded {
            marks.push("agent");
        }
        if k.encrypted {
            marks.push("passphrase");
        }
        let used = if k.used_by.is_empty() {
            String::new()
        } else {
            format!("  used by {}", k.used_by.join(", "))
        };
        println!(
            "{:name_w$}  {:7}  {}{used}",
            k.name(),
            k.kind,
            marks.join(" ")
        );
    }
}

fn new(args: NewArgs) {
    let name = args.name.unwrap_or_else(|| format!("id_{}", args.kind));
    if name.contains('/') {
        fail("a key's name is a file name in ~/.ssh, without a `/`");
    }
    let path = sshcfg::ssh_dir().join(&name);
    if path.exists() {
        fail(&format!(
            "{} already exists: pick another NAME",
            collapse_tilde(&path.to_string_lossy())
        ));
    }
    let mut argv = vec![
        "ssh-keygen".to_string(),
        "-t".into(),
        args.kind.clone(),
        "-f".into(),
        path.to_string_lossy().into_owned(),
    ];
    if args.kind == "rsa" {
        argv.extend(["-b".into(), "4096".into()]);
    }
    if let Some(c) = args.comment {
        argv.extend(["-C".into(), c]);
    }
    if args.no_passphrase {
        argv.extend(["-N".into(), String::new()]);
    }
    let shown: Vec<String> = argv
        .iter()
        .map(|a| match a.as_str() {
            "" => "\"\"".into(),
            a => collapse_tilde(a),
        })
        .collect();
    step(&shell_join(&shown));
    if args.dry.dry_run {
        return;
    }
    let _ = std::fs::create_dir_all(sshcfg::ssh_dir());
    match run_tool(&argv) {
        Ok(0) => {}
        Ok(code) => fail(&format!("ssh-keygen exited {code}")),
        Err(e) => fail(&e),
    }
}

/// The private key and its `.pub`, from a name in ~/.ssh or a path to either.
fn key_paths(key: &str) -> (PathBuf, PathBuf) {
    let given = if key.contains('/') || key.starts_with('~') {
        sshcfg::expand_tilde(key)
    } else {
        sshcfg::ssh_dir().join(key)
    };
    let private = match given.to_string_lossy().strip_suffix(".pub") {
        Some(p) => PathBuf::from(p),
        None => given,
    };
    let public = keys::pub_path(&private);
    if !public.exists() {
        fail(&format!(
            "no public key at {}: `essh key ls` lists the keys in ~/.ssh",
            collapse_tilde(&public.to_string_lossy())
        ));
    }
    (private, public)
}

fn install(args: InstallArgs) {
    let (_, public) = key_paths(&args.key);
    let shown_pub = collapse_tilde(&public.to_string_lossy());
    let Some(via) = args.via else {
        let argv = vec![
            "ssh-copy-id".to_string(),
            "-i".into(),
            public.to_string_lossy().into_owned(),
            args.host.clone(),
        ];
        step(&format!("ssh-copy-id -i {shown_pub} {}", args.host));
        if args.dry.dry_run {
            return;
        }
        match run_tool(&argv) {
            Ok(0) => {}
            Ok(code) => fail(&format!(
                "ssh-copy-id exited {code}; when {} takes keys only, `--via` a host that can already log in there",
                args.host
            )),
            Err(e) => fail(&e),
        }
        return;
    };
    // The far side's authorized_keys is the one this machine logs in to, so
    // the user and address are ours, as ssh resolves them.
    let Some(login) = sshcfg::resolve(&args.host) else {
        fail("could not run `ssh -G` to resolve the host");
    };
    let pubkey = std::fs::read_to_string(&public).unwrap_or_default();
    let Some(argv) = keys::install_via_argv(&via, &args.host, &login, pubkey.trim()) else {
        fail(&format!("{shown_pub} does not hold a public key"));
    };
    step(&format!(
        "ssh -t {via}: add {shown_pub} to {}@{}'s authorized_keys",
        login.user, args.host
    ));
    if args.dry.dry_run {
        return;
    }
    match run_tool(&argv) {
        Ok(keys::ADDED) => {}
        // Already listed, so a refused login is about the User or permissions.
        Ok(keys::ALREADY_AUTHORIZED) => step(&format!(
            "already in {}@{}'s authorized_keys",
            login.user, args.host
        )),
        Ok(code) => fail(&format!(
            "the install through {via} did not finish (exit {code})"
        )),
        Err(e) => fail(&e),
    }
}
