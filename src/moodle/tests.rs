use super::*;
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

pub(crate) fn response(status: u16, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

// HTTP solo dentro del fixture: el constructor de producción exige HTTPS.
pub(crate) fn mock(
    responses: Vec<String>,
) -> (MoodleClient, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    mock_with(|_| responses.into_iter().map(String::into_bytes).collect())
}

pub(crate) fn mock_with(
    build: impl FnOnce(&str) -> Vec<Vec<u8>>,
) -> (MoodleClient, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = MoodleClient::new("https://campus.example/moodle", "secret123").unwrap();
    client.base_url = format!("http://{}/moodle", listener.local_addr().unwrap());
    let responses = build(&client.base_url);
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&chunk[..read]);
                if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap_or("0")
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            sender.send(String::from_utf8(request).unwrap()).unwrap();
            // El cliente puede cerrar antes de consumir una respuesta demasiado grande.
            let _ = stream.write_all(&response);
        }
    });
    (client, receiver, handle)
}

#[test]
fn configuration_requires_https_and_an_ascii_token() {
    for (url, token) in [
        ("http://campus.example", "secret123"),
        ("https://user:pass@campus.example", "secret123"),
        ("https://campus.example?token=value", "secret123"),
        ("https://campus.example#fragment", "secret123"),
        ("https://campus.example", ""),
        ("https://campus.example", "clave inválida"),
    ] {
        assert!(MoodleClient::new(url, token).is_err());
    }
}

#[test]
fn file_urls_reject_other_origins_endpoints_and_encoded_traversal() {
    let client = MoodleClient::new("https://campus.example/moodle", "secret123").unwrap();
    for url in [
        "https://other.example/webservice/pluginfile.php/1/a.txt",
        "http://campus.example/moodle/webservice/pluginfile.php/1/a.txt",
        "https://campus.example/moodle/login/index.php",
        "https://campus.example/moodle/webservice/pluginfile.php/../rest/server.php",
        "https://campus.example/moodle/webservice/pluginfile.php/%2e%2e/rest/server.php",
        "https://campus.example/moodle/webservice/pluginfile.php/a%2f..%2frest/server.php",
        "https://campus.example/moodle/webservice/pluginfile.php/%5cother/a.txt",
        "https://user:password@campus.example/moodle/webservice/pluginfile.php/1/a.txt",
    ] {
        assert!(client.file_request_url(url).is_err(), "{url}");
    }
    let url = client.file_request_url("https://campus.example/moodle/webservice/pluginfile.php?file=%2F42%2Fcontent%2Fapuntes.txt&TOKEN=old&wstoken=another#fragment").unwrap();
    assert_eq!(
        url.query_pairs().collect::<Vec<_>>(),
        vec![("file".into(), "/42/content/apuntes.txt".into())]
    );
    assert!(url.fragment().is_none());
}

#[tokio::test]
async fn streamed_files_enforce_limits_and_put_the_token_only_in_the_post_body() {
    let (client, requests, handle) =
        mock_with(|_| vec![b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n12345".to_vec()]);
    let mut body = Vec::new();
    let url = format!(
        "{}/webservice/pluginfile.php?file=%2F42%2Fcontent%2Ftest&token=old",
        client.base_url
    );
    assert!(
        client
            .save(&url, &mut body, 4)
            .await
            .unwrap_err()
            .contains("límite")
    );
    assert!(body.len() <= 4);
    let request = requests.recv().unwrap();
    assert!(!request.lines().next().unwrap().contains("token="));
    assert!(request.ends_with("token=secret123"));
    assert!(!request.contains("old"));
    handle.join().unwrap();
}

#[tokio::test]
async fn post_keeps_token_out_of_url_and_encodes_moodle_array_parameters() {
    let (client, requests, handle) = mock(vec![response(200, "{\"courses\":[]}")]);
    assert_eq!(
        client
            .call(FUNCTIONS[3], &[("courseids[0]", "7".into())])
            .await
            .unwrap(),
        json!({"courses": []})
    );
    let request = requests.recv().unwrap();
    assert!(request.starts_with("POST /moodle/webservice/rest/server.php HTTP/1.1\r\n"));
    assert!(!request.lines().next().unwrap().contains("secret123"));
    let body = request.split("\r\n\r\n").nth(1).unwrap();
    let params: std::collections::BTreeMap<_, _> =
        Url::parse(&format!("https://example.test/?{body}"))
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
    assert_eq!(params["wstoken"], "secret123");
    assert_eq!(params["courseids[0]"], "7");
    assert_eq!(params["wsfunction"], FUNCTIONS[3]);
    handle.join().unwrap();
}

#[tokio::test]
async fn write_functions_never_reach_the_network() {
    let client = MoodleClient::new("https://127.0.0.1:1", "secret123").unwrap();
    let error = client
        .call("core_course_delete_courses", &[])
        .await
        .unwrap_err();
    assert!(error.contains("no está permitida"));
}

#[tokio::test]
async fn redirects_http_errors_and_non_json_are_rejected_without_secrets() {
    for raw in [
        "HTTP/1.1 302 Found\r\nLocation: https://otro.example/secret123\r\nContent-Length: 0\r\n\r\n".into(),
        response(403, "secret123"), response(200, "<html>secret123</html>"), response(200, "42"),
        response(200, "{\"exception\":\"error\",\"errorcode\":\"secret123\",\"message\":\"secret123\"}"),
    ] {
        let (client, _requests, handle) = mock(vec![raw]);
        let error = client.call(FUNCTIONS[0], &[]).await.unwrap_err();
        assert!(!error.contains("secret123"));
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn response_limit_applies_with_and_without_content_length() {
    let large_body = "a".repeat(MAX_BYTES + 1);
    for raw in [
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_BYTES + 1
        ),
        format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{large_body}"),
    ] {
        let (client, _requests, handle) = mock(vec![raw]);
        assert!(
            client
                .call(FUNCTIONS[0], &[])
                .await
                .unwrap_err()
                .contains("10 MiB")
        );
        handle.join().unwrap();
    }
}

#[test]
fn nested_outputs_redact_tokens_and_private_fields() {
    let client = MoodleClient::new("https://campus.example", "secret123").unwrap();
    let cleaned = client.clean(json!({
        "items": ["secret123", "https://example.test/file?TOKEN=anothersecret&x=1", "https://x/?wstoken=third"],
        "PrivateToken": "privatevalue", "nested": {"token": "hidden", "safe": "ok"}
    }));
    let serialized = cleaned.to_string();
    for secret in [
        "secret123",
        "anothersecret",
        "third",
        "privatevalue",
        "hidden",
    ] {
        assert!(!serialized.contains(secret));
    }
    assert_eq!(cleaned["nested"], json!({"safe": "ok"}));
}

#[test]
fn html_and_unicode_limits_keep_readable_text_without_scripts() {
    let nested = "<div>".repeat(2000) + "Texto" + &"</div>".repeat(2000);
    assert_eq!(text(&nested), "Texto");
    assert_eq!(
        text(
            "<p>Uno &amp; dos</p><script>malicioso()</script><style>oculto</style><p>Tres<br>Cuatro</p>"
        ),
        "Uno & dos\nTres\nCuatro"
    );
    assert_eq!(plain(&json!({"name": "á🙂b"}), "name", 2).unwrap(), "á🙂");
    assert_eq!(
        utc(&json!(1700000000)).unwrap(),
        "2023-11-14T22:13:20+00:00"
    );
    assert_eq!(utc(&json!(0)).unwrap(), Value::Null);
    assert!(utc(&json!(i64::MAX)).is_err());
}
