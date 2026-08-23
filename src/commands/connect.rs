//! `essh <host> [ssh args…]` - just connect. Anything that isn't a known
//! subcommand is treated as an ssh destination and handed straight to `ssh`,
//! so all your config aliases, agent auth and extra flags keep working exactly
//! as before. We `exec` (replace this process) so ssh owns the terminal cleanly.

use colored::Colorize;

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        eprintln!(
            "{}",
            "essh: no host given. Try `essh ls` or just `essh`.".red()
        );
        std::process::exit(2);
    }

    // Stamp the connect before handing the terminal over: `exec` never comes
    // back, so there is no "after" in which to record it. The first argument is
    // the destination; the rest are ssh's own flags.
    crate::history::record(&args[0]);

    // The same launcher the TUI uses: plain `ssh`, or whatever wrapper Settings
    // names, so both ways in behave identically.
    let launcher = crate::settings::load().ssh_argv();
    let (program, leading) = launcher.split_first().expect("ssh_argv is never empty");
    let program = program.clone();
    let argv: Vec<String> = leading.iter().cloned().chain(args).collect();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // exec never returns on success; ssh takes over this PID and terminal.
        let err = std::process::Command::new(&program).args(&argv).exec();
        eprintln!("{}", format!("essh: could not run {program}: {err}").red());
        std::process::exit(127);
    }

    #[cfg(not(unix))]
    {
        match std::process::Command::new(&program).args(&argv).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("{}", format!("essh: could not run {program}: {e}").red());
                std::process::exit(127);
            }
        }
    }
}
