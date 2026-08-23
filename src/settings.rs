//! The handful of choices that are yours rather than ssh's, kept in
//! `~/.config/easyssh/settings` as `key = value` lines you can read and edit by
//! hand. Nothing here changes what ssh does; it changes what easyssh assumes
//! before you type anything.
//!
//! Every setting has a default that works with no file at all, so a fresh
//! install writes nothing until you change something. An unknown key is left
//! alone rather than dropped, so an older build never eats a newer one's file.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// How the Hosts tab is ordered.
#[derive(Clone, Copy, PartialEq)]
pub enum HostOrder {
    /// Most recently connected first: the box you were on is the one you want.
    Recent,
    /// Plain alphabetical, the order `essh ls` always uses.
    Alpha,
}

pub struct Settings {
    /// Directory a mount lands under: the wizard offers `<root>/<host>`.
    pub mount_root: String,
    /// Whether to check each host's ssh port in the background.
    pub probe: bool,
    /// How long a probe waits for the TCP handshake, in seconds.
    pub probe_timeout: u64,
    pub host_order: HostOrder,
    /// What runs an interactive login. `ssh`, or a wrapper like `kitten ssh`.
    pub ssh_command: String,
    /// Where the sftp server lives on the *remote* side, for a sudo mount.
    pub sftp_server: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Absolute, so a mount always lands in the same place no matter
            // which directory essh was launched from.
            mount_root: "~/sshfs".into(),
            probe: true,
            probe_timeout: 2,
            host_order: HostOrder::Recent,
            ssh_command: "ssh".into(),
            sftp_server: "/usr/lib/openssh/sftp-server".into(),
        }
    }
}

/// One editable line in the Settings tab. `choices` is what makes it a cycled
/// answer rather than a typed one.
/// The two kinds of setting, which are not the same promise: one decides what
/// easyssh does, the other only decides what a wizard offers you before you
/// overwrite it.
#[derive(PartialEq, Clone, Copy)]
pub enum Group {
    /// Always in force.
    Behaviour,
    /// Pre-fills a wizard field; every use can still say otherwise.
    Default,
}

impl Group {
    pub fn label(&self) -> &'static str {
        match self {
            Group::Behaviour => "behaviour",
            Group::Default => "defaults",
        }
    }
}

pub struct Row {
    pub key: &'static str,
    pub group: Group,
    pub label: &'static str,
    /// What it changes, in the words of the thing it changes.
    pub help: &'static str,
    pub value: String,
    pub default: String,
    pub choices: Option<&'static [&'static str]>,
}

impl Row {
    pub fn is_default(&self) -> bool {
        self.value == self.default
    }
}

const PROBE_CHOICES: &[&str] = &["on", "off"];
const TIMEOUT_CHOICES: &[&str] = &["1", "2", "3", "5", "10"];
const ORDER_CHOICES: &[&str] = &["recent", "alphabetical"];

pub fn path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
        .join("easyssh")
        .join("settings")
}

pub fn load() -> Settings {
    load_from(&path())
}

/// Parse the file over the defaults. A missing file, an unreadable one or a
/// line that makes no sense all mean "keep the default", never an error: a
/// broken settings file must not stop you connecting to anything.
pub fn load_from(path: &std::path::Path) -> Settings {
    let mut s = Settings::default();
    let Ok(text) = fs::read_to_string(path) else {
        return s;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        s.set(key.trim(), value.trim());
    }
    s
}

impl Settings {
    /// Apply one `key = value`, ignoring anything we do not recognise.
    pub fn set(&mut self, key: &str, value: &str) {
        match key {
            "mount_root" => self.mount_root = non_empty(value, "~/sshfs"),
            "probe" => self.probe = value != "off",
            "probe_timeout" => {
                if let Ok(n) = value.parse::<u64>() {
                    self.probe_timeout = n.clamp(1, 60);
                }
            }
            "host_order" => {
                self.host_order = match value {
                    "alphabetical" | "alpha" => HostOrder::Alpha,
                    _ => HostOrder::Recent,
                }
            }
            "ssh_command" => self.ssh_command = non_empty(value, "ssh"),
            "sftp_server" => self.sftp_server = non_empty(value, "/usr/lib/openssh/sftp-server"),
            _ => {}
        }
    }

