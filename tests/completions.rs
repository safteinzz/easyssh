//! `essh completions <shell> --add` writes the loader line once, into a
//! throwaway `HOME`.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("essh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp HOME");
        Home(dir)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn add(home: &Path, shell: &str) {
    let out = Command::new(env!("CARGO_BIN_EXE_essh"))
        .args(["completions", shell, "--add"])
        .env("HOME", home)
        .env_remove("COMPLETE")
        .output()
        .expect("essh runs");
    assert!(
        out.status.success(),
        "`essh completions {shell} --add` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn loader_lines(rc: &Path) -> usize {
    std::fs::read_to_string(rc)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("essh completions bash") || l.contains("COMPLETE=bash essh"))
        .count()
}

#[test]
fn adding_completion_twice_leaves_one_loader_line() {
    let home = Home::new("add-twice");
    let rc = home.0.join(".bashrc");
    std::fs::write(&rc, "alias ll='ls -l'\n").unwrap();
    add(&home.0, "bash");
    add(&home.0, "bash");
    assert_eq!(loader_lines(&rc), 1, "a second --add must not append again");
    assert!(
        std::fs::read_to_string(&rc)
            .unwrap()
            .starts_with("alias ll='ls -l'\n"),
        "the lines already there stay as they were"
    );
}

#[test]
fn an_earlier_complete_line_counts_as_already_set_up() {
    let home = Home::new("earlier-line");
    let rc = home.0.join(".bashrc");
    std::fs::write(&rc, "source <(COMPLETE=bash essh)\n").unwrap();
    add(&home.0, "bash");
    assert_eq!(
        loader_lines(&rc),
        1,
        "the README's earlier line already loads it"
    );
}
