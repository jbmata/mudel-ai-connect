use super::*;
use crate::moodle::tests::{mock_with, response};

fn binary(body: &[u8], content_type: &str) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    response.extend_from_slice(body);
    response
}

fn sections(base: &str, name: &str, size: u64) -> Value {
    json!([{"id": 1, "modules": [{"id": 90, "url": format!("{base}/mod/resource/view.php?id=90"), "contents": [{
        "type": "file", "filename": name, "filesize": size,
        "fileurl": format!("{base}/webservice/pluginfile.php/42/content/test?token=old&forcedownload=1")
    }]}]}])
}

fn file() -> CourseFile {
    CourseFile {
        course_id: 7,
        module_id: 90,
        file_index: 0,
    }
}
fn read_file(offset: u64) -> ReadFile {
    ReadFile {
        course_id: 7,
        module_id: 90,
        file_index: 0,
        offset,
    }
}

#[test]
fn filenames_keep_the_legacy_rules_and_block_programs_for_both_platforms() {
    for (name, expected) in [
        ("../../../Startup/x.pdf", "_.._.._Startup_x.pdf"),
        (".bashrc", "bashrc"),
        ("CON.txt", "_CON.txt"),
        ("..", "archivo"),
        ("a<b>:\"c\"?.txt", "a_b___c__.txt"),
        ("nota. ", "nota"),
        ("COM9.pdf", "_COM9.pdf"),
        ("cafe\u{301}.txt", "café.txt"),
    ] {
        assert_eq!(safe_filename(name, "archivo"), expected);
    }
    let long = "🙂".repeat(130) + ".pdf";
    assert_eq!(safe_filename(&long, "archivo").chars().count(), 120);
    assert!(safe_filename(&long, "archivo").ends_with(".pdf"));
    for name in [
        "setup.exe",
        "apuntes.PDF.js",
        "Installer.PKG",
        "imagen.dmg",
        "run.command",
        "run.sh",
        "x.app",
    ] {
        assert!(blocked(name), "{name}");
    }
    assert!(!blocked("diapositivas.pptx"));
}

#[test]
fn valid_pdf_text_is_read_and_encrypted_blank_or_broken_pdfs_are_rejected() {
    let body = include_bytes!("../../tests/fixtures/text.pdf");
    assert!(pdf_text(body).unwrap().contains("Apuntes de prueba"));
    assert!(
        pdf_text(include_bytes!("../../tests/fixtures/encrypted.pdf"))
            .unwrap_err()
            .contains("cifrado")
    );
    assert!(
        pdf_text(include_bytes!("../../tests/fixtures/blank.pdf"))
            .unwrap_err()
            .contains("OCR")
    );
    assert!(pdf_text(b"%PDF-1.7 roto").is_err());
    let malformed = String::from_utf8_lossy(body)
        .replace("/MediaBox", "/OtherBox")
        .into_bytes();
    assert!(pdf_text(&malformed).is_err());
}

