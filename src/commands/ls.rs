//! `essh ls` - every host in your config, one per line. This is your old
//! `grep '^Host ' ~/.ssh/config | grep -v '*' | awk '{print $2}'` alias, built
//! in, so you never have to remember the names again. Given `host:path` it
//! lists that folder on the host instead.

use super::cp::remote_alias;
use crate::completion::remote_dir;
use crate::sshcfg;
use colored::Colorize;
use std::process::Command;

#[derive(clap::Args)]
pub struct Args {
    /// Also show where each alias connects (user@hostname:port)
    #[arg(short, long)]
    pub verbose: bool,
    #[command(flatten)]
    pub json: super::Json,
    /// A folder to list on a host instead, as `host:path`; `host:` lists its home
    #[arg(value_name = "HOST:PATH", conflicts_with_all = ["verbose", "json"],
          add = clap_complete::ArgValueCompleter::new(crate::completion::remote_path))]
    pub remote: Option<String>,
}

pub fn run(args: Args) {
    if let Some(remote) = &args.remote {
        list_remote(remote);
    }
    let hosts = sshcfg::list_hosts();

    if args.json.json {
        use super::{json_array, json_opt, json_str};
        let rows: Vec<String> = hosts
            .iter()
            .map(|h| {
                format!(
                    "{{\"alias\": {}, \"hostname\": {}, \"user\": {}, \"port\": {}, \"key\": {}, \"jump\": {}, \"command\": {}, \"forward_agent\": {}}}",
                    json_str(&h.alias),
                    json_opt(h.hostname.as_deref()),
                    json_opt(h.user.as_deref()),
                    json_opt(h.port.as_deref()),
                    json_opt(h.identity.as_deref()),
                    json_opt(h.proxy_jump.as_deref()),
                    json_opt(h.remote_command.as_deref()),
                    json_opt(h.forward_agent.as_deref()),
                )
            })
            .collect();
        println!("{}", json_array(&rows));
        return;
    }

    if hosts.is_empty() {
        eprintln!(
            "{}",
            "No hosts in ~/.ssh/config yet: `essh host add <ALIAS>` adds one.".dimmed()
        );
        return;
    }

    if args.verbose {
        // Pad the alias column so the targets line up.
        let width = hosts.iter().map(|h| h.alias.len()).max().unwrap_or(0);
        for h in &hosts {
            // Show the key too when the host pins one - handy for "which key is
            // this?" - and the jump host, since a ProxyJump is the difference
            // between an address you can reach and one you cannot.
            let key = match &h.identity {
                Some(i) => format!("  ({i})"),
                None => String::new(),
            };
            let via = match &h.proxy_jump {
                Some(j) => format!("  via {j}"),
                None => String::new(),
            };
            // A RemoteCommand host does not give you a shell, so it belongs on
            // the line beside where it connects.
            let runs = match &h.remote_command {
                Some(c) => format!("  runs {c}"),
                None => String::new(),
            };
            println!(
                "  {:width$}  {}{}{}{}",
                h.alias.bold(),
                h.target().dimmed(),
                via.dimmed(),
                key.dimmed(),
                runs.dimmed()
            );
        }
    } else {
        for h in &hosts {
            println!("{}", h.alias);
        }
    }
}

/// `ls` on the far side, its output as ours and its exit code as ours.
fn list_remote(remote: &str) -> ! {
    let Some(alias) = remote_alias(remote).filter(|a| !a.starts_with('-')) else {
        super::fail(&format!(
            "`{remote}` is not a folder on a host: write it as `<host>:<path>`"
        ));
    };
    let path = &remote[alias.len() + 1..];
    let target = if path.is_empty() {
        Some(String::new())
    } else {
        remote_dir(path)
    };
    let Some(target) = target else {
        super::fail(&format!(
            "`{path}` is not a path essh can quote: name the folder without `~user`"
        ));
    };
    let status = Command::new("ssh")
        .args(["-o", "RemoteCommand=none", "-o", "RequestTTY=no"])
        .args([&alias, "--", &format!("command ls -- {target}")])
        .status();
    match status {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            super::fail("`ssh` is not installed: install OpenSSH's client")
        }
        Err(e) => super::fail(&format!("could not run `ssh`: {}", e.kind())),
    }
}