    /// Put one setting back the way it ships.
    pub fn reset(&mut self, key: &str) {
        let d = Settings::default();
        let value = d.rows().into_iter().find(|r| r.key == key).map(|r| r.value);
        if let Some(value) = value {
            self.set(key, &value);
        }
    }

    /// Move a cycled setting to its next (or previous) answer. A no-op on a
    /// typed setting, which is edited in a field instead.
    pub fn cycle(&mut self, key: &str, delta: i32) {
        let Some(row) = self.rows().into_iter().find(|r| r.key == key) else {
            return;
        };
        let Some(choices) = row.choices else {
            return;
        };
        let at = choices.iter().position(|c| *c == row.value).unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(choices.len() as i32) as usize;
        self.set(key, choices[next]);
    }

    /// Everything the Settings tab shows, in the order it shows it.
    pub fn rows(&self) -> Vec<Row> {
        let d = Settings::default();
        // Behaviour first, then the wizard defaults: what easyssh always does
        // reads differently from what it merely suggests.
        vec![
            Row {
                key: "probe",
                group: Group::Behaviour,
                label: "Check ports",
                help: "ask each host's ssh port whether it answers, for the up/down dot",
                value: on_off(self.probe).into(),
                default: on_off(d.probe).into(),
                choices: Some(PROBE_CHOICES),
            },
            Row {
                key: "probe_timeout",
                group: Group::Behaviour,
                label: "Check waits",
                help: "seconds a port check waits before calling a host down",
                value: self.probe_timeout.to_string(),
                default: d.probe_timeout.to_string(),
                choices: Some(TIMEOUT_CHOICES),
            },
            Row {
                key: "host_order",
                group: Group::Behaviour,
                label: "Host order",
                help: "recent = last connected first · alphabetical = like essh ls",
                value: match self.host_order {
                    HostOrder::Recent => "recent".into(),
                    HostOrder::Alpha => "alphabetical".into(),
                },
                default: "recent".into(),
                choices: Some(ORDER_CHOICES),
            },
            Row {
                key: "ssh_command",
                group: Group::Behaviour,
                label: "Connect with",
                help: "what runs a login: ssh, or a wrapper such as `kitten ssh`",
                value: self.ssh_command.clone(),
                default: d.ssh_command.clone(),
                choices: None,
            },
            Row {
                key: "mount_root",
                group: Group::Default,
                label: "Mount folder",
                help: "the mountpoint a mount wizard offers: <this>/<host>",
                value: self.mount_root.clone(),
                default: d.mount_root.clone(),
                choices: None,
            },
            Row {
                key: "sftp_server",
                group: Group::Default,
                label: "sftp-server",
                help: "the server-side path a sudo mount wizard offers",
                value: self.sftp_server.clone(),
                default: d.sftp_server.clone(),
                choices: None,
            },
        ]
    }

    /// The program and leading arguments an interactive login runs, so a
    /// wrapper like `kitten ssh` works without quoting rules of its own.
    pub fn ssh_argv(&self) -> Vec<String> {
        let words: Vec<String> = self
            .ssh_command
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if words.is_empty() {
            vec!["ssh".into()]
        } else {
            words
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&path())
    }

    /// Write every setting, defaults included, so the file doubles as the list
    /// of what can be changed.
    pub fn save_to(&self, path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut body = String::from(
            "# easyssh settings - `key = value`, one per line. Delete a line\n# to go back to its default. The Settings tab edits this file.\n\n",
        );
        for row in self.rows() {
            body.push_str(&format!("{} = {}\n", row.key, row.value));
        }
        fs::write(path, body).with_context(|| format!("writing {}", path.display()))
    }
}

