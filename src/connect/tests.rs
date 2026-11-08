use super::*;

fn answer(base: &str, passport: &str, token: &str) -> String {
    let site = format!("{:x}", Md5::digest(format!("{base}{passport}")));
    let payload = BASE64_STANDARD.encode(format!("{site}:::{token}:::privado123"));
    format!("{SCHEME}://token={payload}")
}

fn request(port: u16, line: &str) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(stream, "{line}\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply
}

fn wait_for_answer(receiver: &Receiver) -> Option<String> {
    for _ in 0..50 {
        if let Some(answer) = receiver.try_answer() {
            return Some(answer);
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    None
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
    let url = first.launch_url("http://127.0.0.1:4242/mudel-login?");
    assert!(url.starts_with("https://campus.example/admin/tool/mobile/launch.php?"));
    assert!(url.contains("service=moodle_mobile_app"));
    assert!(url.contains(&format!("passport={}", first.passport)));
    assert!(url.ends_with("&urlscheme=http%3A%2F%2F127%2E0%2E0%2E1%3A4242%2Fmudel%2Dlogin%3F"));
}

#[test]
fn a_genuine_answer_gives_the_token_however_the_browser_wrote_it() {
    let attempt = Attempt::new("https://campus.example").unwrap();
    let genuine = answer(attempt.base_url(), &attempt.passport, "abc123XYZ");
    assert_eq!(attempt.accept(&genuine).unwrap(), "abc123XYZ");
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
fn the_browser_request_is_turned_into_the_answer_only_when_it_is_the_redirect() {
    assert_eq!(
        answer_in("GET /mudel-login?://token=QUJD%3D HTTP/1.1").as_deref(),
        Some("mudelai://token=QUJD%3D")
    );
    for other in [
        "",
        "GET / HTTP/1.1",
        "GET /favicon.ico HTTP/1.1",
        "GET /mudel-login HTTP/1.1",
        "GET /mudel-login?://token= HTTP/1.1",
        "GET /mudel-login?token=abc HTTP/1.1",
        "POST /mudel-login?://token=abc HTTP/1.1",
    ] {
        assert!(answer_in(other).is_none(), "{other}");
    }
}

#[test]
fn the_local_receiver_hands_over_the_answer_and_ignores_everything_else() {
    let receiver = Receiver::start().unwrap();
    let redirect = receiver.redirect();
    assert!(redirect.starts_with("http://127.0.0.1:"));
    assert!(redirect.ends_with("/mudel-login?"));
    let port = receiver.port;

    // Una petición cualquiera (por ejemplo el icono de la pestaña) no cuenta.
    let reply = request(port, "GET /favicon.ico HTTP/1.1");
    assert!(reply.starts_with("HTTP/1.1 404"), "{reply}");
    assert!(wait_for_answer(&receiver).is_none());

    // La redirección de Moodle sí, y el navegador recibe una página sin el token.
    let attempt = Attempt::new("https://campus.example").unwrap();
    let genuine = answer(attempt.base_url(), &attempt.passport, "abc123XYZ");
    let payload = genuine.split_once("token=").unwrap().1;
    let reply = request(
        port,
        &format!("GET /mudel-login?://token={payload} HTTP/1.1"),
    );
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.contains("Ya puedes cerrar esta pestaña"));
    assert!(reply.contains("history.replaceState"));
    assert!(!reply.contains(payload));
    let received = wait_for_answer(&receiver).expect("la respuesta debía llegar");
    assert_eq!(attempt.accept(&received).unwrap(), "abc123XYZ");
}

#[test]
fn a_receiver_stops_listening_when_dropped() {
    let receiver = Receiver::start().unwrap();
    let port = receiver.port;
    drop(receiver);
    std::thread::sleep(Duration::from_millis(400));
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
}
