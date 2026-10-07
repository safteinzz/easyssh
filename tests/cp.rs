//! `essh cp` adds `-r` for a folder it cannot see, run against a stand-in
//! `scp` that records its arguments, so nothing leaves the machine.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
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

#[test]
fn copying_from_a_host_hands_scp_the_recursive_flag() {
    let home = Home::new("cp-remote");
    let bin = home.0.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let scp = bin.join("scp");
    std::fs::write(&scp, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$SCP_ARGS\"\n").unwrap();
    std::fs::set_permissions(&scp, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(home.0.join(".ssh")).unwrap();
    std::fs::write(
        home.0.join(".ssh/config"),
        "Host box\n    HostName 192.0.2.10\n",
    )
    .unwrap();
    let recorded = home.0.join("scp-args");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let out = Command::new(env!("CARGO_BIN_EXE_essh"))
        .args(["cp", "box:~/project", home.0.to_str().unwrap()])
        .env_clear()
        .env("HOME", &home.0)
        .env("XDG_CONFIG_HOME", home.0.join(".config"))
        .env("XDG_STATE_HOME", home.0.join(".local/state"))
        .env("PATH", path)
        .env("SCP_ARGS", &recorded)
        .output()
        .expect("essh runs");
    assert!(
        out.status.success(),
        "`essh cp` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let args = std::fs::read_to_string(&recorded).expect("the stand-in scp ran");
    assert!(
        args.lines().any(|a| a == "-r"),
        "a remote source may be a folder, so scp needs `-r`, got: {args:?}"
    );
}