#[tokio::test]
async fn text_and_html_reads_match_the_reference_including_unicode_offsets() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../../tests/fixtures/file_reads.json")).unwrap();
    for case in cases {
        let (client, requests, handle) = mock_with(|base| {
            let metadata = case["sections"]
                .to_string()
                .replace("https://campus.example/moodle", base);
            vec![
                response(200, &metadata).into_bytes(),
                binary(
                    case["body"].as_str().unwrap().as_bytes(),
                    case["content_type"].as_str().unwrap(),
                ),
            ]
        });
        let value = read(&client, read_file(case["offset"].as_u64().unwrap()))
            .await
            .unwrap();
        let normalized = value
            .to_string()
            .replace(&client.base_url, "https://campus.example/moodle");
        assert_eq!(
            serde_json::from_str::<Value>(&normalized).unwrap(),
            case["expected"]
        );
        let requests: Vec<_> = requests.try_iter().collect();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].ends_with("token=secret123"));
        assert!(!requests[1].lines().next().unwrap().contains("token="));
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn read_rejects_html_logins_json_errors_and_oversized_files() {
    for (name, body, mime, message) in [
        (
            "apuntes.txt",
            b"<html>Login</html>".as_slice(),
            "text/html",
            "página HTML",
        ),
        (
            "apuntes.txt",
            b"{}".as_slice(),
            "application/json",
            "permiso de descarga",
        ),
        (
            "apuntes.pdf",
            b"not a pdf".as_slice(),
            "application/pdf",
            "no es un PDF",
        ),
    ] {
        let (client, _requests, handle) = mock_with(|base| {
            vec![
                response(200, &sections(base, name, body.len() as u64).to_string()).into_bytes(),
                binary(body, mime),
            ]
        });
        assert!(
            read(&client, read_file(0))
                .await
                .unwrap_err()
                .contains(message)
        );
        handle.join().unwrap();
    }
    for (name, size) in [("apuntes.txt", MAX_BYTES as u64 + 1), ("apuntes.docx", 1)] {
        let (client, requests, handle) = mock_with(|base| {
            vec![response(200, &sections(base, name, size).to_string()).into_bytes()]
        });
        assert!(read(&client, read_file(0)).await.is_err());
        assert_eq!(requests.try_iter().count(), 1);
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn pdf_reads_run_through_the_binary_download_and_extraction_path() {
    let body = include_bytes!("../../tests/fixtures/text.pdf");
    let (client, _requests, handle) = mock_with(|base| {
        vec![
            response(
                200,
                &sections(base, "apuntes.pdf", body.len() as u64).to_string(),
            )
            .into_bytes(),
            binary(body, "application/pdf"),
        ]
    });
    let result = read(&client, read_file(0)).await.unwrap();
    assert!(
        result["text"]
            .as_str()
            .unwrap()
            .contains("Apuntes de prueba")
    );
    assert_eq!(result["next_offset"], Value::Null);
    handle.join().unwrap();
}

#[tokio::test]
async fn missing_modules_indices_and_unenrolled_courses_never_download() {
    for (module_id, file_index) in [(999, 0), (90, 999)] {
        let (client, requests, handle) = mock_with(|base| {
            vec![response(200, &sections(base, "apuntes.txt", 1).to_string()).into_bytes()]
        });
        assert!(
            read(
                &client,
                ReadFile {
                    course_id: 7,
                    module_id,
                    file_index,
                    offset: 0
                }
            )
            .await
            .unwrap_err()
            .contains("No se encontró")
        );
        assert_eq!(requests.try_iter().count(), 1);
        handle.join().unwrap();
    }
    let directory = tempfile::tempdir().unwrap();
    let (client, requests, handle) = mock_with(|_| {
        vec![
            response(200, "{\"userid\":8}").into_bytes(),
            response(200, "[{\"id\":9}]").into_bytes(),
        ]
    });
    let root = directory.path().join("downloads");
    assert!(
        download(
            &client,
            file(),
            &directory.path().join("downloads.json"),
            &root
        )
        .await
        .unwrap_err()
        .contains("no está entre")
    );
    assert!(!root.exists());
    assert_eq!(requests.try_iter().count(), 2);
    handle.join().unwrap();
}

#[test]
fn downloaded_files_are_reused_or_numbered_without_overwriting() {
    let directory = tempfile::tempdir().unwrap();
    let wanted = directory.path().join("Tema 1.pdf");
    let save = |body: &[u8]| {
        let mut temp = NamedTempFile::new_in(directory.path()).unwrap();
        temp.write_all(body).unwrap();
        place(temp, &wanted).unwrap()
    };
    let (first, created) = save(b"v1");
    assert!(created);
    assert_eq!(save(b"v1"), (first.clone(), false));
    let (other, created) = save(b"v2");
    assert!(created);
    assert_eq!(other.file_name().unwrap(), "Tema 1 (1).pdf");
    assert_eq!(fs::read(first).unwrap(), b"v1");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn simultaneous_downloads_do_not_overwrite_or_leave_temporary_files() {
    let directory = tempfile::tempdir().unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let barrier = barrier.clone();
        let root = directory.path().to_owned();
        threads.push(std::thread::spawn(move || {
            let mut temp = NamedTempFile::new_in(&root).unwrap();
            temp.write_all(b"same content").unwrap();
            barrier.wait();
            place(temp, &root.join("apuntes.txt")).unwrap()
        }));
    }
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(results[0].0, results[1].0);
    assert_eq!(results.iter().filter(|(_, created)| *created).count(), 1);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn existing_symlinks_are_never_used_as_download_targets() {
    let directory = tempfile::tempdir().unwrap();
    let outside = directory.path().join("original");
    fs::write(&outside, "private").unwrap();
    let target = directory.path().join("apuntes.txt");
    std::os::unix::fs::symlink(&outside, &target).unwrap();
    let mut temp = NamedTempFile::new_in(directory.path()).unwrap();
    temp.write_all(b"new").unwrap();
    let (saved, _) = place(temp, &target).unwrap();
    assert_eq!(saved.file_name().unwrap(), "apuntes (1).txt");
    assert_eq!(fs::read(outside).unwrap(), b"private");
}

#[cfg(unix)]
#[tokio::test]
async fn a_course_folder_symlink_cannot_redirect_downloads_outside_the_chosen_root() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("downloads");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("Control")).unwrap();
    let (client, requests, handle) = mock_with(|base| {
        vec![
            response(200, "{\"userid\":8}").into_bytes(),
            response(200, "[{\"id\":7,\"fullname\":\"Control\"}]").into_bytes(),
            response(200, &sections(base, "apuntes.txt", 1).to_string()).into_bytes(),
        ]
    });
    assert!(
        download(
            &client,
            file(),
            &directory.path().join("settings.json"),
            &root
        )
        .await
        .unwrap_err()
        .contains("sale de la carpeta")
    );
    assert_eq!(requests.try_iter().count(), 3);
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    handle.join().unwrap();
}

#[tokio::test]
async fn downloads_stream_to_the_chosen_folder_with_redacted_origin_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let chosen = directory.path().join("Universidad");
    fs::create_dir(&chosen).unwrap();
    let settings = directory.path().join("downloads.json");
    fs::write(&settings, json!({"folder": chosen}).to_string()).unwrap();
    let (client, requests, handle) = mock_with(|base| {
        vec![
            response(200, "{\"userid\":8}").into_bytes(),
            response(200, "[{\"id\":7,\"fullname\":\"Control <b>Avanzado</b>\"}]").into_bytes(),
            response(200, &sections(base, "Diapositivas.pptx", 10).to_string()).into_bytes(),
            binary(b"PK presentacion", "application/octet-stream"),
        ]
    });
    let result = download(
        &client,
        file(),
        &settings,
        &directory.path().join("Default"),
    )
    .await
    .unwrap();
    let path = Path::new(result["path"].as_str().unwrap());
    assert_eq!(path, chosen.join("Control Avanzado/Diapositivas.pptx"));
    assert_eq!(fs::read(path).unwrap(), b"PK presentacion");
    assert_eq!(result["already_downloaded"], false);
    assert_eq!(result["size_bytes"], 15);
    assert!(!result.to_string().contains("secret123"));
    assert!(!result.to_string().contains("token=old"));
    #[cfg(windows)]
    {
        let mark = fs::read_to_string(format!("{}:Zone.Identifier", path.display())).unwrap();
        assert!(mark.contains("ZoneId=3"));
        assert!(!mark.contains("token=old"));
    }
    #[cfg(target_os = "macos")]
    {
        let mark = std::process::Command::new("/usr/bin/xattr")
            .args(["-p", "com.apple.quarantine"])
            .arg(path)
            .output()
            .unwrap();
        assert!(mark.status.success());
        assert!(
            String::from_utf8(mark.stdout)
                .unwrap()
                .contains("Mudel AI Connect")
        );
    }
    assert_eq!(requests.try_iter().count(), 4);
    handle.join().unwrap();
}

