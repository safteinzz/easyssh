//! `essh mount`: the Mounts tab without the terminal. The same kept lines, the
//! same sshfs command the mount wizard builds, and the same cleanup of an
//! emptied mountpoint.

use super::{DryRun, Json, fail, json_array, json_opt, json_str, run_tool, step};
use crate::mounts::{self, Mount};
use crate::sshcfg::{self, collapse_tilde};
use crate::tui::mount_spec::{self, MountSpec, Sudo, shell_join};
use std::fs;

#[derive(clap::Subcommand)]
pub enum Cmd {
    /// Every mount you keep, mounted or not
    Ls(Json),
    /// Keep a mount and mount it
    Add(AddArgs),
    /// Mount kept mounts
    On(Pick),
    /// Unmount and keep their lines
    Off(OffArgs),
    /// Unmount and delete its line
    Rm(RmArgs),
}

#[derive(clap::Args)]
pub struct AddArgs {
    /// The remote side: a config alias or `user@host`, then `:path` (blank is the login home)
    #[arg(value_name = "HOST[:PATH]",
          add = clap_complete::ArgValueCompleter::new(crate::completion::cp_path))]
    source: String,
    /// The local folder (default: the mount root setting, then the host's name)
    #[arg(value_name = "LOCAL")]
    local: Option<String>,
    /// Mount as root through `sudo <SERVER>`, which needs a NOPASSWD sudoers line there
    #[arg(long, value_name = "SERVER", num_args = 0..=1, default_missing_value = "")]
    sudo: Option<String>,
    #[command(flatten)]
    dry: DryRun,
}

#[derive(clap::Args)]
pub struct Pick {
    /// Its local folder as `mount ls` prints it
    #[arg(value_name = "LOCAL", required_unless_present = "all")]
    local: Option<String>,
    /// Every kept mount
    #[arg(long, conflicts_with = "local")]
    all: bool,
    #[command(flatten)]
    dry: DryRun,
}

#[derive(clap::Args)]
pub struct OffArgs {
    #[command(flatten)]
    pick: Pick,
    /// Detach a busy mount now and let the kernel free it once nothing uses it
    #[arg(long)]
    lazy: bool,
}

#[derive(clap::Args)]
pub struct RmArgs {
    /// Its local folder as `mount ls` prints it
    #[arg(value_name = "LOCAL")]
    local: String,
    #[command(flatten)]
    dry: DryRun,
}

pub fn run(cmd: Cmd) {
    match cmd {
        Cmd::Ls(args) => list(args.json),
        Cmd::Add(args) => add(args),
        Cmd::On(args) => {
            for m in pick(args.local.as_deref(), args.all) {
                start(MountSpec::from_saved(&m), m.on, false, args.dry.dry_run);
            }
        }
        Cmd::Off(args) => {
            for m in pick(args.pick.local.as_deref(), args.pick.all) {
                stop(&m, args.lazy, args.pick.dry.dry_run);
            }
        }
        Cmd::Rm(args) => {
            for m in pick(Some(&args.local), false) {
                stop(&m, false, args.dry.dry_run);
                step(&format!("forget {}", collapse_tilde(&m.local)));
                if !args.dry.dry_run
                    && let Err(e) = mounts::forget(&m.local)
                {
                    fail(&format!("could not delete the line: {e:#}"));
                }
            }
        }
    }
}

fn state(m: &Mount) -> &'static str {
    if m.on { "on" } else { "off" }
}

fn list(json: bool) {
    let rows = mounts::entries();
    if json {
        let rows: Vec<String> = rows
            .iter()
            .map(|m| {
                format!(
                    "{{\"local\": {}, \"remote\": {}, \"host\": {}, \"state\": {}, \"sudo\": {}}}",
                    json_str(&m.local),
                    json_str(&m.remote),
                    json_str(m.host()),
                    json_str(state(m)),
                    json_opt(m.sudo.as_deref()),
                )
            })
            .collect();
        println!("{}", json_array(&rows));
        return;
    }
    for m in &rows {
        println!("{:3}  {}", state(m), m.describe());
    }
}

