//! Every listing's `--json` keeps the field names `essh --help` promises are
//! fixed, run against a throwaway `HOME` holding one of each thing.

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

/// `essh <args>` with every place it reads or writes inside `home`.
fn essh(home: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_essh"))
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .expect("essh runs");
    assert!(
        out.status.success(),
        "`essh {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

fn assert_fields(command: &str, json: &str, fields: &[&str]) {
    assert!(
        json.trim_start().starts_with('[') && json.contains('{'),
        "`{command}` should print a non-empty JSON array, got: {json}"
    );
    for field in fields {
        assert!(
            json.contains(&format!("\"{field}\": ")),
            "`{command}` lost the fixed field `{field}`: {json}"
        );
    }
}

#[test]
fn every_listing_keeps_its_json_field_names() {
    let home = Home::new("json-fields");
    essh(
        &home.0,
        &[
            "host",
            "add",
            "box",
            "--hostname",
            "198.51.100.7",
            "--user",
            "deploy",
        ],
    );
    essh(&home.0, &["key", "new", "id_test", "--no-passphrase"]);
    let config = home.0.join(".config/easyssh");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("tunnels"), "-D 1080 box = router\n").unwrap();
    std::fs::write(config.join("mounts"), "~/sshfs/box <- box:\n").unwrap();

    assert_fields(
        "essh ls --json",
        &essh(&home.0, &["ls", "--json"]),
        &[
            "alias",
            "hostname",
            "user",
            "port",
            "key",
            "jump",
            "command",
            "forward_agent",
        ],
    );
    assert_fields(
        "essh tunnel ls --json",
        &essh(&home.0, &["tunnel", "ls", "--json"]),
        &["name", "flag", "spec", "host", "state", "pid", "command"],
    );
    assert_fields(
        "essh mount ls --json",
        &essh(&home.0, &["mount", "ls", "--json"]),
        &["local", "remote", "host", "state", "sudo"],
    );
    assert_fields(
        "essh key ls --json",
        &essh(&home.0, &["key", "ls", "--json"]),
        &[
            "name",
            "path",
            "type",
            "bits",
            "fingerprint",
            "comment",
            "agent",
            "passphrase",
            "used_by",
        ],
    );
}
