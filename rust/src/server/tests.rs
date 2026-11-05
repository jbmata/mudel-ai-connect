use super::*;
use crate::moodle::tests::{mock, response};
use std::sync::atomic::{AtomicUsize, Ordering};

fn temporary() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "mudel-rust-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn server(client: MoodleClient) -> MoodleServer {
    MoodleServer {
        client: Ok(client),
        permissions_path: Ok(temporary()),
        tool_router: MoodleServer::tool_router(),
    }
}

#[test]
fn permissions_preserve_disabled_queries_and_fail_closed_on_invalid_files() {
    let path = temporary();
    assert!(require_permission(&path, "courses").is_ok());
    for content in [
        r#"{"courses":false,"materials":true,"assignments":true,"calendar":true,"downloads":true}"#,
        r#"{"courses":false,"materials":true,"assignments":true,"calendar":true}"#,
    ] {
        fs::write(&path, content).unwrap();
        assert!(
            require_permission(&path, "courses")
                .unwrap_err()
                .contains("desactivada")
        );
        assert!(require_permission(&path, "materials").is_ok());
    }
    for content in [
        "{",
        "{}",
        "null",
        r#"{"courses":1}"#,
        r#"{"courses":true,"materials":true,"assignments":true,"calendar":true,"downloads":true,"extra":true}"#,
    ] {
        fs::write(&path, content).unwrap();
        assert_eq!(
            require_permission(&path, "courses").unwrap_err(),
            PERMISSION_ERROR
        );
    }
    fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn disabled_permissions_and_invalid_arguments_do_not_make_requests() {
    let client = MoodleClient::new("https://127.0.0.1:1", "secret123").unwrap();
    let server = server(client);
    let path = server.permissions_path.as_ref().unwrap();
    fs::write(path, r#"{"courses":false,"materials":false,"assignments":false,"calendar":false,"downloads":false}"#).unwrap();
    assert!(server.courses().await.unwrap_err().contains("desactivada"));
    assert!(server.events().await.unwrap_err().contains("desactivada"));
    assert!(
        server
            .assignments(7)
            .await
            .unwrap_err()
            .contains("desactivada")
    );
    assert!(
        server
            .assignments(0)
            .await
            .unwrap_err()
            .contains("positivo")
    );
    assert!(
        server
            .context(Sections {
                course_id: 7,
                section_start: 0,
                section_limit: 11
            })
            .await
            .unwrap_err()
            .contains("entre 1 y 10")
    );
    assert!(
        server
            .context(Sections {
                course_id: 7,
                section_start: 0,
                section_limit: 3
            })
            .await
            .unwrap_err()
            .contains("desactivada")
    );
    fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn five_tools_match_the_python_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/moodle.json")).unwrap();
    let raw = &fixture["responses"];
    let bodies = [
        FUNCTIONS[0],
        FUNCTIONS[0],
        FUNCTIONS[1],
        FUNCTIONS[2],
        FUNCTIONS[3],
        FUNCTIONS[4],
    ]
    .iter()
    .map(|function| response(200, &raw[function].to_string()))
    .collect();
    let (client, requests, handle) = mock(bodies);
    let base = client.base_url.clone();
    let server = server(client);
    let mut actual = json!({
        "moodle_status": server.result(server.status().await).structured_content.unwrap(),
        "list_courses": server.result(server.courses().await).structured_content.unwrap(),
        "get_course_context": server.result(server.context(Sections { course_id: 7, section_start: 0, section_limit: 3 }).await).structured_content.unwrap(),
        "list_assignments": server.result(server.assignments(7).await).structured_content.unwrap(),
        "get_upcoming_events": server.result(server.events().await).structured_content.unwrap(),
    });
    let fetched_at = actual["get_course_context"]
        .as_object_mut()
        .unwrap()
        .remove("fetched_at")
        .unwrap();
    assert!(chrono::DateTime::parse_from_rfc3339(fetched_at.as_str().unwrap()).is_ok());
    let actual = actual
        .to_string()
        .replace(&base, "https://campus.example/moodle");
    assert_eq!(
        serde_json::from_str::<Value>(&actual).unwrap(),
        fixture["expected"]
    );
    let requests: Vec<_> = requests.try_iter().collect();
    assert_eq!(requests.len(), 6);
    assert!(requests[4].contains("courseids%5B0%5D=7"));
    handle.join().unwrap();
}

#[tokio::test]
async fn pagination_returns_the_last_section_and_handles_large_offsets() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/moodle.json")).unwrap();
    let raw = fixture["responses"][FUNCTIONS[2]].to_string();
    let (client, _requests, handle) = mock(vec![response(200, &raw), response(200, &raw)]);
    let server = server(client);
    let last = server
        .context(Sections {
            course_id: 7,
            section_start: 3,
            section_limit: 3,
        })
        .await
        .unwrap();
    assert_eq!(last["sections"].as_array().unwrap().len(), 1);
    assert_eq!(last["next_section_start"], Value::Null);
    let empty = server
        .context(Sections {
            course_id: 7,
            section_start: u64::MAX,
            section_limit: 3,
        })
        .await
        .unwrap();
    assert_eq!(empty["sections"], json!([]));
    assert_eq!(empty["next_section_start"], Value::Null);
    handle.join().unwrap();
}