fn add(args: AddArgs) {
    let (host, path) = args
        .source
        .split_once(':')
        .unwrap_or((args.source.as_str(), ""));
    if host.is_empty() {
        fail("a mount needs a host before the `:`");
    }
    let alias = host.rsplit('@').next().unwrap_or(host);
    let settings = crate::settings::load();
    let default = format!("{}/{alias}", settings.mount_root.trim_end_matches('/'));
    let (sudo, server) = match args.sudo {
        None => (Sudo::No, String::new()),
        Some(s) if s.is_empty() => (Sudo::NoPasswd, settings.sftp_server.clone()),
        Some(s) => (Sudo::NoPasswd, s),
    };
    let spec = MountSpec {
        host: host.to_string(),
        remote: mount_spec::remote_path(path).to_string(),
        local: mount_spec::mount_point(&default, args.local.as_deref().unwrap_or("")),
        sudo,
        server,
        forced_command: sshcfg::forces_command(alias),
    };
    if let Some(problem) = spec.problem() {
        fail(&problem);
    }
    let on = mounts::entries()
        .iter()
        .any(|m| m.on && same_folder(&m.local, &spec.local));
    start(spec, on, true, args.dry.dry_run);
}

/// Make the folder, keep the line when it is new, then mount. The line is
/// written before the mount is tried, so one that fails is still there to try
/// again.
fn start(mut spec: MountSpec, on: bool, keep: bool, dry: bool) {
    let shown = collapse_tilde(&spec.local);
    if on {
        step(&format!("already on: {shown}"));
        return;
    }
    if !mounts::sshfs_installed() {
        fail(
            "sshfs is not installed (apt install sshfs · pacman -S sshfs · dnf install fuse-sshfs)",
        );
    }
    step(&format!("mkdir -p {shown}"));
    if !dry {
        if let Err(e) = fs::create_dir_all(&spec.local) {
            fail(&format!(
                "cannot create {shown}: {}, so pass another LOCAL",
                e.kind()
            ));
        }
        // `/proc/mounts` lists it absolute with symlinks resolved, and the kept
        // line is matched against that.
        if let Ok(real) = fs::canonicalize(&spec.local) {
            spec.local = real.to_string_lossy().into_owned();
        }
    }
    if keep {
        let saved = spec.saved();
        let sudo = match &saved.sudo {
            Some(server) => format!(" (sudo {server})"),
            None => String::new(),
        };
        step(&format!(
            "keep {} <- {}{sudo}",
            collapse_tilde(&saved.local),
            saved.remote
        ));
        if !dry && let Err(e) = mounts::remember(&saved) {
            fail(&format!("could not keep it: {e:#}"));
        }
    }
    let argv = spec.argv();
    let shown_argv: Vec<String> = argv.iter().map(|a| collapse_tilde(a)).collect();
    step(&shell_join(&shown_argv));
    if dry {
        return;
    }
    match run_tool(&argv) {
        Ok(0) => {}
        Ok(code) => fail(&format!(
            "sshfs exited {code}, so {shown} is not mounted; the line is kept for `essh mount on`"
        )),
        Err(e) => fail(&e),
    }
}

fn stop(m: &Mount, lazy: bool, dry: bool) {
    if !m.on {
        return;
    }
    let shown = collapse_tilde(&m.local);
    step(&if lazy {
        format!("fusermount -u -z {shown}")
    } else {
        format!("fusermount -u {shown}")
    });
    if dry {
        return;
    }
    let done = if lazy {
        mounts::unmount_lazy(&m.local)
    } else {
        mounts::unmount(&m.local)
    };
    if let Err(e) = done {
        fail(&format!(
            "{shown}: {e}\n  something still uses it (a shell inside it, an open file): `essh mount off --lazy {shown}` detaches it now"
        ));
    }
    // `remove_dir` only removes an empty folder, so one that had files of its
    // own under the mount is left alone.
    let _ = fs::remove_dir(&m.local);
}

/// The same folder however it was typed: `~/x`, an absolute path, or one
/// through a symlink.
fn same_folder(kept: &str, typed: &str) -> bool {
    let typed = sshcfg::expand_tilde(typed.trim_end_matches('/'));
    let typed = fs::canonicalize(&typed).unwrap_or(typed);
    kept == typed.to_string_lossy()
}

fn pick(local: Option<&str>, all: bool) -> Vec<Mount> {
    let rows = mounts::entries();
    if all {
        return rows;
    }
    let local = local.unwrap_or_default();
    let found: Vec<Mount> = rows
        .into_iter()
        .filter(|m| same_folder(&m.local, local))
        .collect();
    if found.is_empty() {
        fail(&format!(
            "no kept mount at `{local}`: `essh mount ls` lists them by local folder"
        ));
    }
    found
}
