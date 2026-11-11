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
        download_folder: Ok(temporary()),
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
    let read = server
        .read_course_file(Parameters(ReadFile {
            course_id: 7,
            module_id: 90,
            file_index: 0,
            offset: 0,
            with_figures: false,
        }))
        .await;
    assert!(read.is_error.unwrap_or(false));
    assert!(
        read.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("desactivada")
    );
    let download = server
        .download_course_file(Parameters(CourseFile {
            course_id: 7,
            module_id: 90,
            file_index: 0,
        }))
        .await;
    assert!(download.is_error.unwrap_or(false));
    assert!(
        download.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("desactivada")
    );
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
async fn five_tools_match_the_reference_contract() {
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

#[tokio::test]
async fn study_prompt_embeds_the_instructions_and_the_request() {
    let (client, _requests, _handle) = mock(vec![]);
    let server = server(client);
    let text = |messages: Vec<PromptMessage>| {
        serde_json::to_value(&messages[0]).unwrap()["content"]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let default = text(
        server
            .estudiar_curso(Parameters(Study {
                course_id: "7".into(),
                pregunta: None,
            }))
            .await,
    );
    assert_eq!(
        default,
        format!(
            "{INSTRUCTIONS}\nCurso solicitado: 7.\nPetición del usuario: Ayúdame a estudiar esta asignatura\nCarga el contexto del curso y lee los materiales relevantes. Separa lo que aparece en las fuentes de tu explicación y cita sus enlaces."
        )
    );
    let custom = text(
        server
            .estudiar_curso(Parameters(Study {
                course_id: "7".into(),
                pregunta: Some("Explica el tema 1".into()),
            }))
            .await,
    );
    assert!(custom.contains("Curso solicitado: 7.\nPetición del usuario: Explica el tema 1\n"));
    assert!(custom.starts_with(INSTRUCTIONS));
}

fn call_body(request: &str) -> String {
    request.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

#[tokio::test]
async fn assignment_status_reports_submission_grade_and_teacher_comments() {
    let body = json!({"lastattempt": {"submission": {"id": 1, "status": "submitted", "timemodified": 1700000000},
            "graded": true, "locked": false, "cansubmit": false, "extensionduedate": 0},
        "feedback": {"gradefordisplay": "<b>8,50</b> / 10,00", "gradeddate": 1700003600,
            "plugins": [{"type": "comments", "editorfields": [{"name": "comments", "text": "<p>Muy bien &amp; claro</p>"}]}]},
        "warnings": []});
    let (client, requests, handle) = mock(vec![response(200, &body.to_string())]);
    let server = server(client);
    let value = server.assignment_status(5).await.unwrap();
    assert_eq!(value["submission_status"], "submitted");
    assert_eq!(value["submitted"], true);
    assert_eq!(value["graded"], true);
    assert_eq!(value["grade"], "8,50 / 10,00");
    assert_eq!(value["graded_at_utc"], "2023-11-14T23:13:20+00:00");
    assert_eq!(value["teacher_feedback"][0]["text"], "Muy bien & claro");
    assert_eq!(value["extension_due_utc"], Value::Null);
    let request = requests.try_iter().next().unwrap();
    let body = call_body(&request);
    assert!(body.contains("wsfunction=mod_assign_get_submission_status"));
    assert!(body.contains("assignid=5"));
    handle.join().unwrap();

    // Sin entrega todavía: no se inventa una fecha de modificación.
    let body = json!({"lastattempt": {"submission": {"status": "new", "timemodified": 1700000000},
        "graded": false, "cansubmit": true}, "feedback": {}});
    let (client, _requests, handle) = mock(vec![response(200, &body.to_string())]);
    let value = server_for(client).assignment_status(5).await.unwrap();
    assert_eq!(value["submitted"], false);
    assert_eq!(value["last_modified_utc"], Value::Null);
    handle.join().unwrap();
}

fn server_for(client: MoodleClient) -> MoodleServer {
    server(client)
}

#[tokio::test]
async fn grades_use_the_user_of_the_token_and_keep_teacher_comments() {
    let info = json!({"userid": 8, "fullname": "Alumno"});
    let grades = json!({"usergrades": [{"gradeitems": [
        {"itemname": "Examen parcial", "itemtype": "mod", "itemmodule": "quiz",
         "gradeformatted": "7,00", "rangeformatted": "0–10", "percentageformatted": "70 %",
         "feedback": "<p>Bien</p>"},
        {"itemtype": "course", "gradeformatted": "-", "rangeformatted": "0–100"}]}], "warnings": []});
    let (client, requests, handle) = mock(vec![
        response(200, &info.to_string()),
        response(200, &grades.to_string()),
    ]);
    let value = server(client).grades(7).await.unwrap();
    assert_eq!(value["items"][0]["name"], "Examen parcial");
    assert_eq!(value["items"][0]["grade"], "7,00");
    assert_eq!(value["items"][0]["feedback"], "Bien");
    assert_eq!(value["items"][1]["name"], "");
    let requests: Vec<_> = requests.try_iter().collect();
    assert!(call_body(&requests[1]).contains("wsfunction=gradereport_user_get_grade_items"));
    assert!(call_body(&requests[1]).contains("userid=8"));
    handle.join().unwrap();
}

#[tokio::test]
async fn forums_discussions_and_posts_are_listed_with_plain_text() {
    let forums = json!([{"id": 3, "cmid": 55, "name": "Avisos", "type": "news",
        "intro": "<p>Avisos del profesor</p>", "numdiscussions": 4}]);
    let discussions = json!({"discussions": [{"discussion": 9, "name": "Cambio de aula",
        "userfullname": "Profesor", "created": 1700000000, "timemodified": 1700000100,
        "numreplies": 2, "pinned": true, "message": "<p>La clase será en el <b>aula 3</b></p>"}]});
    let posts = json!({"posts": [{"id": 1, "parentid": 0, "subject": "Cambio de aula",
        "author": {"fullname": "Profesor"}, "timecreated": 1700000000, "message": "<p>Hola</p>"},
        {"id": 2, "parentid": 1, "subject": "Re: Cambio de aula",
        "author": {"fullname": "Alumno"}, "timecreated": 1700000200, "message": "<p>Gracias</p>"}]});
    let (client, requests, handle) = mock(vec![
        response(200, &forums.to_string()),
        response(200, &discussions.to_string()),
        response(200, &posts.to_string()),
    ]);
    let server = server(client);
    let base = server.client.as_ref().unwrap().base_url.clone();
    let listed = server.forums(7).await.unwrap();
    assert_eq!(listed["forums"][0]["type"], "news");
    assert_eq!(
        listed["forums"][0]["url"],
        format!("{base}/mod/forum/view.php?id=55")
    );
    let found = server
        .forum_discussions(Forum {
            forum_id: 3,
            page: 0,
        })
        .await
        .unwrap();
    assert_eq!(found["discussions"][0]["discussion_id"], 9);
    assert_eq!(
        found["discussions"][0]["message"],
        "La clase será en el aula 3"
    );
    assert_eq!(found["discussions"][0]["pinned"], true);
    assert_eq!(found["next_page"], Value::Null);
    let thread = server.discussion_posts(9).await.unwrap();
    assert_eq!(thread["posts"][1]["reply_to"], 1);
    assert_eq!(thread["posts"][1]["author"], "Alumno");
    let requests: Vec<_> = requests.try_iter().collect();
    assert!(call_body(&requests[1]).contains("forumid=3"));
    assert!(call_body(&requests[1]).contains("perpage=10"));
    assert!(call_body(&requests[2]).contains("discussionid=9"));
    handle.join().unwrap();
}

#[tokio::test]
async fn a_full_forum_page_offers_the_next_one() {
    let items: Vec<Value> = (1..=10)
        .map(|n| json!({"discussion": n, "name": format!("Tema {n}"), "message": "<p>x</p>"}))
        .collect();
    let (client, _requests, handle) = mock(vec![response(
        200,
        &json!({"discussions": items}).to_string(),
    )]);
    let value = server(client)
        .forum_discussions(Forum {
            forum_id: 3,
            page: 1,
        })
        .await
        .unwrap();
    assert_eq!(value["next_page"], 2);
    handle.join().unwrap();
}

fn course_with_search_material() -> String {
    json!([{"id": 1, "name": "Tema 1: Álgebra", "summary": "", "modules": [
        {"id": 90, "name": "Apuntes de derivadas", "modname": "resource", "description": "",
         "url": "https://campus.example/m/90", "contents": [
            {"type": "file", "filename": "Ejercicios.PDF"}, {"type": "file", "filename": "teoria.pdf"}]},
        {"id": 91, "name": "Foro de dudas", "modname": "forum", "description": "Pregunta aquí"}]},
      {"id": 2, "name": "Tema 2", "summary": "", "modules": []}])
    .to_string()
}

#[tokio::test]
async fn search_ignores_case_and_accents_and_needs_a_match_in_the_module_itself() {
    for (query, expected_kinds, expected_files) in [
        (
            "DERIVADAS",
            vec!["module"],
            vec!["Ejercicios.PDF", "teoria.pdf"],
        ),
        ("ejercicios", vec!["module"], vec!["Ejercicios.PDF"]),
        ("algebra", vec!["section"], vec![]),
        (
            "tema derivadas",
            vec!["module"],
            vec!["Ejercicios.PDF", "teoria.pdf"],
        ),
        ("pregunta aqui", vec!["module"], vec![]),
        ("inexistente", vec![], vec![]),
    ] {
        let (client, _requests, handle) = mock(vec![response(200, &course_with_search_material())]);
        let value = server(client)
            .search(Search {
                query: query.into(),
                course_id: Some(7),
            })
            .await
            .unwrap();
        let results = value["results"].as_array().unwrap();
        let kinds: Vec<_> = results
            .iter()
            .map(|r| r["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, expected_kinds, "{query}");
        let files: Vec<_> = results
            .iter()
            .flat_map(|r| r["files"].as_array().into_iter().flatten())
            .map(|f| f["filename"].as_str().unwrap())
            .collect();
        assert_eq!(files, expected_files, "{query}");
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn search_without_a_course_looks_in_every_enrolled_course() {
    let info = json!({"userid": 8});
    let enrolled = json!([{"id": 1, "fullname": "Cálculo"}, {"id": 2, "fullname": "Física"}]);
    let (client, requests, handle) = mock(vec![
        response(200, &info.to_string()),
        response(200, &enrolled.to_string()),
        response(200, &course_with_search_material()),
        response(200, "[]"),
    ]);
    let value = server(client)
        .search(Search {
            query: "derivadas".into(),
            course_id: None,
        })
        .await
        .unwrap();
    assert_eq!(value["results"][0]["course_name"], "Cálculo");
    assert_eq!(value["results"][0]["files"][0]["file_index"], 0);
    assert_eq!(requests.try_iter().count(), 4);
    handle.join().unwrap();
}

#[tokio::test]
async fn book_chapters_get_their_titles_in_the_course_context() {
    let structure = json!([{"title": "Introducción", "href": "1/index.html", "level": 0,
        "subitems": [{"title": "Primeros pasos", "href": "2/index.html", "level": 1, "subitems": []}]}]);
    let sections = json!([{"id": 1, "name": "Tema", "modules": [{"id": 70, "name": "Manual",
        "modname": "book", "contents": [
        {"type": "content", "filename": "structure", "content": structure.to_string()},
        {"type": "file", "filename": "index.html", "filepath": "/1/", "fileurl": "https://x/1"},
        {"type": "file", "filename": "index.html", "filepath": "/2/", "fileurl": "https://x/2"},
        {"type": "file", "filename": "index.html", "filepath": "/9/", "fileurl": "https://x/9"}]}]}]);
    let (client, _requests, handle) = mock(vec![response(200, &sections.to_string())]);
    let value = server(client)
        .context(Sections {
            course_id: 7,
            section_start: 0,
            section_limit: 3,
        })
        .await
        .unwrap();
    let contents = &value["sections"][0]["modules"][0]["contents"];
    assert_eq!(contents[1]["chapter_title"], "Introducción");
    assert_eq!(contents[2]["chapter_title"], "Primeros pasos");
    assert!(contents[3].get("chapter_title").is_none());
    handle.join().unwrap();
}

#[tokio::test]
async fn new_tools_respect_the_permissions_and_reject_invalid_ids() {
    let (client, requests, handle) = mock(vec![]);
    let server = server(client);
    for result in [
        server.assignment_status(0).await,
        server.grades(0).await,
        server.forums(-1).await,
        server.discussion_posts(0).await,
        server
            .forum_discussions(Forum {
                forum_id: 0,
                page: 0,
            })
            .await,
        server
            .search(Search {
                query: " ".into(),
                course_id: None,
            })
            .await,
    ] {
        assert!(result.is_err());
    }
    assert_eq!(requests.try_iter().count(), 0);
    // La nota y la entrega dependen del permiso de tareas; foros y búsqueda, del de materiales.
    let path = server.permissions_path.clone().unwrap();
    fs::write(
        &path,
        r#"{"courses": true, "materials": false, "assignments": false, "calendar": true, "downloads": true}"#,
    )
    .unwrap();
    for result in [
        server.assignment_status(5).await,
        server.grades(7).await,
        server.forums(7).await,
        server.discussion_posts(9).await,
        server
            .search(Search {
                query: "tema".into(),
                course_id: Some(7),
            })
            .await,
    ] {
        assert!(result.unwrap_err().contains("desactivada"));
    }
    assert_eq!(requests.try_iter().count(), 0);
    fs::remove_file(path).unwrap();
    drop(handle);
}

#[tokio::test]
async fn null_and_numeric_fields_from_moodle_do_not_break_the_new_tools() {
    // El total del curso llega sin nombre (null) y algunas notas como número.
    let info = json!({"userid": 8});
    let grades = json!({"usergrades": [{"gradeitems": [
        {"itemname": null, "itemtype": "course", "gradeformatted": null, "rangeformatted": "0–100",
         "percentageformatted": 55, "feedback": null}]}]});
    let (client, _requests, handle) = mock(vec![
        response(200, &info.to_string()),
        response(200, &grades.to_string()),
    ]);
    let value = server(client).grades(7).await.unwrap();
    assert_eq!(value["items"][0]["name"], "");
    assert_eq!(value["items"][0]["grade"], "");
    assert_eq!(value["items"][0]["percentage"], "55");
    handle.join().unwrap();

    let discussions = json!({"discussions": [{"discussion": 4, "name": null, "userfullname": null,
        "message": null, "created": 0, "timemodified": null}]});
    let (client, _requests, handle) = mock(vec![response(200, &discussions.to_string())]);
    let value = server(client)
        .forum_discussions(Forum {
            forum_id: 3,
            page: 0,
        })
        .await
        .unwrap();
    assert_eq!(value["discussions"][0]["title"], "");
    handle.join().unwrap();
}

#[tokio::test]
async fn guided_prompts_name_the_right_tools_and_fill_in_their_arguments() {
    let (client, _requests, _handle) = mock(vec![]);
    let server = server(client);
    let text = |messages: Vec<PromptMessage>| {
        serde_json::to_value(&messages[0]).unwrap()["content"]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let exam = text(
        server
            .preparar_examen(Parameters(Exam {
                course_id: "7".into(),
                tema: Some("termopares".into()),
            }))
            .await,
    );
    assert!(exam.starts_with(INSTRUCTIONS));
    assert!(exam.contains("Curso solicitado: 7.\nTema: termopares."));
    for tool in ["search_course", "read_course_file", "view_course_file"] {
        assert!(exam.contains(tool), "{tool}");
    }
    assert!(exam.contains("No muestres las soluciones hasta que el usuario responda"));
    let whole = text(
        server
            .preparar_examen(Parameters(Exam {
                course_id: "7".into(),
                tema: None,
            }))
            .await,
    );
    assert!(whole.contains("Tema: todo el temario."));

    let plan = text(
        server
            .plan_de_estudio(Parameters(Plan {
                course_id: None,
                hasta: None,
            }))
            .await,
    );
    assert!(
        plan.contains("Curso: todas mis asignaturas.\nFecha límite: la próxima entrega o examen.")
    );
    for tool in [
        "get_upcoming_events",
        "list_assignments",
        "get_assignment_status",
    ] {
        assert!(plan.contains(tool), "{tool}");
    }
    let dated = text(
        server
            .plan_de_estudio(Parameters(Plan {
                course_id: Some("7".into()),
                hasta: Some("el 20 de noviembre".into()),
            }))
            .await,
    );
    assert!(dated.contains("Curso: 7.\nFecha límite: el 20 de noviembre."));

    let summary = text(
        server
            .resumir_tema(Parameters(Topic {
                course_id: "7".into(),
                tema: "Tema 3".into(),
            }))
            .await,
    );
    assert!(summary.contains("Curso solicitado: 7.\nTema: Tema 3."));
    assert!(summary.contains("read_course_files"));
}
