use super::*;

fn paths(root: &Path, macos: bool) -> Paths {
    Paths {
        home: root.join("home"),
        data: root.join("data/mudel-ai-connect"),
        local: root.join("local"),
        roaming: root.join("roaming"),
        macos,
    }
}

#[test]
fn detects_current_claude_folders_and_both_mac_and_windows_clients() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = paths(temporary.path(), false);
    fs::create_dir_all(paths.local.join("Claude-Data/claude_profile")).unwrap();
    fs::create_dir_all(paths.roaming.join("Claude")).unwrap();
    fs::create_dir_all(paths.home.join(".codex")).unwrap();
    fs::write(
        paths.home.join(".codex/config.toml"),
        "[plugins.\"mudel-ai-connect@local\"]\nenabled = true\n",
    )
    .unwrap();
    fs::write(paths.home.join(".claude.json"), "{}").unwrap();
    let clients = paths.clients();
    assert_eq!(clients.len(), 3);
    assert!(clients[0].path.starts_with(paths.local.join("Claude-Data")));
    assert!(
        !clients
            .iter()
            .find(|client| client.key == "codex")
            .unwrap()
            .selected
    );
    assert_eq!(paths.desktop_dirs().1.len(), 1);
    let mac = self::paths(temporary.path(), true);
    fs::create_dir_all(mac.home.join("Library/Application Support/Claude")).unwrap();
    assert!(
        mac.clients()[0]
            .path
            .starts_with(mac.home.join("Library/Application Support/Claude"))
    );
}

#[test]
fn registration_preserves_other_servers_settings_comments_and_quoted_toml() {
    let command = Path::new("C:/Prueba con espacios/servidor.exe");
    let json = r#"{"theme":"light","mcpServers":{"other":{"command":"keep"},"moodle-ai-connect":{"command":"old"}}}"#;
    let result = configuration(json, Format::Json { stdio: true }, Some(command)).unwrap();
    let value: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["theme"], "light");
    assert_eq!(value["mcpServers"]["other"]["command"], "keep");
    assert!(value["mcpServers"].get(OLD_NAME).is_none());
    assert_eq!(value["mcpServers"][NAME]["args"], json!([]));
    assert_eq!(value["mcpServers"][NAME]["type"], "stdio");
    let removed = configuration(&result, Format::Json { stdio: true }, None).unwrap();
    assert!(
        configured_command(&removed, Format::Json { stdio: true })
            .unwrap()
            .is_none()
    );
    for text in [
        "# keep comment\nmodel = 'keep'\n[mcp_servers.other]\ncommand = 'keep'\n[mcp_servers.'mudel-ai-connect']\ncommand = 'old'\n[mcp_servers.'mudel-ai-connect'.env]\nOLD = 'secret'\n[plugins.'keep@local']\nenabled = true\n",
        "# keep comment\nmcp_servers = { other = { command = 'keep' }, 'mudel-ai-connect' = { command = 'old' } }\n",
    ] {
        let result = configuration(text, Format::Toml, Some(command)).unwrap();
        assert!(result.contains("# keep comment"));
        assert_eq!(
            configured_command(&result, Format::Toml).unwrap(),
            Some(command.to_string_lossy().into())
        );
        let doc = result.parse::<DocumentMut>().unwrap();
        assert_eq!(
            doc["mcp_servers"]["other"]["command"].as_str(),
            Some("keep")
        );
        assert!(!result.contains("OLD"));
        let removed = configuration(&result, Format::Toml, None).unwrap();
        assert!(
            configured_command(&removed, Format::Toml)
                .unwrap()
                .is_none()
        );
        assert!(removed.contains("other"));
    }
    for (text, format) in [
        ("{broken", Format::Json { stdio: false }),
        ("{\"mcpServers\":[]}", Format::Json { stdio: false }),
        ("[broken", Format::Toml),
        ("mcp_servers = false", Format::Toml),
    ] {
        assert!(configuration(text, format, Some(command)).is_err());
    }
}

