//! `essh host`: the add and edit host forms without the terminal. Every write
//! goes through the same `sshcfg` calls the forms use, so the config is backed
//! up first, a shared `Host a b c` block is joined or refused the same way, and
//! only the touched block changes.

use super::{DryRun, fail, step, warn};
use crate::keys;
use crate::sshcfg::{self, NewHost};

#[derive(clap::Subcommand)]
pub enum Cmd {
    /// Add a host
    Add(AddArgs),
    /// Change only the settings given; an empty value removes that line
    Edit(EditArgs),
    /// Delete a host's block
    Rm(RmArgs),
}

/// One flag per field of the host form.
#[derive(clap::Args)]
pub struct Fields {
    /// HostName: the address ssh dials (default: the alias itself)
    #[arg(long, value_name = "ADDR")]
    hostname: Option<String>,
    /// User to log in as (default: your login name)
    #[arg(long)]
    user: Option<String>,
    /// Port, when it is not 22
    #[arg(long)]
    port: Option<String>,
    /// IdentityFile: the private key to log in with
    #[arg(long, value_name = "PATH")]
    key: Option<String>,
    /// ProxyJump: the host to hop through
    #[arg(long, value_name = "HOST")]
    jump: Option<String>,
    /// RemoteCommand: what runs instead of a shell, e.g. `tmux a`
    #[arg(long, value_name = "CMD")]
    command: Option<String>,
    /// ForwardAgent: yes, no, or a socket path
    #[arg(long, value_name = "yes|no|PATH")]
    forward_agent: Option<String>,
}

#[derive(clap::Args)]
pub struct AddArgs {
    /// The name you type after `essh`; a pasted `user@host:port` fills --user, --hostname and --port
    #[arg(value_name = "ALIAS")]
    alias: String,
    #[command(flatten)]
    fields: Fields,
    #[command(flatten)]
    dry: DryRun,
}

#[derive(clap::Args)]
pub struct EditArgs {
    /// The host to change; it must be the only alias on its `Host` line
    #[arg(value_name = "ALIAS",
          add = clap_complete::ArgValueCompleter::new(crate::completion::host_names))]
    alias: String,
    /// Give it a new alias
    #[arg(long, value_name = "ALIAS")]
    rename: Option<String>,
    #[command(flatten)]
    fields: Fields,
    #[command(flatten)]
    dry: DryRun,
}

#[derive(clap::Args)]
pub struct RmArgs {
    /// The host to delete; it must be the only alias on its `Host` line
    #[arg(value_name = "ALIAS",
          add = clap_complete::ArgValueCompleter::new(crate::completion::host_names))]
    alias: String,
    #[command(flatten)]
    dry: DryRun,
}

pub fn run(cmd: Cmd) {
    match cmd {
        Cmd::Add(args) => add(args),
        Cmd::Edit(args) => edit(args),
        Cmd::Rm(args) => {
            if !exists(&args.alias) {
                fail(&format!(
                    "no host '{}' in ~/.ssh/config: `essh ls` lists them",
                    args.alias
                ));
            }
            step(&format!("delete Host {} from ~/.ssh/config", args.alias));
            if !args.dry.dry_run
                && let Err(e) = sshcfg::delete_host(&args.alias)
            {
                fail(&format!("{e:#}"));
            }
        }
    }
}

fn exists(alias: &str) -> bool {
    sshcfg::list_hosts().iter().any(|h| h.alias == alias)
}

fn add(args: AddArgs) {
    let mut f = args.fields;
    let mut alias = args.alias.trim().to_string();
    // The paste the add form splits: only when no flag already says otherwise.
    if f.hostname.is_none()
        && let Some(t) = sshcfg::parse_target(&alias)
    {
        alias = t.alias;
        f.hostname = Some(t.hostname);
        if !t.user.is_empty() {
            f.user.get_or_insert(t.user);
        }
        if !t.port.is_empty() {
            f.port.get_or_insert(t.port);
        }
    }
    if alias.is_empty() || alias.contains(char::is_whitespace) {
        fail("an alias is one word");
    }
    if exists(&alias) {
        fail(&format!(
            "'{alias}' is already in ~/.ssh/config: `essh host edit {alias}` changes it"
        ));
    }
    let nh = NewHost {
        alias: alias.clone(),
        hostname: f.hostname.unwrap_or_default(),
        user: f.user.unwrap_or_default(),
        port: f.port.unwrap_or_default(),
        identity: f.key.unwrap_or_default(),
        proxy_jump: f.jump.unwrap_or_default(),
        remote_command: f.command.unwrap_or_default(),
        forward_agent: f.forward_agent.unwrap_or_else(|| "no".into()),
    };
    write(&nh, None, args.dry.dry_run);
}

fn edit(args: EditArgs) {
    let Some(h) = sshcfg::list_hosts()
        .into_iter()
        .find(|h| h.alias == args.alias)
    else {
        fail(&format!(
            "no host '{}' in ~/.ssh/config: `essh ls` lists them",
            args.alias
        ));
    };
    let f = args.fields;
    // What the edit form opens on, with the flags given laid over it.
    let keep = |given: Option<String>, now: Option<String>| given.or(now).unwrap_or_default();
    let alias = args.rename.unwrap_or_else(|| h.alias.clone());
    if alias != h.alias && exists(&alias) {
        fail(&format!("'{alias}' is already in ~/.ssh/config"));
    }
    let nh = NewHost {
        alias,
        hostname: keep(f.hostname, h.hostname),
        user: keep(f.user, h.user),
        port: keep(f.port, h.port),
        identity: keep(f.key, h.identity),
        proxy_jump: keep(f.jump, h.proxy_jump),
        remote_command: keep(f.command, h.remote_command),
        forward_agent: f
            .forward_agent
            .unwrap_or_else(|| sshcfg::forward_agent_for(&h.alias)),
    };
    write(&nh, Some(&h.alias), args.dry.dry_run);
}

/// Refuse what the form refuses, write it, and warn about a key that is not
/// there yet, since ssh skips a missing IdentityFile without a word.
fn write(nh: &NewHost, original: Option<&str>, dry: bool) {
    let identity = nh.identity.trim();
    if keys::is_public_key(&sshcfg::expand_tilde(identity)) {
        let private = identity.strip_suffix(".pub").unwrap_or("its private half");
        fail(&format!(
            "`{identity}` is a public key: --key takes the private one, `{private}`"
        ));
    }
    match original {
        None => step(&format!("add Host {} to ~/.ssh/config", nh.alias)),
        Some(was) => step(&format!("edit Host {was} in ~/.ssh/config")),
    }
    if dry {
        return;
    }
    let done = match original {
        None => sshcfg::add_host(nh),
        Some(was) => sshcfg::update_host(was, nh),
    };
    if let Err(e) = done {
        fail(&format!("{e:#}"));
    }
    // ssh expands `%d`, `${HOME}` and quotes itself, so such a path cannot be
    // checked from here.
    if !identity.is_empty()
        && !identity.contains(['%', '$', '"'])
        && !sshcfg::expand_tilde(identity).exists()
    {
        warn(&format!(
            "IdentityFile `{identity}` does not exist yet, so ssh will skip it: `essh key new` makes one"
        ));
    }
}
