//! `essh <host> [ssh args…]` - just connect. Anything that isn't a known
//! subcommand is treated as an ssh destination and handed straight to `ssh`,
//! so all your config aliases, agent auth and extra flags keep working exactly
//! as before. We `exec` (replace this process) so ssh owns the terminal cleanly.

use colored::Colorize;

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        eprintln!("{}", "essh: no host given. Try `essh ls` or just `essh`.".red());
        std::process::exit(2);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // exec never returns on success; ssh takes over this PID and terminal.
        let err = std::process::Command::new("ssh").args(&args).exec();
        eprintln!("{}", format!("essh: could not run ssh: {err}").red());
        std::process::exit(127);
    }

    #[cfg(not(unix))]
    {
        match std::process::Command::new("ssh").args(&args).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("{}", format!("essh: could not run ssh: {e}").red());
                std::process::exit(127);
            }
        }
    }
}
