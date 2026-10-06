//! `essh tunnel`: the Tunnels tab without the terminal. The same file of kept
//! forwards, the same state file, the same detached `ssh -N`, so a forward
//! opened here is a row there and the other way round.

use super::{DryRun, Json, fail, json_array, json_str, step};
use crate::tunnels::{self, Entry, Saved};
use clap::ArgGroup;

#[derive(clap::Subcommand)]
pub enum Cmd {
    /// Every forward you keep, on or off
    Ls(Json),
    /// Keep a forward and start it
    Add(AddArgs),
    /// Start kept forwards
    On(Pick),
    /// Stop forwards and keep their lines
    Off(Pick),
    /// Stop a forward and delete its line
    Rm(RmArgs),
}

/// `-L`, `-R` or `-D` and its spec: what ssh is handed, so what the line keeps.
#[derive(clap::Args)]
pub struct Forward {
    /// Reach a port the host can reach, from here: [BIND:]PORT:HOST:PORT
    #[arg(short = 'L', value_name = "SPEC")]
    local: Option<String>,
    /// Expose a port this machine can reach, on the host: [BIND:]PORT:HOST:PORT
    #[arg(short = 'R', value_name = "SPEC")]
    remote: Option<String>,
    /// A SOCKS proxy here that reaches whatever the host can: [BIND:]PORT
    #[arg(short = 'D', value_name = "SPEC")]
    dynamic: Option<String>,
}

impl Forward {
    fn get(&self) -> Option<(char, &str)> {
        [
            ('L', &self.local),
            ('R', &self.remote),
            ('D', &self.dynamic),
        ]
        .into_iter()
        .find_map(|(kind, spec)| spec.as_deref().map(|s| (kind, s)))
    }
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("flag").args(["local", "remote", "dynamic"]).required(true)))]
pub struct AddArgs {
    #[command(flatten)]
    forward: Forward,
    /// The host it goes through, a config alias or `user@host`
    #[arg(value_name = "HOST")]
    host: String,
    /// A label for the row; without one it is spelled out from the ports
    #[arg(long, value_name = "LABEL")]
    name: Option<String>,
    #[command(flatten)]
    dry: DryRun,
}

/// Which kept forwards: by the name `tunnel ls` prints, by what they do, or all.
#[derive(clap::Args)]
#[command(group(ArgGroup::new("flag").args(["local", "remote", "dynamic"])))]
pub struct Pick {
    #[command(flatten)]
    forward: Forward,
    /// Its name as `tunnel ls` prints it, or its host after -L, -R or -D
    #[arg(value_name = "NAME", required_unless_present = "all")]
    target: Option<String>,
    /// Every kept forward
    #[arg(long, conflicts_with_all = ["target", "flag"])]
    all: bool,
    #[command(flatten)]
    dry: DryRun,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("flag").args(["local", "remote", "dynamic"])))]
pub struct RmArgs {
    #[command(flatten)]
    forward: Forward,
    /// Its name as `tunnel ls` prints it, or its host after -L, -R or -D
    #[arg(value_name = "NAME")]
    target: String,
    #[command(flatten)]
    dry: DryRun,
}

pub fn run(cmd: Cmd) {
    match cmd {
        Cmd::Ls(args) => list(args.json),
        Cmd::Add(args) => add(args),
        Cmd::On(args) => {
            for e in pick(&args.forward, args.target.as_deref(), args.all) {
                start(&e, args.dry.dry_run);
            }
        }
        Cmd::Off(args) => {
            for e in pick(&args.forward, args.target.as_deref(), args.all) {
                stop(&e, args.dry.dry_run);
            }
        }
        Cmd::Rm(args) => {
            for e in pick(&args.forward, Some(&args.target), false) {
                stop(&e, args.dry.dry_run);
                step(&format!("forget -{} {} {}", e.kind, e.spec, e.host));
                if !args.dry.dry_run
                    && let Err(e) = tunnels::forget(e.kind, &e.spec, &e.host)
                {
                    fail(&format!("could not delete the line: {e:#}"));
                }
            }
        }
    }
}

fn list(json: bool) {
    let rows = tunnels::entries();
    if json {
        let rows: Vec<String> = rows
            .iter()
            .map(|e| {
                format!(
                    "{{\"name\": {}, \"flag\": {}, \"spec\": {}, \"host\": {}, \"state\": {}, \"pid\": {}, \"command\": {}}}",
                    json_str(&e.label()),
                    json_str(&format!("-{}", e.kind)),
                    json_str(&e.spec),
                    json_str(&e.host),
                    json_str(state(e)),
                    e.pid().map_or_else(|| "null".into(), |p| p.to_string()),
                    json_str(&e.command()),
                )
            })
            .collect();
        println!("{}", json_array(&rows));
        return;
    }
    let name_w = rows
        .iter()
        .map(|e| e.label().chars().count())
        .max()
        .unwrap_or(0);
    let spec_w = rows.iter().map(|e| e.spec.len()).max().unwrap_or(0);
    for e in &rows {
        let label = e.label();
        let pad = " ".repeat(name_w - label.chars().count());
        println!(
            "{:3}  {label}{pad}  -{} {:spec_w$}  {}",
            state(e),
            e.kind,
            e.spec,
            e.host
        );
    }
}

