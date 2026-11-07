use super::*;

fn answer(base: &str, passport: &str, token: &str) -> String {
    let site = format!("{:x}", Md5::digest(format!("{base}{passport}")));
    let payload = BASE64_STANDARD.encode(format!("{site}:::{token}:::privado123"));
    format!("{SCHEME}://token={payload}")
}

#[test]
fn the_campus_address_is_normalised_and_must_be_https() {
    assert_eq!(
        Attempt::new("sifo.comillas.edu/").unwrap().base_url(),
        "https://sifo.comillas.edu"
    );
    assert_eq!(
        Attempt::new("  https://campus.example/moodle ")
            .unwrap()
            .base_url(),
        "https://campus.example/moodle"
    );
    for invalid in [
        "http://campus.example",
        "https://usuario:clave@campus.example",
        "https://campus.example?token=1",
        "",
    ] {
        assert!(Attempt::new(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn the_launch_url_carries_a_fresh_random_passport_each_time() {
    let first = Attempt::new("https://campus.example").unwrap();
    let second = Attempt::new("https://campus.example").unwrap();
    assert_eq!(first.passport.len(), 32);
    assert!(first.passport.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(first.passport, second.passport);
    let url = first.launch_url();
    assert!(url.starts_with("https://campus.example/admin/tool/mobile/launch.php?"));
    assert!(url.contains("service=moodle_mobile_app"));
    assert!(url.contains(&format!("passport={}", first.passport)));
    assert!(url.ends_with("&urlscheme=mudelai"));
}

#[test]
fn a_genuine_answer_gives_the_token_however_the_browser_wrote_it() {
    let attempt = Attempt::new("https://campus.example").unwrap();
    let genuine = answer(attempt.base_url(), &attempt.passport, "abc123XYZ");
    assert_eq!(attempt.accept(&genuine).unwrap(), "abc123XYZ");
    assert_eq!(attempt.accept(&format!("{genuine}/")).unwrap(), "abc123XYZ");
    assert_eq!(
        attempt.accept(&format!("  {genuine}\n")).unwrap(),
        "abc123XYZ"
    );
    assert_eq!(
        attempt
            .accept(&genuine.replacen("mudelai", "MUDELAI", 1))
            .unwrap(),
        "abc123XYZ"
    );
    let (head, payload) = genuine.split_once("token=").unwrap();
    let encoded = format!(
        "{head}token={}",
        payload.replace('=', "%3D").replace('+', "%2B")
    );
    assert_eq!(attempt.accept(&encoded).unwrap(), "abc123XYZ");
}

#[test]
fn answers_from_another_attempt_campus_or_source_are_refused() {
    let attempt = Attempt::new("https://campus.example").unwrap();
    let other_attempt = Attempt::new("https://campus.example").unwrap();
    let wrong_passport = answer(attempt.base_url(), &other_attempt.passport, "abc123");
    let wrong_campus = answer("https://otro.example", &attempt.passport, "abc123");
    for forged in [wrong_passport, wrong_campus] {
        let error = attempt.accept(&forged).unwrap_err();
        assert!(error.contains("no corresponde"), "{error}");
        assert!(!error.contains("abc123"));
    }
    for bad in [
        "".to_string(),
        "https://campus.example/token=abc".to_string(),
        "mudelai://token=".to_string(),
        "mudelai://token=%%%no-es-base64".to_string(),
        "mudelai://otracosa=abc".to_string(),
        "mudelai://token=aGVsbG8=".to_string(), // base64 válido, pero no es una respuesta de Moodle
        "ñ".repeat(10),
    ] {
        assert!(attempt.accept(&bad).is_err(), "{bad}");
    }
    // Un token con símbolos o vacío nunca se acepta, aunque la firma sea correcta.
    for token in ["", "con espacio", "a/b", "tok<script>"] {
        let signed = answer(attempt.base_url(), &attempt.passport, token);
        assert!(attempt.accept(&signed).is_err(), "{token:?}");
    }
}

#[test]
fn the_second_instance_hands_the_answer_over_through_a_file() {
    let directory = tempfile::tempdir().unwrap();
    assert!(take_callback(directory.path()).is_none());
    hand_over(directory.path(), "mudelai://token=ABC=").unwrap();
    assert_eq!(
        take_callback(directory.path()).as_deref(),
        Some("mudelai://token=ABC=")
    );
    assert!(take_callback(directory.path()).is_none());
    // La última respuesta sustituye a la anterior.
    hand_over(directory.path(), "mudelai://token=UNO").unwrap();
    hand_over(directory.path(), "mudelai://token=DOS").unwrap();
    assert_eq!(
        take_callback(directory.path()).as_deref(),
        Some("mudelai://token=DOS")
    );
    // Solo se entregan direcciones propias y de tamaño razonable.
    for bad in [
        "https://campus.example".to_string(),
        "otro://token=1".to_string(),
        format!("mudelai://token={}", "a".repeat(5000)),
    ] {
        assert!(hand_over(directory.path(), &bad).is_err());
    }
    assert!(take_callback(directory.path()).is_none());
}

#[cfg(windows)]
#[test]
fn the_scheme_is_registered_for_the_current_user_and_removed_afterwards() {
    let scheme = format!("mudelaitest{}", std::process::id());
    let exe = std::env::current_exe().unwrap();
    let query = |path: &str| {
        std::process::Command::new("reg")
            .args(["query", &format!(r"HKCU\Software\Classes\{scheme}{path}")])
            .output()
            .unwrap()
    };
    register(&scheme, &exe).unwrap();
    let command = String::from_utf8_lossy(&query(r"\shell\open\command").stdout).to_string();
    assert!(command.contains("%1"), "{command}");
    assert!(command.contains(exe.to_str().unwrap()), "{command}");
    assert!(String::from_utf8_lossy(&query("").stdout).contains("URL Protocol"));
    unregister(&scheme);
    assert!(!query("").status.success());
    // Quitar algo que no existe no es un error.
    unregister(&scheme);
}
