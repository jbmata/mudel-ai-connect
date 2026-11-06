use super::*;

#[test]
fn legacy_accounts_are_validated_without_echoing_secrets() {
    let directory = tempfile::tempdir().unwrap();
    assert!(read_saved(directory.path()).unwrap().is_none());
    let path = directory.path().join("credentials.json");
    std::fs::write(
        &path,
        r#"{"MOODLE_URL":" https://campus.example/ ","MOODLE_TOKEN":" secret123 "}"#,
    )
    .unwrap();
    let account = read_saved(directory.path()).unwrap().unwrap();
    assert_eq!(account.url, "https://campus.example");
    assert_eq!(account.token, "secret123");
    for value in [
        r#"{"MOODLE_URL":"https://campus.example","MOODLE_TOKEN":123}"#,
        r#"{"MOODLE_URL":"https://secret123@campus.example","MOODLE_TOKEN":"secret123"}"#,
        r#"{"MOODLE_URL":"https://campus.example","MOODLE_TOKEN":"secret123!"}"#,
        r#"{"MOODLE_URL":"https://campus.example","MOODLE_TOKEN_DPAPI":"secret123"}"#,
        "secret123",
    ] {
        std::fs::write(&path, value).unwrap();
        let error = read_saved(directory.path()).err().unwrap();
        assert!(!error.contains("secret123"));
    }
    std::fs::write(&path, vec![b' '; MAX_CONFIG as usize + 1]).unwrap();
    assert!(read_saved(directory.path()).is_err());
}

#[test]
fn piped_setup_input_is_bounded_and_requires_two_lines() {
    let mut input = std::io::Cursor::new("https://campus.example\r\nsecret123\r\n");
    let url = read_line(&mut input).unwrap();
    let token = read_line(&mut input).unwrap();
    assert!(Credentials::new(&url, &token).is_ok());
    assert!(read_line(&mut input).is_err());
    let mut input = std::io::Cursor::new(vec![b'a'; MAX_CONFIG as usize + 1]);
    assert!(read_line(&mut input).is_err());
}

#[cfg(windows)]
#[test]
fn dpapi_roundtrip_replaces_installer_account_and_preserves_plugin() {
    let directory = tempfile::tempdir().unwrap();
    let plugin = directory.path().join("credentials.json");
    std::fs::write(&plugin, b"plugin account stays unchanged").unwrap();
    for token in ["secret123", "replacement456"] {
        let account = Credentials::new("https://campus.example", token).unwrap();
        save_windows(directory.path(), Kind::Installed, &account).unwrap();
        let bytes = std::fs::read(directory.path().join("credentials.dpapi.json")).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(token));
        let saved = read_saved(directory.path()).unwrap().unwrap();
        assert_eq!(saved.url, account.url);
        assert_eq!(saved.token, token);
        assert_eq!(
            std::fs::read(&plugin).unwrap(),
            b"plugin account stays unchanged"
        );
    }
    let oversized =
        Credentials::new("https://campus.example", &"a".repeat(MAX_CONFIG as usize)).unwrap();
    assert!(save_windows(directory.path(), Kind::Installed, &oversized).is_err());
    assert_eq!(
        read_saved(directory.path()).unwrap().unwrap().token,
        "replacement456"
    );
    assert!(dpapi(&dpapi(b"", true).unwrap(), false).unwrap().is_empty());
    // Un archivo protegido roto bloquea la consulta en lugar de usar otra cuenta.
    std::fs::write(directory.path().join("credentials.dpapi.json"), "{}").unwrap();
    assert!(read_saved(directory.path()).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn keychain_roundtrip_replaces_only_the_isolated_test_entry() {
    struct Entry(String);
    impl Drop for Entry {
        fn drop(&mut self) {
            let _ =
                security_framework::passwords::delete_generic_password("mudel-ai-connect", &self.0);
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let entry = Entry(format!("test-{}", directory.path().display()));
    assert!(read_keychain(&entry.0).unwrap().is_none());
    for token in ["secret123", "replacement456"] {
        let account = Credentials::new("https://campus.example", token).unwrap();
        save_keychain(&entry.0, &account).unwrap();
        let saved = read_keychain(&entry.0).unwrap().unwrap();
        assert_eq!(saved.url, account.url);
        assert_eq!(saved.token, token);
    }
    security_framework::passwords::set_generic_password("mudel-ai-connect", &entry.0, b"broken")
        .unwrap();
    assert!(read_keychain(&entry.0).is_err());
}
