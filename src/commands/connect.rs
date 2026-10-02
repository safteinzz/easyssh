//! `essh <host> [ssh args…]` - just connect. Anything that isn't a known
//! subcommand is treated as an ssh destination and handed straight to `ssh`,
//! so all your config aliases, agent auth and extra flags keep working exactly
//! as before. We `exec` (replace this process) so ssh owns the terminal cleanly,
//! except on a host with image paste on, where essh has to stay alive beside
//! ssh to serve the clipboard (`crate::paste`).

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
    let forward = match crate::paste::serve_for(&args[0]) {
        Ok(forward) => forward,
        Err(e) => {
            eprintln!(
                "{}",
                format!("essh: image paste is off for this login: {e:#}").yellow()
            );
            None
        }
    };
    let mut argv: Vec<String> = leading.iter().cloned().chain(args).collect();

    #[cfg(unix)]
    {
        if let Some(forward) = forward {
            forward.insert_into(&mut argv, leading.len());
            let code = run_beside(&program, &argv);
            drop(forward);
            std::process::exit(code);
        }
        use std::os::unix::process::CommandExt;
        // exec never returns on success; ssh takes over this PID and terminal.
        let err = std::process::Command::new(&program).args(&argv).exec();
        could_not_run(&program, &err);
    }

    #[cfg(not(unix))]
    {
        drop(forward);
        match std::process::Command::new(&program).args(&argv).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => could_not_run(&program, &e),
        }
    }
}

/// Run ssh as a child and return the exit code a shell would report for it,
/// `128 + signal` when a signal ended it.
///
/// Ctrl-C and Ctrl-\ reach every process on the terminal, and they are ssh's to
/// act on, so essh catches them and does nothing once ssh has started.
#[cfg(unix)]
fn run_beside(program: &str, argv: &[String]) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    let mut child = match std::process::Command::new(program).args(argv).spawn() {
        Ok(child) => child,
        Err(e) => could_not_run(program, &e),
    };
    swallow_interrupts();
    match child.wait() {
        Ok(status) => status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
        Err(e) => {
            eprintln!(
                "{}",
                format!("essh: lost track of {program}: {}", e.kind()).red()
            );
            1
        }
    }
}

/// Catch Ctrl-C and Ctrl-\ and do nothing with them, while a child that owns
/// the terminal acts on them. A caught signal is reset to the default in an
/// exec'd child, so this never reaches it, unlike `SIG_IGN`, which it would
/// inherit. Hand the ids to `signal_hook::low_level::unregister` to stop.
#[cfg(unix)]
pub(crate) fn swallow_interrupts() -> Vec<signal_hook::SigId> {
    use signal_hook::consts::{SIGINT, SIGQUIT};
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    [SIGINT, SIGQUIT]
        .into_iter()
        .filter_map(|signal| signal_hook::flag::register(signal, seen.clone()).ok())
        .collect()
}

fn could_not_run(program: &str, err: &std::io::Error) -> ! {
    let why = match err.kind() {
        std::io::ErrorKind::NotFound => "it is not installed or not on PATH".to_string(),
        kind => kind.to_string(),
    };
    eprintln!(
        "{}",
        format!("essh: could not run `{program}`: {why}").red()
    );
    std::process::exit(127);
}