#[test]
fn atomic_changes_restore_files_and_refuse_concurrent_edits() {
    let temporary = tempfile::tempdir().unwrap();
    let client = Client {
        key: "test".into(),
        name: "test".into(),
        path: temporary.path().join("config.json"),
        format: Format::Json { stdio: false },
        selected: true,
    };
    let original = b"{\"theme\": \"keep\"}";
    fs::write(&client.path, original).unwrap();
    let change = Change::client(&client, Some(Path::new("/server"))).unwrap();
    change.apply().unwrap();
    assert_eq!(
        fs::read(temporary.path().join("config.json.mudel-ai-connect-backup")).unwrap(),
        original
    );
    change.rollback().unwrap();
    assert_eq!(fs::read(&client.path).unwrap(), original);
    fs::write(&client.path, b"concurrent edit").unwrap();
    assert!(change.apply().is_err());
    assert_eq!(fs::read(&client.path).unwrap(), b"concurrent edit");
    let _first = lock(temporary.path()).unwrap();
    assert!(lock(temporary.path()).is_err());
}

#[test]
fn server_update_and_uninstall_can_restore_the_previous_complete_bundle() {
    let temporary = tempfile::tempdir().unwrap();
    let target = temporary.path().join("bin/server");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join(EXE), b"old exe").unwrap();
    fs::write(target.join("old-library.dll"), b"keep until success").unwrap();
    #[cfg(windows)]
    {
        let path = target.join("old-library.dll");
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).unwrap();
    }
    let change = ServerChange::apply(&target, Some(b"new exe"))
        .unwrap()
        .unwrap();
    assert_eq!(fs::read(target.join(EXE)).unwrap(), b"new exe");
    assert!(
        ServerChange::apply(&target, Some(b"new exe"))
            .unwrap()
            .is_none()
    );
    change.rollback().unwrap();
    assert_eq!(fs::read(target.join(EXE)).unwrap(), b"old exe");
    assert!(target.join("old-library.dll").is_file());
    let removed = ServerChange::apply(&target, None).unwrap().unwrap();
    assert!(!target.exists());
    removed.rollback().unwrap();
    assert!(target.join("old-library.dll").is_file());
    ServerChange::apply(&target, None)
        .unwrap()
        .unwrap()
        .finish()
        .unwrap();
    assert!(!target.exists());
}

#[test]
fn failed_migration_restores_current_and_previous_name_bundles() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = paths(temporary.path(), false);
    for (directory, bytes) in [
        (paths.server_dir(), b"current".as_slice()),
        (paths.legacy().join("bin"), b"previous".as_slice()),
    ] {
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(EXE), bytes).unwrap();
    }
    let changes = change_servers(&paths, Some(b"new server")).unwrap();
    assert!(!paths.legacy().join("bin").exists());
    assert!(complete_servers(changes, Err("simulated failure".into())).is_err());
    assert_eq!(fs::read(paths.server()).unwrap(), b"current");
    assert_eq!(
        fs::read(paths.legacy().join("bin").join(EXE)).unwrap(),
        b"previous"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn failed_install_restores_exact_credentials_and_client_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = paths(temporary.path(), false);
    fs::create_dir_all(paths.home.join(".codex")).unwrap();
    fs::write(
        paths.home.join(".codex/config.toml"),
        "# keep\nmodel = 'test'\n",
    )
    .unwrap();
    let old = Credentials::new("https://campus.example", "old123").unwrap();
    credentials::save(&paths.data, Kind::Installed, &old).unwrap();
    credentials::save(
        &paths.data,
        Kind::Plugin,
        &Credentials::new("https://plugin.example", "plugin123").unwrap(),
    )
    .unwrap();
    let credential_path = paths.data.join(Kind::Installed.filename());
    let original = fs::read(&credential_path).unwrap();
    let client = paths.clients().remove(0);
    let config = fs::read(&client.path).unwrap();
    let changes = installation_changes(&paths, std::slice::from_ref(&client)).unwrap();
    let account = Credentials::new("https://campus.example", "new456").unwrap();
    let result = apply_installation(&paths, &account, &changes, || async {
        Err("simulated failure".into())
    })
    .await;
    assert!(result.is_err());
    assert_eq!(fs::read(&credential_path).unwrap(), original);
    assert_eq!(fs::read(&client.path).unwrap(), config);
    apply_installation(&paths, &account, &changes, || async { Ok(()) })
        .await
        .unwrap();
    assert_eq!(
        credentials::read(&paths.data, Kind::Installed)
            .unwrap()
            .unwrap()
            .token,
        "new456"
    );
    assert_eq!(
        credentials::read(&paths.data, Kind::Plugin)
            .unwrap()
            .unwrap()
            .token,
        "plugin123"
    );
    assert!(!diagnostics(&paths).contains("new456"));
    uninstall(&paths).unwrap();
    assert!(!credential_path.exists());
    assert_eq!(
        credentials::read(&paths.data, Kind::Plugin)
            .unwrap()
            .unwrap()
            .token,
        "plugin123"
    );
    assert!(
        configured_command(&fs::read_to_string(client.path).unwrap(), Format::Toml)
            .unwrap()
            .is_none()
    );
}

