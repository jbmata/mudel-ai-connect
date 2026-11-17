use super::*;
use crate::moodle::tests::{mock_with, response};

fn binary(body: &[u8], mime: &str) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    response.extend_from_slice(body);
    response
}

fn item(base: &str, name: &str, size: usize, modified: u64) -> Value {
    json!({"type": "file", "filename": name, "filesize": size, "timemodified": modified,
        "fileurl": format!("{base}/webservice/pluginfile.php/42/{name}?token=secret123")})
}

fn sections(base: &str, contents: Vec<Value>) -> String {
    json!([{"id": 1, "modules": [{"id": 90, "name": "Materiales", "modname": "resource",
        "url": format!("{base}/mod/resource/view.php?id=90"), "contents": contents}]}])
    .to_string()
}

fn params(query: &str, file_start: u64) -> SearchFiles {
    SearchFiles {
        course_id: 7,
        query: query.into(),
        file_start,
    }
}

#[test]
fn fragments_preserve_unicode_offsets_and_match_across_boundaries() {
    let document = format!(
        "{}TERMÓPAR y calibracio\u{301}n{}",
        "😀".repeat(795),
        "x".repeat(900)
    );
    let terms = vec!["termopar".into(), "calibracion".into()];
    let (offset, text) = fragment(&document, &terms).unwrap();
    assert!(offset > 0);
    assert!(text.chars().count() <= FRAGMENT);
    assert_eq!(
        text,
        document
            .chars()
            .skip(offset)
            .take(FRAGMENT)
            .collect::<String>()
    );
    assert!(fragment(&document, &["inexistente".into()]).is_none());
    assert!(fragment("", &terms).is_none());
}

#[test]
fn cache_is_bounded_and_invalidates_changed_or_missing_metadata() {
    let mut index = Index::default();
    let item = item("https://campus.example", "test.txt", 10, 1);
    for position in 0..9 {
        index.insert((7, 90, position), &item, "x".repeat(MAX_TEXT).into());
    }
    assert!(index.bytes <= MAX_CACHE);
    assert!(index.get((7, 90, 0), &item).is_none());
    assert!(index.get((8, 90, 8), &item).is_none());
    assert!(Index::default().get((7, 90, 8), &item).is_none());
    let mut changed = item.clone();
    changed["timemodified"] = json!(2);
    assert!(index.get((7, 90, 8), &changed).is_none());
    assert!(index.get((7, 90, 8), &item).is_none());
    changed["timemodified"] = Value::Null;
    index.insert((7, 90, 8), &changed, "never cache".into());
    assert!(index.get((7, 90, 8), &changed).is_none());
}

#[tokio::test]
async fn search_reuses_text_checks_changes_and_never_returns_removed_or_inaccessible_files() {
    let original = "Termopar y calibración. secret123";
    let changed = "La resistencia mide otra cosa.";
    let (client, requests, handle) = mock_with(|base| {
        let listing = |size, modified| {
            sections(
                base,
                vec![
                    json!({"type": "url", "filename": "enlace"}),
                    item(base, "material.txt", size, modified),
                ],
            )
        };
        vec![
            response(200, &listing(original.len(), 1)).into_bytes(),
            binary(original.as_bytes(), "text/plain"),
            response(200, &listing(original.len(), 1)).into_bytes(),
            response(200, &listing(changed.len(), 2)).into_bytes(),
            binary(changed.as_bytes(), "text/plain"),
            response(200, "[]").into_bytes(),
            response(403, "denied").into_bytes(),
        ]
    });
    let index = Mutex::default();
    for _ in 0..2 {
        let value = search(&client, &index, params("TERMOPAR calibracion", 0))
            .await
            .unwrap();
        let result = &value["results"][0];
        assert_eq!(result["file_index"], 1);
        assert_eq!(result["filename"], "material.txt");
        assert_eq!(result["offset"], 0);
        assert!(result["text"].as_str().unwrap().contains("[REDACTED]"));
        assert!(!value.to_string().contains("secret123"));
        assert_eq!(value["next_file_start"], Value::Null);
    }
    for _ in 0..2 {
        let value = search(&client, &index, params("termopar", 0))
            .await
            .unwrap();
        assert!(value["results"].as_array().unwrap().is_empty());
    }
    assert!(
        search(&client, &index, params("termopar", 0))
            .await
            .is_err()
    );
    assert_eq!(requests.try_iter().count(), 7);
    handle.join().unwrap();
}