fn state(e: &Entry) -> &'static str {
    if e.on() { "on" } else { "off" }
}

fn add(args: AddArgs) {
    let (kind, spec) = args.forward.get().expect("clap requires one flag");
    // The kept line is `-L spec host = label`, split on whitespace and the first
    // `=`, so either inside a spec or a host would not read back.
    for (what, value) in [("spec", spec), ("host", args.host.as_str())] {
        if value.is_empty() || value.contains(char::is_whitespace) || value.contains('=') {
            fail(&format!(
                "the {what} `{value}` cannot hold a space or `=`, since the kept line could not be read back"
            ));
        }
    }
    // ssh's own shapes: a target for -L and -R, none for -D, and an optional
    // bind address in front of either.
    let wanted: &[usize] = if kind == 'D' { &[1, 2] } else { &[3, 4] };
    if !wanted.contains(&fields(spec)) {
        let shape = if kind == 'D' {
            "[BIND:]PORT"
        } else {
            "[BIND:]PORT:HOST:PORT"
        };
        fail(&format!("-{kind} takes {shape}, not `{spec}`"));
    }
    let name = args.name.filter(|n| !n.trim().is_empty());
    if name.as_deref().is_some_and(|n| n.contains('\n')) {
        fail("a name is one line");
    }
    let saved = Saved {
        kind,
        spec: spec.to_string(),
        host: args.host.clone(),
        name: name.clone(),
    };
    let label = match &name {
        Some(n) => format!(" = {n}"),
        None => String::new(),
    };
    step(&format!("keep -{kind} {spec} {}{label}", args.host));
    if !args.dry.dry_run
        && let Err(e) = tunnels::remember(&saved)
    {
        fail(&format!("could not keep it: {e:#}"));
    }
    let entry = tunnels::entries()
        .into_iter()
        .find(|e| e.kind == kind && e.spec == spec && e.host == args.host)
        .unwrap_or(Entry {
            name,
            kind,
            spec: spec.to_string(),
            host: args.host,
            live: None,
        });
    start(&entry, args.dry.dry_run);
}

/// How many `:`-separated fields a spec has, an IPv6 address in brackets
/// counting as one.
fn fields(spec: &str) -> usize {
    let mut depth = 0;
    1 + spec
        .chars()
        .filter(|&c| {
            match c {
                '[' => depth += 1,
                ']' => depth -= 1,
                _ => {}
            }
            c == ':' && depth == 0
        })
        .count()
}

/// Turn one on, unless it already is.
fn start(e: &Entry, dry: bool) {
    if let Some(pid) = e.pid() {
        step(&format!("already on: {} (pid {pid})", e.command()));
        return;
    }
    step(&e.command());
    if dry {
        return;
    }
    if let Err(err) = tunnels::open(e.kind, &e.spec, &e.host) {
        fail(&format!(
            "could not start '{}': {err}\n  the line is kept, so `essh tunnel on` tries it again",
            e.label()
        ));
    }
}

/// Turn one off, if it is on.
fn stop(e: &Entry, dry: bool) {
    let Some(pid) = e.pid() else {
        return;
    };
    step(&format!("kill {pid}"));
    if !dry && let Err(err) = tunnels::kill(pid) {
        fail(&format!("could not stop '{}': {err:#}", e.label()));
    }
}

/// The kept forwards these arguments name. Refuses rather than guessing when a
/// name matches none or several.
fn pick(forward: &Forward, target: Option<&str>, all: bool) -> Vec<Entry> {
    let rows = tunnels::entries();
    if all {
        return rows;
    }
    let target = target.unwrap_or_default();
    if let Some((kind, spec)) = forward.get() {
        return match rows
            .into_iter()
            .find(|e| e.kind == kind && e.spec == spec && e.host == target)
        {
            Some(e) => vec![e],
            None => fail(&format!(
                "no kept forward `-{kind} {spec} {target}`: `essh tunnel ls` lists them, `essh tunnel add` keeps one"
            )),
        };
    }
    let found: Vec<Entry> = rows.into_iter().filter(|e| e.label() == target).collect();
    match found.len() {
        0 => fail(&format!(
            "no kept forward called `{target}`: `essh tunnel ls` lists them by name"
        )),
        1 => found,
        _ => fail(&format!(
            "several forwards are called `{target}`: name one by what it does, as in `-L SPEC HOST`"
        )),
    }
}