#[cfg(windows)]
#[tokio::test]
async fn repairing_invalid_credentials_can_restore_the_exact_original() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = paths(temporary.path(), false);
    fs::create_dir_all(&paths.data).unwrap();
    let credential_path = paths.data.join(Kind::Installed.filename());
    fs::write(&credential_path, b"broken credential to repair").unwrap();
    let account = Credentials::new("https://campus.example", "new456").unwrap();
    apply_installation(&paths, &account, &[], || async { Err("failure".into()) })
        .await
        .unwrap_err();
    assert_eq!(
        fs::read(&credential_path).unwrap(),
        b"broken credential to repair"
    );
    apply_installation(&paths, &account, &[], || async { Ok(()) })
        .await
        .unwrap();
    assert_eq!(
        credentials::read(&paths.data, Kind::Installed)
            .unwrap()
            .unwrap()
            .token,
        "new456"
    );
}

#[test]
fn preferences_keep_legacy_denials_and_roll_back_failed_folder_updates() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = paths(temporary.path(), false);
    assert_eq!(read_permissions(&paths).unwrap(), [true; 5]);
    atomic_write(
        &paths.data.join("permissions.json"),
        br#"{"courses":true,"materials":false,"assignments":true,"calendar":false}"#,
    )
    .unwrap();
    assert_eq!(
        read_permissions(&paths).unwrap(),
        [true, false, true, false, false]
    );
    save_preferences(&paths, [false; 5], Some(temporary.path())).unwrap();
    assert_eq!(read_permissions(&paths).unwrap(), [false; 5]);
    assert_eq!(
        download_folder(&paths).unwrap(),
        Some(temporary.path().into())
    );
    let before = fs::read(paths.data.join("permissions.json")).unwrap();
    assert!(save_preferences(&paths, [true; 5], Some(Path::new("relative"))).is_err());
    assert_eq!(
        fs::read(paths.data.join("permissions.json")).unwrap(),
        before
    );
    assert!(save_preferences(&paths, [true; 5], Some(&temporary.path().join("missing"))).is_err());
}

#[cfg(unix)]
#[test]
fn linked_configurations_are_preserved_and_copies_are_private() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let linked = temporary.path().join("linked");
    atomic_write(&source, b"keep").unwrap();
    assert_eq!(
        fs::metadata(&source).unwrap().permissions().mode() & 0o777,
        0o600
    );
    symlink(&source, &linked).unwrap();
    assert!(atomic_write(&linked, b"replace").is_err());
    assert_eq!(fs::read(source).unwrap(), b"keep");
}