fn on_off(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

fn non_empty(value: &str, fallback: &str) -> String {
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A throwaway settings file under the temp dir, uniquely named per test so
    /// the suite never touches the real `~/.config/easyssh/settings`.
    fn temp_file() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("easyssh-settings-{}-{stamp}", std::process::id()))
    }

    #[test]
    fn defaults_need_no_file() {
        // A fresh install has nothing on disk and must still work.
        let s = load_from(&temp_file());
        assert_eq!(s.mount_root, "~/sshfs");
        assert!(s.probe);
        assert_eq!(s.ssh_command, "ssh");
        assert!(matches!(s.host_order, HostOrder::Recent));
    }

    #[test]
    fn saved_settings_round_trip() {
        let path = temp_file();
        let mut s = Settings::default();
        s.set("mount_root", "/mnt");
        s.set("probe", "off");
        s.set("probe_timeout", "10");
        s.set("host_order", "alphabetical");
        s.set("ssh_command", "kitten ssh");
        s.save_to(&path).unwrap();

        let back = load_from(&path);
        assert_eq!(back.mount_root, "/mnt");
        assert!(!back.probe);
        assert_eq!(back.probe_timeout, 10);
        assert!(matches!(back.host_order, HostOrder::Alpha));
        assert_eq!(back.ssh_command, "kitten ssh");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_line_means_its_default() {
        // The README says deleting a line goes back to the default, so a file
        // holding one key must leave every other setting alone.
        let path = temp_file();
        fs::write(&path, "# a comment\nprobe = off\n").unwrap();
        let s = load_from(&path);
        assert!(!s.probe);
        assert_eq!(s.mount_root, "~/sshfs", "untouched keys keep their default");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn a_broken_file_never_costs_you_a_setting() {
        // A settings file must not be able to stop you connecting to anything.
        let path = temp_file();
        fs::write(&path, "garbage\nprobe_timeout = not a number\nnope = 1\n").unwrap();
        let s = load_from(&path);
        assert_eq!(s.probe_timeout, Settings::default().probe_timeout);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn cycling_walks_the_choices_and_wraps() {
        let mut s = Settings::default();
        let value = |s: &Settings| {
            s.rows()
                .into_iter()
                .find(|r| r.key == "probe_timeout")
                .unwrap()
                .value
        };
        assert_eq!(value(&s), "2");
        s.cycle("probe_timeout", 1);
        assert_eq!(value(&s), "3");
        s.cycle("probe_timeout", -1);
        assert_eq!(value(&s), "2");
        // Backwards off the front lands on the last choice, not out of range.
        s.cycle("probe_timeout", -1);
        s.cycle("probe_timeout", -1);
        assert_eq!(value(&s), "10");
    }

    #[test]
    fn reset_puts_one_setting_back() {
        let mut s = Settings::default();
        s.set("mount_root", "/mnt");
        s.set("probe", "off");
        s.reset("mount_root");
        assert_eq!(s.mount_root, "~/sshfs");
        assert!(!s.probe, "resetting one setting leaves the others alone");
    }

    #[test]
    fn a_wrapper_keeps_its_own_arguments() {
        // `kitten ssh <host>` has to reach the program as two words plus the
        // host, and an empty setting can never produce an empty command.
        let mut s = Settings::default();
        s.set("ssh_command", "kitten ssh");
        assert_eq!(s.ssh_argv(), vec!["kitten", "ssh"]);
        s.ssh_command = String::new();
        assert_eq!(s.ssh_argv(), vec!["ssh"]);
    }

    #[test]
    fn defaults_are_a_group_of_their_own() {
        // The tab promises two kinds: what is always in force, and what a wizard
        // merely offers. A setting in the wrong group misleads about that.
        let s = Settings::default();
        let group = |key: &str| s.rows().into_iter().find(|r| r.key == key).unwrap().group;
        assert!(group("mount_root") == Group::Default);
        assert!(group("sftp_server") == Group::Default);
        assert!(group("probe") == Group::Behaviour);
        assert!(group("ssh_command") == Group::Behaviour);
    }
}
