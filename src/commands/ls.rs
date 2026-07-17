//! `essh ls` - every host in your config, one per line. This is your old
//! `grep '^Host ' ~/.ssh/config | grep -v '*' | awk '{print $2}'` alias, built
//! in, so you never have to remember the names again.

use crate::sshcfg;
use colored::Colorize;

#[derive(clap::Args)]
pub struct Args {
    /// Also show where each alias connects (user@hostname:port)
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn run(args: Args) {
    let hosts = sshcfg::list_hosts();

    if hosts.is_empty() {
        println!(
            "{}",
            "No hosts in ~/.ssh/config yet. Run `essh` and press `n` to add one.".dimmed()
        );
        return;
    }

    if args.verbose {
        // Pad the alias column so the targets line up.
        let width = hosts.iter().map(|h| h.alias.len()).max().unwrap_or(0);
        for h in &hosts {
            // Show the key too when the host pins one - handy for "which key is this?".
            let key = match &h.identity {
                Some(i) => format!("  ({i})"),
                None => String::new(),
            };
            println!("  {:width$}  {}{}", h.alias.bold(), h.target().dimmed(), key.dimmed());
        }
    } else {
        for h in &hosts {
            println!("{}", h.alias);
        }
    }
}