#[cfg(windows)]
#[test]
fn a_running_server_is_replaced_and_restored_without_closing_it() {
    let temporary = tempfile::tempdir().unwrap();
    let target = temporary.path().join("bin/server");
    fs::create_dir_all(&target).unwrap();
    let ping = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/PING.EXE");
    fs::copy(&ping, target.join(EXE)).unwrap();
    let original = fs::read(target.join(EXE)).unwrap();
    fs::write(target.join("old-library.dll"), b"old").unwrap();
    // Claude y Codex mantienen el servidor en marcha mientras se actualiza.
    let mut running = std::process::Command::new(target.join(EXE))
        .args(["-n", "60", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let change = ServerChange::apply(&target, Some(b"new exe"))
        .unwrap()
        .unwrap();
    assert_eq!(fs::read(target.join(EXE)).unwrap(), b"new exe");
    assert!(running.try_wait().unwrap().is_none());
    change.rollback().unwrap();
    assert_eq!(fs::read(target.join(EXE)).unwrap(), original);
    assert!(running.try_wait().unwrap().is_none());

    let change = ServerChange::apply(&target, Some(b"new exe"))
        .unwrap()
        .unwrap();
    change.finish().unwrap();
    assert_eq!(fs::read(target.join(EXE)).unwrap(), b"new exe");
    assert!(
        running.try_wait().unwrap().is_none(),
        "no se debe cerrar el programa en marcha"
    );
    running.kill().unwrap();
    running.wait().unwrap();
}

#[cfg(windows)]
#[test]
fn a_locked_folder_falls_back_to_replacing_only_the_executable() {
    use std::os::windows::fs::OpenOptionsExt;
    let temporary = tempfile::tempdir().unwrap();
    let target = temporary.path().join("bin/server");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join(EXE), b"old exe").unwrap();
    fs::write(target.join("loaded.dll"), b"in use").unwrap();
    // Un archivo abierto sin permiso para moverse impide renombrar la carpeta.
    let _held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(target.join("loaded.dll"))
        .unwrap();
    assert!(fs::rename(&target, temporary.path().join("moved")).is_err());

    let change = ServerChange::apply(&target, Some(b"new exe"))
        .unwrap()
        .unwrap();
    assert_eq!(fs::read(target.join(EXE)).unwrap(), b"new exe");
    assert!(target.join("loaded.dll").is_file());
    change.rollback().unwrap();
    assert_eq!(fs::read(target.join(EXE)).unwrap(), b"old exe");

    let change = ServerChange::apply(&target, Some(b"new exe"))
        .unwrap()
        .unwrap();
    change.finish().unwrap();
    assert_eq!(fs::read(target.join(EXE)).unwrap(), b"new exe");
    // Desinstalar con la carpeta bloqueada deja desconectado el ejecutable sin fallar.
    let removed = ServerChange::apply(&target, None).unwrap().unwrap();
    assert!(!target.join(EXE).exists());
    removed.rollback().unwrap();
    assert_eq!(fs::read(target.join(EXE)).unwrap(), b"new exe");
}

#[test]
fn only_the_claude_desktop_app_counts_as_open_not_claude_code() {
    assert!(is_claude_desktop(
        r"c:\program files\windowsapps\claude_1.2.3_x64__abc\app\claude.exe"
    ));
    assert!(is_claude_desktop(
        r"c:\users\a\appdata\local\anthropicclaude\app-1.0\claude.exe"
    ));
    assert!(is_claude_desktop(
        "/applications/claude.app/contents/macos/claude"
    ));
    assert!(!is_claude_desktop(r"c:\users\a\.local\bin\claude.exe"));
    assert!(!is_claude_desktop(r"c:\tools\other\codex.exe"));
}

#[test]
fn nothing_is_reported_or_closed_when_no_app_is_selected() {
    assert!(running_apps(&[]).is_empty());
    assert_eq!(close_apps(&[]).unwrap(), "No había aplicaciones abiertas.");
}
