//! The CLI: connect, list and copy, plus one noun per TUI tab (`host`,
//! `tunnel`, `mount`, `key`) and `check`, so a script or an agent can do what
//! the toolbox does without driving a terminal.

pub mod check;
pub mod completions;
pub mod connect;
pub mod cp;
pub mod host;
pub mod key;
pub mod ls;
pub mod mount;
pub mod selfcmd;
pub mod tunnel;

use colored::Colorize;
use std::process::{Command, Stdio};

/// `-n`, shared by every command that changes something.
#[derive(clap::Args)]
pub struct DryRun {
    /// Dry run: print the steps it would take and change nothing
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,
}

/// `--json`, shared by every listing.
#[derive(clap::Args)]
pub struct Json {
    /// Print it for a script: a JSON array whose field names are fixed
    #[arg(long)]
    pub json: bool,
}

/// One step of a change, on stdout. A dry run prints the same lines and stops
/// there, so its output is the real run's line for line.
pub fn step(line: &str) {
    println!("{line}");
}

/// Our own words for a failure, on stderr, then exit 1.
pub fn fail(msg: &str) -> ! {
    eprintln!("{}", format!("essh: {msg}").red());
    std::process::exit(1);
}

/// A warning that does not stop the command.
pub fn warn(msg: &str) {
    eprintln!("{}", format!("essh: {msg}").yellow());
}

/// Run a tool that may ask on the terminal (ssh, sshfs, ssh-keygen), with its
/// stdout sent to our stderr so stdout stays the command's own output.
pub fn run_tool(argv: &[String]) -> Result<i32, String> {
    let (program, args) = argv.split_first().expect("argv is never empty");
    Command::new(program)
        .args(args)
        .stdout(Stdio::from(std::io::stderr()))
        .status()
        .map(|s| s.code().unwrap_or(1))
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("`{program}` is not installed"),
            kind => format!("could not run `{program}`: {kind}"),
        })
}

/// A JSON string literal.
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A JSON string, or `null` for a value that is not set.
pub fn json_opt(s: Option<&str>) -> String {
    s.map_or_else(|| "null".into(), json_str)
}

/// One object per row, one row per line, so a diff of two runs reads.
pub fn json_array(rows: &[String]) -> String {
    if rows.is_empty() {
        return "[]".into();
    }
    format!("[\n  {}\n]", rows.join(",\n  "))
}