#[tokio::test]
async fn rejected_downloads_leave_no_files_or_partial_downloads() {
    let directory = tempfile::tempdir().unwrap();
    for (name, size, body) in [
        ("setup.exe", 1, None),
        ("run.command", 1, None),
        ("apuntes.txt", MAX_DOWNLOAD_BYTES as u64 + 1, None),
        ("apuntes.txt", 1, Some(b"{}".as_slice())),
    ] {
        let (client, requests, handle) = mock_with(|base| {
            let mut responses = vec![
                response(200, "{\"userid\":8}").into_bytes(),
                response(200, "[{\"id\":7}]").into_bytes(),
                response(200, &sections(base, name, size).to_string()).into_bytes(),
            ];
            if let Some(body) = body {
                responses.push(binary(body, "application/json"));
            }
            responses
        });
        let root = directory.path().join("downloads");
        assert!(
            download(
                &client,
                file(),
                &directory.path().join("settings.json"),
                &root
            )
            .await
            .is_err()
        );
        assert_eq!(
            requests.try_iter().count(),
            if body.is_some() { 4 } else { 3 }
        );
        if root.exists() {
            for course in fs::read_dir(&root).unwrap() {
                assert_eq!(fs::read_dir(course.unwrap().path()).unwrap().count(), 0);
            }
        }
        handle.join().unwrap();
    }
}

#[test]
fn missing_or_invalid_chosen_folders_fall_back_without_creating_them() {
    let directory = tempfile::tempdir().unwrap();
    let settings = directory.path().join("downloads.json");
    let default = directory.path().join("Default");
    assert_eq!(download_root(&settings, &default), (default.clone(), None));
    for value in [
        "roto".to_owned(),
        json!({"folder": "relative"}).to_string(),
        json!({"folder": directory.path().join("USB ausente")}).to_string(),
    ] {
        fs::write(&settings, value).unwrap();
        let (path, warning) = download_root(&settings, &default);
        assert_eq!(path, default);
        assert!(warning.is_some());
    }
    assert!(!directory.path().join("USB ausente").exists());
    assert!(default_download_folder().unwrap().is_absolute());
}