#[tokio::test]
async fn pagination_continues_even_when_the_first_page_has_no_matches() {
    let text = "Aquí no aparece la palabra buscada.";
    let (client, requests, handle) = mock_with(|base| {
        let contents: Vec<_> = (0..10)
            .map(|n| item(base, &format!("{n}.txt"), text.len(), 1))
            .collect();
        let mut responses = vec![response(200, &sections(base, contents.clone())).into_bytes()];
        responses.extend((0..PAGE_FILES).map(|_| binary(text.as_bytes(), "text/plain")));
        responses.push(response(200, &sections(base, contents)).into_bytes());
        responses.extend((PAGE_FILES..10).map(|_| binary(text.as_bytes(), "text/plain")));
        responses
    });
    let index = Mutex::default();
    let first = search(&client, &index, params("termopar", 0))
        .await
        .unwrap();
    assert!(first["results"].as_array().unwrap().is_empty());
    assert_eq!(first["next_file_start"], PAGE_FILES);
    let last = search(&client, &index, params("termopar", PAGE_FILES as u64))
        .await
        .unwrap();
    assert_eq!(last["next_file_start"], Value::Null);
    assert_eq!(last["total_files"], 10);
    assert_eq!(requests.try_iter().count(), 12);
    handle.join().unwrap();
}

#[tokio::test]
async fn results_are_short_and_files_with_errors_do_not_hide_valid_matches() {
    let text = format!("Termopar{}", " con texto".repeat(100));
    let (client, _requests, handle) = mock_with(|base| {
        let mut contents = vec![
            item(base, "programa.exe", 1, 1),
            item(base, "roto.pdf", 3, 1),
        ];
        contents.extend((0..6).map(|n| item(base, &format!("{n}.txt"), text.len(), 1)));
        let mut responses = vec![
            response(200, &sections(base, contents)).into_bytes(),
            binary(b"bad", "application/pdf"),
        ];
        responses.extend((0..RESULTS).map(|_| binary(text.as_bytes(), "text/plain")));
        responses
    });
    let value = search(&client, &Mutex::default(), params("termopar", 0))
        .await
        .unwrap();
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), RESULTS);
    assert!(
        results
            .iter()
            .all(|r| r["text"].as_str().unwrap().chars().count() <= FRAGMENT)
    );
    assert_eq!(value["skipped_files"], 1);
    assert_eq!(value["unsupported_files"], 1);
    assert_eq!(value["errors"][0]["filename"], "roto.pdf");
    assert_eq!(value["next_file_start"], 6);
    handle.join().unwrap();
}

#[tokio::test]
async fn pdf_and_office_search_use_the_existing_text_extractors() {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Apuntes de prueba</w:t></w:r></w:p></w:body></w:document>"#).unwrap();
    let docx = zip.finish().unwrap().into_inner();
    for (name, bytes, mime) in [
        (
            "apuntes.pdf",
            include_bytes!("../../tests/fixtures/text.pdf").as_slice(),
            "application/pdf",
        ),
        ("apuntes.docx", docx.as_slice(), "application/zip"),
    ] {
        let (client, _requests, handle) = mock_with(|base| {
            vec![
                response(200, &sections(base, vec![item(base, name, bytes.len(), 1)])).into_bytes(),
                binary(bytes, mime),
            ]
        });
        let value = search(&client, &Mutex::default(), params("prueba", 0))
            .await
            .unwrap();
        assert!(
            value["results"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Apuntes de prueba")
        );
        assert_eq!(value["skipped_files"], 0);
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn invalid_searches_make_no_requests() {
    let client = MoodleClient::new("https://127.0.0.1:1", "secret123").unwrap();
    for mut params in [
        params("a", 0),
        params("  ", 0),
        params(&"x".repeat(101), 0),
        params("test", 0),
    ] {
        if params.query == "test" {
            params.course_id = 0;
        }
        assert!(search(&client, &Mutex::default(), params).await.is_err());
    }
}

#[tokio::test]
async fn missing_modification_dates_force_fresh_downloads() {
    let (client, requests, handle) = mock_with(|base| {
        let mut file = item(base, "apuntes.txt", 8, 1);
        file["timemodified"] = Value::Null;
        let listing = sections(base, vec![file]);
        vec![
            response(200, &listing).into_bytes(),
            binary(b"termopar", "text/plain"),
            response(200, &listing).into_bytes(),
            binary(b"sin dato", "text/plain"),
        ]
    });
    let index = Mutex::default();
    assert_eq!(
        search(&client, &index, params("termopar", 0))
            .await
            .unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        search(&client, &index, params("termopar", 0))
            .await
            .unwrap()["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(requests.try_iter().count(), 4);
    handle.join().unwrap();
}

#[tokio::test]
async fn unavailable_modules_and_oversized_files_are_not_downloaded() {
    let (client, requests, handle) = mock_with(|base| {
        let hidden = item(base, "hidden.txt", 8, 1);
        let oversized = item(base, "large.txt", crate::moodle::MAX_BYTES + 1, 1);
        let sections = json!([{"id": 1, "modules": [
            {"id": 90, "uservisible": false, "contents": [hidden]},
            {"id": 91, "contents": [oversized]}
        ]}]);
        vec![response(200, &sections.to_string()).into_bytes()]
    });
    let value = search(&client, &Mutex::default(), params("termopar", 0))
        .await
        .unwrap();
    assert!(value["results"].as_array().unwrap().is_empty());
    assert_eq!(value["total_files"], 1);
    assert_eq!(value["skipped_files"], 1);
    assert_eq!(requests.try_iter().count(), 1);
    handle.join().unwrap();
}
