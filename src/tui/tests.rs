//! The TUI's render and keymap tests, driven through ratatui's `TestBackend`.

use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::mount_spec::{MountSpec, mount_point};
use super::picker::{Picker, PickerAction};
use super::prompt::{Action, Kind};
use super::*;

/// Render each view + a wizard against a headless backend and dump the
/// buffer to a string, so we can assert the layout draws real content
/// without needing an actual terminal.
fn render(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|f| ui(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    buf.content().iter().map(|c| c.symbol()).collect()
}

#[test]
fn renders_chrome_and_wizards() {
    let mut app = App::empty();

    // Tab chrome is always present.
    let hosts = render(&mut app);
    assert!(hosts.contains("easyssh"), "title bar missing");
    assert!(
        hosts.contains("Hosts") && hosts.contains("Mounts"),
        "tabs missing"
    );

    // The add-host wizard overlays its fields.
    app.prompt = Some(Prompt::add_host());
    let add = render(&mut app);
    assert!(add.contains("add host"), "add-host title missing");
    assert!(add.contains("Alias"), "alias field missing");
    app.prompt = None;

    // The forward wizard builds off a host name.
    app.prompt = Some(Prompt::forward("box".into()));
    let fwd = render(&mut app);
    assert!(fwd.contains("Remote port"), "forward field missing");

    // Switching to the Tunnels view renders its (possibly empty) body.
    app.view = View::Tunnels;
    app.prompt = None;
    let tun = render(&mut app);
    assert!(tun.contains("Tunnels"), "tunnels view missing");
}

#[test]
fn tab_cycles_views() {
    let mut app = App::empty();
    assert!(matches!(app.view, View::Hosts));
    app.cycle_view(1);
    assert!(matches!(app.view, View::Keys));
    app.cycle_view(-1);
    assert!(matches!(app.view, View::Hosts));
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

#[test]
fn vim_keys_switch_views() {
    let mut app = App::empty();
    app.on_key(press(KeyCode::Char('l'))); // → next view
    assert!(matches!(app.view, View::Keys));
    app.on_key(press(KeyCode::Char('h'))); // ← prev view
    assert!(matches!(app.view, View::Hosts));
}

#[test]
fn c_creates_in_every_view() {
    let mut app = App::empty();
    // Hosts: `c` opens the add-host wizard.
    app.on_key(press(KeyCode::Char('c')));
    assert!(matches!(
        app.prompt.as_ref().map(|p| &p.action),
        Some(Action::AddHost)
    ));
    app.prompt = None;

    // Keys: the same `c` starts the new-key flow, asking the type first.
    app.view = View::Keys;
    app.on_key(press(KeyCode::Char('c')));
    assert!(matches!(
        app.picker.as_ref().map(|p| &p.action),
        Some(PickerAction::NewKeyType)
    ));

    // Picking the first entry (ed25519) opens the name/comment wizard.
    app.on_key(press(KeyCode::Enter));
    match app.prompt.as_ref().map(|p| &p.action) {
        Some(Action::NewKey { kind }) => assert_eq!(kind, "ed25519"),
        _ => panic!("expected the new-key wizard"),
    }
}

#[test]
fn new_key_still_offers_rsa_for_old_servers() {
    let mut app = App::empty();
    app.view = View::Keys;
    app.on_key(press(KeyCode::Char('c')));
    app.on_key(press(KeyCode::Char('j'))); // down to the rsa entry
    app.on_key(press(KeyCode::Enter));

    match app.prompt.as_ref().map(|p| &p.action) {
        Some(Action::NewKey { kind }) => assert_eq!(kind, "rsa"),
        _ => panic!("expected the rsa wizard"),
    }
    // The suggested filename follows the chosen type.
    assert_eq!(app.prompt.as_ref().unwrap().fields[0].default, "id_rsa");
}

#[test]
fn form_typing_vs_field_nav() {
    let mut app = App::empty();
    app.prompt = Some(Prompt::add_host()); // six fields, idx 0

    // Plain h/j/k/l are literal text in a form.
    for c in ['h', 'j', 'k', 'l'] {
        app.on_key(press(KeyCode::Char(c)));
    }
    assert_eq!(app.prompt.as_ref().unwrap().fields[0].value, "hjkl");
    assert_eq!(
        app.prompt.as_ref().unwrap().idx,
        0,
        "typing must not move fields"
    );

    // Ctrl-j / Ctrl-k move between fields.
    app.on_key(ctrl(KeyCode::Char('j')));
    assert_eq!(app.prompt.as_ref().unwrap().idx, 1);
    app.on_key(ctrl(KeyCode::Char('j')));
    assert_eq!(app.prompt.as_ref().unwrap().idx, 2);
    app.on_key(ctrl(KeyCode::Char('k')));
    assert_eq!(app.prompt.as_ref().unwrap().idx, 1);

    // Ctrl-c bails out of the wizard.
    app.on_key(ctrl(KeyCode::Char('c')));
    assert!(app.prompt.is_none());
}

/// An app with one selected host, built without touching disk.
fn app_with_host() -> App {
    let mut app = App::empty();
    app.hosts = vec![Host {
        alias: "raspi".into(),
        hostname: Some("10.0.0.1".into()),
        user: Some("pi".into()),
        port: None,
        identity: None,
        proxy_jump: None,
    }];
    app.host_state.select(Some(0));
    app
}

#[test]
fn e_opens_edit_prefilled() {
    let mut app = app_with_host();
    app.on_key(press(KeyCode::Char('e')));
    let p = app.prompt.as_ref().expect("edit wizard should open");
    assert!(matches!(p.action, Action::EditHost { .. }));
    assert_eq!(p.fields[0].value, "raspi", "alias pre-filled");
    assert_eq!(p.fields[1].value, "10.0.0.1", "hostname pre-filled");
}

#[test]
fn d_gates_delete_behind_confirm() {
    let mut app = app_with_host();
    app.on_key(press(KeyCode::Char('d')));
    assert!(app.confirm.is_some(), "delete must ask first");
    app.on_key(press(KeyCode::Char('n'))); // decline
    assert!(app.confirm.is_none(), "declining clears the gate");
    // The host is still there (we never confirmed).
    assert_eq!(app.hosts.len(), 1);
}

#[test]
fn confirm_starts_on_no_so_enter_cannot_fire_it() {
    let mut app = app_with_host();
    app.on_key(press(KeyCode::Char('d')));
    app.on_key(press(KeyCode::Enter)); // reflex Enter must not delete
    assert!(app.confirm.is_none(), "Enter still closes the gate");
    assert_eq!(app.hosts.len(), 1, "Enter on the default must not delete");
}

#[test]
fn confirm_arrows_move_between_the_buttons() {
    let mut app = app_with_host();
    app.on_key(press(KeyCode::Char('d')));
    assert!(!app.confirm.as_ref().unwrap().yes, "starts on No");
    app.on_key(press(KeyCode::Left));
    assert!(app.confirm.as_ref().unwrap().yes, "arrow selects Yes");
    app.on_key(press(KeyCode::Char('l')));
    assert!(!app.confirm.as_ref().unwrap().yes, "and back to No");
}

#[test]
fn confirm_ignores_stray_keys() {
    // Anything unbound used to cancel, so brushing a key silently dropped the
    // question - including the one offering the lazy-unmount fix.
    let mut app = app_with_host();
    app.on_key(press(KeyCode::Char('d')));
    app.on_key(press(KeyCode::Char('z')));
    app.on_key(press(KeyCode::Down));
    assert!(app.confirm.is_some(), "stray keys must leave the modal up");
}

#[test]
fn y_picks_a_host_instead_of_typing_one() {
    let mut app = app_with_host();
    app.keys = vec![keys::Key {
        path: "/home/u/.ssh/id_ed25519".into(),
        kind: "ED25519".into(),
        bits: "256".into(),
        comment: String::new(),
        fingerprint: String::new(),
        agent_loaded: false,
        encrypted: false,
        used_by: Vec::new(),
    }];
    app.key_state.select(Some(0));
    app.view = View::Keys;

    // `y` = yank/copy (vim); `c` is create here, not copy, since vim's `c` is "change".
    app.on_key(press(KeyCode::Char('y')));
    let p = app.picker.as_ref().expect("host picker should open");
    assert_eq!(
        p.items,
        vec!["raspi".to_string()],
        "picker lists the config hosts"
    );
    assert!(
        app.prompt.is_none(),
        "must not make the user type an alias we already know"
    );
}

#[test]
fn n_is_not_a_create_key() {
    // Create is `c` (tmux); `n` was dropped, so it must do nothing.
    let mut app = App::empty();
    app.on_key(press(KeyCode::Char('n')));
    assert!(app.prompt.is_none(), "`n` must not create anything");
}

#[test]
fn status_expires_after_its_ttl() {
    let mut app = App::empty();
    app.set_status("connected");
    assert_eq!(app.live_status(), Some("connected"));

    // Backdate it past the TTL: a stale message must stop showing.
    app.status_at = Instant::now().checked_sub(STATUS_TTL * 2);
    assert!(app.live_status().is_none());
}

#[test]
fn keygen_command_preview_tracks_fields() {
    let mut p = Prompt::new_key("ed25519");
    p.fields[0].value = "work".into();
    p.fields[1].value = "me@x".into();
    assert_eq!(
        p.command_preview().unwrap(),
        "ssh-keygen -t ed25519 -f ~/.ssh/work -C me@x"
    );

    // RSA always gets -b 4096; a blank name falls back to id_<kind>.
    let rsa = Prompt::new_key("rsa");
    assert_eq!(
        rsa.command_preview().unwrap(),
        "ssh-keygen -t rsa -f ~/.ssh/id_rsa -b 4096"
    );

    // Config-writing wizards have no single command to preview.
    assert!(Prompt::add_host().command_preview().is_none());
}

#[test]
fn picker_fills_identityfile_field() {
    // A key chosen from the picker lands in the wizard's IdentityFile field,
    // so the user never has to type the path.
    let mut app = App::empty();
    app.prompt = Some(Prompt::add_host());
    let idx = app
        .prompt
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .position(|f| f.label.contains("IdentityFile"))
        .unwrap();
    app.picker = Some(Picker {
        title: "pick".into(),
        items: vec!["~/.ssh/id_ed25519".into()],
        idx: 0,
        action: PickerAction::FillField { field: idx },
    });
    app.on_key(press(KeyCode::Enter));
    assert!(app.picker.is_none());
    assert_eq!(
        app.prompt.as_ref().unwrap().fields[idx].value,
        "~/.ssh/id_ed25519"
    );
}

#[test]
fn mount_defaults_under_the_mount_folder() {
    // Mountpoints group under one folder so leftovers do not scatter, and the
    // default is absolute so it does not depend on where essh was launched.
    let p = Prompt::mount("raspi".into(), &Settings::default());
    let mountpoint = p
        .fields
        .iter()
        .find(|f| f.label.contains("mountpoint"))
        .unwrap();
    assert_eq!(mountpoint.default, "~/sshfs/raspi");
}

#[test]
fn mount_point_expands_tilde() {
    // sshfs is spawned without a shell, so `~/dir` has to be expanded here or
    // both the mkdir and the mount land on a directory literally named `~`.
    // This is the resolver `submit_prompt` itself calls; the run path beyond it
    // (create_dir_all + spawn) needs sshfs installed and writes to $HOME, so it
    // is not covered.
    let home = dirs::home_dir().unwrap_or_default();
    assert_eq!(
        mount_point("~/sshfs/raspi", "~/exampledirectory"),
        home.join("exampledirectory").to_string_lossy()
    );
    assert_eq!(mount_point("~/sshfs/raspi", "~"), home.to_string_lossy());
    // Left blank, the field's own default applies - tilde expanded the same way.
    assert_eq!(
        mount_point("~/sshfs/raspi", ""),
        home.join("sshfs/raspi").to_string_lossy()
    );
    assert_eq!(mount_point("~/sshfs/raspi", "/mnt/raspi"), "/mnt/raspi");
}

/// A mount wizard with the rights choice already set.
fn mount_prompt(choice: usize) -> Prompt {
    let mut p = Prompt::mount("crusader".into(), &Settings::default());
    p.fields[2].choice = choice;
    p
}

#[test]
fn mount_without_sudo_is_a_plain_sshfs() {
    let spec = MountSpec::from_fields("crusader", &mount_prompt(0).fields);
    let local = mount_point("~/sshfs/crusader", "");
    assert_eq!(spec.argv(), vec!["sshfs", "crusader:", &local]);
}

#[test]
fn mount_with_nopasswd_sudo_wraps_the_server() {
    let spec = MountSpec::from_fields("crusader", &mount_prompt(1).fields);
    let local = mount_point("~/sshfs/crusader", "");
    assert_eq!(
        spec.argv(),
        vec![
            "sshfs",
            "-o",
            "sftp_server=sudo /usr/lib/openssh/sftp-server",
            "crusader:",
            &local,
        ]
    );
}

#[test]
fn mount_never_offers_to_send_a_password() {
    // Every way of feeding sudo a password from here leaks it into `ps` on
    // both machines, so the mode does not exist: NOPASSWD or nothing.
    let p = Prompt::mount("crusader".into(), &Settings::default());
    let options = match &p.fields[2].kind {
        Kind::Choice(o) => o.clone(),
        _ => panic!("the rights field must be a choice"),
    };
    assert_eq!(options.len(), 2, "only plain and NOPASSWD");
    assert!(!options.iter().any(|o| o.contains("password")));
    assert!(
        p.fields
            .iter()
            .all(|f| !f.label.to_lowercase().contains("password"))
    );
    for choice in 0..options.len() {
        let argv = MountSpec::from_fields("crusader", &mount_prompt(choice).fields).argv();
        assert!(
            !argv.iter().any(|a| a.contains("sudo -S")),
            "no password is ever piped to sudo"
        );
    }
}

#[test]
fn mount_refuses_a_comma_in_the_server_path() {
    // sshfs splits -o values on commas, so the option would be misparsed.
    let mut p = mount_prompt(1);
    p.fields[3].value = "/usr/lib,openssh/sftp-server".into();
    assert!(
        MountSpec::from_fields("crusader", &p.fields)
            .problem()
            .is_some()
    );
    assert!(
        MountSpec::from_fields("crusader", &mount_prompt(1).fields)
            .problem()
            .is_none()
    );
}

#[test]
fn mount_hides_the_server_path_until_sudo_is_picked() {
    let mut plain = mount_prompt(0);
    assert!(
        !plain.visible(3),
        "a plain mount never needs the server path"
    );
    plain.idx = 2;
    assert!(
        plain.on_last_field(),
        "so Enter on the rights field submits"
    );
    assert!(mount_prompt(1).visible(3), "NOPASSWD needs it");
}

#[test]
fn mount_rights_cycle_with_h_and_l() {
    let mut app = app_with_host();
    app.on_key(press(KeyCode::Char('m')));
    app.on_key(press(KeyCode::Tab));
    app.on_key(press(KeyCode::Tab)); // onto the rights choice
    assert_eq!(app.prompt.as_ref().unwrap().idx, 2);
    app.on_key(press(KeyCode::Char('l')));
    assert_eq!(app.prompt.as_ref().unwrap().fields[2].choice, 1);
    app.on_key(press(KeyCode::Char('h')));
    assert_eq!(app.prompt.as_ref().unwrap().fields[2].choice, 0);
    app.on_key(press(KeyCode::Left)); // wraps back round
    assert_eq!(app.prompt.as_ref().unwrap().fields[2].choice, 1);
    // h/l are the answer here, never typed text.
    assert!(app.prompt.as_ref().unwrap().fields[2].value.is_empty());
}

#[test]
fn mount_tab_skips_the_hidden_fields() {
    let mut app = app_with_host();
    app.on_key(press(KeyCode::Char('m')));
    for _ in 0..2 {
        app.on_key(press(KeyCode::Tab));
    }
    assert_eq!(app.prompt.as_ref().unwrap().idx, 2, "on the rights choice");
    app.on_key(press(KeyCode::Tab)); // no sudo → wraps past the hidden field
    assert_eq!(app.prompt.as_ref().unwrap().idx, 0);
}

#[test]
fn mount_preview_matches_what_runs() {
    // The preview must resolve the mountpoint exactly like the run path, so it
    // goes through the same `mount_point`.
    let mut p = Prompt::mount("raspi".into(), &Settings::default());
    let idx = p
        .fields
        .iter()
        .position(|f| f.label.contains("mountpoint"))
        .unwrap();
    p.fields[idx].value = "~/exampledirectory".into();
    // Same resolution as the run path, shown home-relative the way you would
    // have typed it.
    assert_eq!(
        p.command_preview().unwrap(),
        "sshfs raspi: ~/exampledirectory"
    );
    assert_eq!(
        sshcfg::expand_tilde("~/exampledirectory").to_string_lossy(),
        mount_point("~/sshfs/raspi", "~/exampledirectory"),
        "and what actually runs is still the absolute path"
    );
}

#[test]
fn mounts_tab_renders() {
    let mut app = App::empty();
    app.view = View::Mounts;
    assert!(render(&mut app).contains("Mounts"));
}

#[test]
fn slash_filters_the_list_and_esc_restores_it() {
    // The README promises `/` filters the list you are on.
    let mut app = App::empty();
    app.hosts = ["web01", "web02", "raspi"]
        .iter()
        .map(|a| Host {
            alias: (*a).into(),
            hostname: Some("10.0.0.1".into()),
            user: None,
            port: None,
            identity: None,
            proxy_jump: None,
        })
        .collect();
    app.host_state.select(Some(0));

    app.on_key(press(KeyCode::Char('/')));
    for c in "web".chars() {
        app.on_key(press(KeyCode::Char(c)));
    }
    assert_eq!(app.host_rows().len(), 2, "only the web hosts survive");
    // Enter keeps the filter and hands the keys back to the list.
    app.on_key(press(KeyCode::Enter));
    assert!(!app.searching);
    assert_eq!(app.host_rows().len(), 2);

    app.on_key(press(KeyCode::Esc));
    assert_eq!(app.host_rows().len(), 3, "Esc puts every row back");
}

#[test]
fn a_filtered_selection_acts_on_the_row_you_can_see() {
    // A selection indexes the filtered rows; indexing the raw list would edit
    // whichever host happened to sit at that position.
    let mut app = App::empty();
    app.hosts = ["alpha", "beta", "gamma"]
        .iter()
        .map(|a| Host {
            alias: (*a).into(),
            hostname: None,
            user: None,
            port: None,
            identity: None,
            proxy_jump: None,
        })
        .collect();
    app.host_state.select(Some(0));
    app.query = "gamma".into();
    app.requery();
    assert_eq!(app.selected_host().unwrap().alias, "gamma");
}

#[test]
fn a_filter_does_not_follow_you_to_the_next_tab() {
    let mut app = App::empty();
    app.on_key(press(KeyCode::Char('/')));
    app.on_key(press(KeyCode::Char('x')));
    app.on_key(press(KeyCode::Enter));
    assert_eq!(app.query, "x");
    app.cycle_view(1);
    assert!(
        app.query.is_empty(),
        "the query belongs to the list you typed it over"
    );
}

#[test]
fn filter_matches_substrings_and_subsequences() {
    assert!(
        filter::matches("", &["anything"]),
        "no query keeps every row"
    );
    assert!(filter::matches("WEB", &["web01"]), "case does not matter");
    assert!(filter::matches("wb1", &["web-01"]), "gaps are allowed");
    assert!(
        filter::matches("pi", &["raspi", "other"]),
        "any field can match"
    );
    assert!(!filter::matches("zzz", &["web01"]));
}

#[test]
fn the_mount_folder_setting_decides_where_a_mount_lands() {
    // Settings promises the mount folder is where a mount lands; the wizard has
    // to offer it, and the command that runs has to use what the wizard offered.
    let mut s = Settings::default();
    s.set("mount_root", "/mnt");
    let p = Prompt::mount("raspi".into(), &s);
    let field = p
        .fields
        .iter()
        .find(|f| f.label.contains("mountpoint"))
        .unwrap();
    assert_eq!(field.default, "/mnt/raspi");
    assert_eq!(
        MountSpec::from_fields("raspi", &p.fields).local,
        "/mnt/raspi",
        "a blank field runs what the box offered"
    );
}

#[test]
fn a_connect_run_says_so_rather_than_being_recognised_by_name() {
    // History and the changed-host-key probe both key off this, and `ssh` stops
    // being argv[0] the moment the ssh_command setting names a wrapper.
    let mut app = app_with_host();
    app.settings.set("ssh_command", "kitten ssh");
    let run = app.on_key(press(KeyCode::Enter)).expect("Enter connects");
    assert_eq!(run.argv, vec!["kitten", "ssh", "raspi"]);
    assert_eq!(run.connect.as_deref(), Some("raspi"));

    // Everything else is not a connect, so it never stamps the history.
    let mut app = App::empty();
    app.view = View::Keys;
    app.on_key(press(KeyCode::Char('c')));
    let run = app.on_key(press(KeyCode::Enter)); // ed25519 -> the keygen wizard
    assert!(run.is_none());
}

#[test]
fn settings_tab_renders_its_two_groups() {
    // Deliberately no key presses: every change in that tab saves at once, and
    // a test must never write the real ~/.config/easyssh/settings.
    let mut app = App::empty();
    app.view = View::Settings;
    app.settings_state.select(Some(0));
    let screen = render(&mut app);
    assert!(screen.contains("Settings"), "the tab is drawn");
    assert!(screen.contains("behaviour"), "grouped by what it promises");
    assert!(screen.contains("defaults"));
}

#[test]
fn a_new_tunnel_is_the_selected_one() {
    // It lands at the bottom of a list you did not make, so selecting it is the
    // app's job rather than three keypresses.
    let mut app = App::empty();
    app.view = View::Tunnels;
    app.tunnels = [111, 222, 333]
        .iter()
        .map(|pid| tunnels::Tunnel {
            pid: *pid,
            kind: 'L',
            spec: "8080:localhost:80".into(),
            host: "web01".into(),
            log: "/nonexistent".into(),
        })
        .collect();
    app.tunnel_state.select(Some(0));

    app.select_tunnel(333);
    assert_eq!(app.selected_tunnel().unwrap().pid, 333);
}

#[test]
fn a_new_mount_is_the_selected_one_once_it_exists() {
    // The mount runs suspended, so it does not exist when the wizard hands the
    // command over; the selection has to wait for the next refresh.
    let mut app = App::empty();
    app.view = View::Mounts;
    app.new_mount = Some("/home/u/sshfs/raspi".into());
    app.mounts = ["/home/u/sshfs/nas", "/home/u/sshfs/raspi"]
        .iter()
        .map(|local| mounts::Mount {
            remote: "x:".into(),
            local: (*local).into(),
            options: "rw".into(),
        })
        .collect();
    app.mount_state.select(Some(0));

    app.settle_new_mount();
    assert_eq!(app.selected_mount().unwrap().local, "/home/u/sshfs/raspi");
    assert!(app.new_mount.is_none(), "the hint is used once");
}

#[test]
fn jumping_to_a_view_drops_the_filter_that_led_there() {
    // Otherwise a mount made from a filtered Hosts list lands on a Mounts tab
    // whose filter can hide the very mount that was just made.
    let mut app = App::empty();
    app.query = "deploy".into();
    app.goto_view(View::Mounts);
    assert!(app.query.is_empty());
    assert!(matches!(app.view, View::Mounts));
}
