use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, ContentBlock, Implementation, PromptMessage, Role, ServerCapabilities,
        ServerConfig,
    },
    prompt, prompt_handler, prompt_router, schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::credentials;
use crate::files::{self, CourseFile, ReadFile, ReadFiles};
use crate::moodle::{
    FORMAT_ERROR, FUNCTIONS, MoodleClient, array, id, optional_array, plain, text, utc,
};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

const PERMISSION_ERROR: &str =
    "No se pudieron leer los permisos. Vuelve a guardarlos desde el instalador.";
const PERMISSIONS: [(&str, &str); 5] = [
    ("courses", "Asignaturas"),
    ("materials", "Materiales y archivos"),
    ("assignments", "Tareas"),
    ("calendar", "Calendario"),
    ("downloads", "Descargar archivos a tu carpeta Descargas"),
];
const INSTRUCTIONS: &str = include_str!("instructions.txt");

#[derive(Clone)]
pub struct MoodleServer {
    client: Result<MoodleClient, String>,
    permissions_path: Result<PathBuf, String>,
    download_folder: Result<PathBuf, String>,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Course {
    #[schemars(range(min = 1))]
    course_id: i64,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sections {
    #[schemars(range(min = 1))]
    course_id: i64,
    #[serde(default)]
    #[schemars(range(min = 0))]
    section_start: u64,
    #[serde(default = "default_section_limit")]
    #[schemars(range(min = 1, max = 10))]
    section_limit: u8,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    #[schemars(range(min = 1))]
    assignment_id: i64,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Forum {
    #[schemars(range(min = 1))]
    forum_id: i64,
    #[serde(default)]
    page: u32,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Discussion {
    #[schemars(range(min = 1))]
    discussion_id: i64,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Search {
    /// Palabras a buscar; deben aparecer todas. No distingue mayúsculas ni tildes.
    #[schemars(length(min = 2, max = 100))]
    query: String,
    /// Curso donde buscar. Sin él se buscan todas las asignaturas del usuario.
    #[schemars(range(min = 1))]
    course_id: Option<i64>,
}

const SEARCH_LIMIT: usize = 40;
const SEARCH_COURSES: usize = 50;
const SEARCH_PARALLEL: usize = 6;
const FORUM_PAGE: u32 = 10;

/// Texto plano de un campo que Moodle puede mandar vacío (`null`), como número o como HTML.
fn soft(value: &Value, key: &str, limit: usize) -> String {
    match &value[key] {
        Value::String(html) => text(html).chars().take(limit).collect(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

/// Minúsculas y sin tildes, para comparar textos.
fn fold(text: &str) -> String {
    text.nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Títulos de los capítulos de un libro de Moodle, por la carpeta de cada capítulo.
fn book_chapters(module: &Value) -> BTreeMap<String, String> {
    fn walk(items: &Value, found: &mut BTreeMap<String, String>) {
        for item in items.as_array().into_iter().flatten() {
            if let (Some(title), Some(href)) = (item["title"].as_str(), item["href"].as_str())
                && let Some((folder, _)) = href.rsplit_once('/')
            {
                found.insert(format!("/{folder}/"), title.to_string());
            }
            walk(&item["subitems"], found);
        }
    }
    let mut found = BTreeMap::new();
    for item in module["contents"].as_array().into_iter().flatten() {
        if item["filename"] == "structure"
            && let Some(structure) = item["content"]
                .as_str()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
        {
            walk(&structure, &mut found);
        }
    }
    found
}

fn default_section_limit() -> u8 {
    3
}

fn permissions_path() -> Result<PathBuf, String> {
    credentials::data_dir()
        .map(|path| path.join("permissions.json"))
        .map_err(|_| PERMISSION_ERROR.into())
}

fn require_permission(path: &Path, permission: &str) -> Result<(), String> {
    let content = match fs::read(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(PERMISSION_ERROR.into()),
    };
    let mut values: BTreeMap<String, bool> =
        serde_json::from_slice(&content).map_err(|_| PERMISSION_ERROR)?;
    if !values.contains_key("downloads")
        && let Some(&materials) = values.get("materials")
    {
        values.insert("downloads".into(), materials);
    }
    if values.len() != PERMISSIONS.len()
        || PERMISSIONS
            .iter()
            .any(|(key, _)| !values.contains_key(*key))
    {
        return Err(PERMISSION_ERROR.into());
    }
    let label = PERMISSIONS
        .iter()
        .find(|(key, _)| *key == permission)
        .ok_or(PERMISSION_ERROR)?
        .1;
    if !values[permission] {
        return Err(format!("Consulta desactivada por el usuario: {label}."));
    }
    Ok(())
}

impl MoodleServer {
    pub fn from_env() -> Self {
        Self::from_account(credentials::Kind::Installed)
    }

    pub fn from_account(kind: credentials::Kind) -> Self {
        Self::with_client(
            credentials::load(kind)
                .and_then(|account| MoodleClient::new(&account.url, &account.token)),
        )
    }

    pub fn with_client(client: Result<MoodleClient, String>) -> Self {
        Self {
            client,
            permissions_path: permissions_path(),
            download_folder: files::default_download_folder(),
            tool_router: Self::tool_router(),
        }
    }

    fn client(&self, permission: Option<&str>) -> Result<&MoodleClient, String> {
        if let Some(permission) = permission {
            require_permission(
                self.permissions_path.as_ref().map_err(Clone::clone)?,
                permission,
            )?;
        }
        self.client.as_ref().map_err(Clone::clone)
    }

    fn result(&self, result: Result<Value, String>) -> CallToolResult {
        match result {
            Ok(value) => match &self.client {
                Ok(client) => CallToolResult::structured(client.clean(value)),
                Err(_) => CallToolResult::error(vec![ContentBlock::text(
                    "Falta configurar la cuenta de Moodle.",
                )]),
            },
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error)]),
        }
    }

    pub async fn status(&self) -> Result<Value, String> {
        self.client(None)?.status().await
    }

    async fn courses(&self) -> Result<Value, String> {
        let client = self.client(Some("courses"))?;
        let info = client.call(FUNCTIONS[0], &[]).await?;
        let courses = client
            .call(
                FUNCTIONS[1],
                &[("userid", id(&info, "userid")?.to_string())],
            )
            .await?;
        let courses = array(&courses)?
            .iter()
            .map(|course| {
                let course_id = id(course, "id")?;
                Ok(
                    json!({"id": course_id, "name": plain(course, "fullname", usize::MAX)?,
                "shortname": course["shortname"], "summary": plain(course, "summary", 4000)?,
                "url": format!("{}/course/view.php?id={course_id}", client.base_url)}),
                )
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(json!({"courses": courses}))
    }

    async fn context(&self, params: Sections) -> Result<Value, String> {
        if params.course_id <= 0 || !(1..=10).contains(&params.section_limit) {
            return Err(
                "course_id debe ser positivo y section_limit debe estar entre 1 y 10.".into(),
            );
        }
        let client = self.client(Some("materials"))?;
        let sections = client
            .call(FUNCTIONS[2], &[("courseid", params.course_id.to_string())])
            .await?;
        let sections = array(&sections)?;
        let start = usize::try_from(params.section_start)
            .unwrap_or(usize::MAX)
            .min(sections.len());
        let selected = sections.iter().skip(start).take(params.section_limit.into()).map(|section| {
            let modules = optional_array(section, "modules")?.iter().map(|module| {
                let chapters = book_chapters(module);
                let contents = optional_array(module, "contents")?.iter().enumerate().map(|(index, item)| {
                    let mut content = serde_json::Map::new();
                    for key in ["type", "filename", "fileurl", "filesize", "mimetype", "filepath"] {
                        if let Some(value) = item.get(key) { content.insert(key.into(), value.clone()); }
                    }
                    content.insert("file_index".into(), json!(index));
                    if let Some(title) = item["filepath"].as_str().and_then(|path| chapters.get(path)) {
                        content.insert("chapter_title".into(), json!(title));
                    }
                    content.insert("inline_text".into(), json!(plain(item, "content", 4000)?));
                    Ok(Value::Object(content))
                }).collect::<Result<Vec<_>, String>>()?;
                Ok(json!({"id": id(module, "id")?, "name": plain(module, "name", usize::MAX)?,
                    "type": module["modname"], "url": module["url"],
                    "description": plain(module, "description", 4000)?, "contents": contents}))
            }).collect::<Result<Vec<_>, String>>()?;
            Ok(json!({"id": id(section, "id")?, "name": plain(section, "name", usize::MAX)?,
                "summary": plain(section, "summary", 4000)?, "modules": modules}))
        }).collect::<Result<Vec<_>, String>>()?;
        let end = start + selected.len();
        Ok(json!({"course_id": params.course_id,
            "url": format!("{}/course/view.php?id={}", client.base_url, params.course_id),
            "fetched_at": chrono::Utc::now().to_rfc3339(), "total_sections": sections.len(),
            "sections": selected, "inline_text_limit": 4000,
            "next_section_start": if end < sections.len() { Some(end) } else { None }}))
    }

    async fn assignments(&self, course_id: i64) -> Result<Value, String> {
        if course_id <= 0 {
            return Err("course_id debe ser positivo.".into());
        }
        let client = self.client(Some("assignments"))?;
        let result = client
            .call(FUNCTIONS[3], &[("courseids[0]", course_id.to_string())])
            .await?;
        if !result.is_object() {
            return Err(FORMAT_ERROR.into());
        }
        let mut assignments = Vec::new();
        for course in optional_array(&result, "courses")? {
            for task in optional_array(course, "assignments")? {
                assignments.push(json!({"id": id(task, "id")?, "name": plain(task, "name", usize::MAX)?,
                    "description": plain(task, "intro", 12000)?, "due_at_utc": utc(&task["duedate"])?,
                    "cutoff_at_utc": utc(&task["cutoffdate"])?, "opens_at_utc": utc(&task["allowsubmissionsfromdate"])?,
                    "url": format!("{}/mod/assign/view.php?id={}", client.base_url, id(task, "cmid")?)}));
            }
        }
        Ok(
            json!({"assignments": assignments, "warnings": result.get("warnings").cloned().unwrap_or(json!([])),
            "note": "Plazos devueltos por Moodle; no se ha consultado el estado de entrega (usa get_assignment_status con el id de la tarea). Verifica posibles prórrogas personales en SIFO."}),
        )
    }

    async fn assignment_status(&self, assignment_id: i64) -> Result<Value, String> {
        if assignment_id <= 0 {
            return Err("assignment_id debe ser positivo.".into());
        }
        let client = self.client(Some("assignments"))?;
        let result = client
            .call(
                "mod_assign_get_submission_status",
                &[("assignid", assignment_id.to_string())],
            )
            .await?;
        if !result.is_object() {
            return Err(FORMAT_ERROR.into());
        }
        let attempt = &result["lastattempt"];
        let submission = &attempt["submission"];
        let feedback = &result["feedback"];
        let mut comments = Vec::new();
        for plugin in optional_array(feedback, "plugins")? {
            for field in optional_array(plugin, "editorfields")? {
                let comment = soft(field, "text", 4000);
                if !comment.is_empty() {
                    comments.push(json!({"type": plugin["type"], "text": comment}));
                }
            }
        }
        let status = submission["status"].as_str().unwrap_or("new");
        Ok(json!({"assignment_id": assignment_id,
            "submission_status": status,
            "submitted": matches!(status, "submitted"),
            "last_modified_utc": if status == "new" { Value::Null } else { utc(&submission["timemodified"])? },
            "graded": attempt["graded"],
            "grade": soft(feedback, "gradefordisplay", 200),
            "graded_at_utc": utc(&feedback["gradeddate"])?,
            "teacher_feedback": comments,
            "locked": attempt["locked"], "can_submit": attempt["cansubmit"],
            "extension_due_utc": utc(&attempt["extensionduedate"])?,
            "warnings": result.get("warnings").cloned().unwrap_or(json!([])),
            "note": "Estado según Moodle en este momento. Un borrador no cuenta como entregado."}))
    }

    async fn grades(&self, course_id: i64) -> Result<Value, String> {
        if course_id <= 0 {
            return Err("course_id debe ser positivo.".into());
        }
        let client = self.client(Some("assignments"))?;
        let info = client.call(FUNCTIONS[0], &[]).await?;
        let result = client
            .call(
                "gradereport_user_get_grade_items",
                &[
                    ("courseid", course_id.to_string()),
                    ("userid", id(&info, "userid")?.to_string()),
                ],
            )
            .await?;
        if !result.is_object() {
            return Err(FORMAT_ERROR.into());
        }
        let mut items = Vec::new();
        if let Some(user) = optional_array(&result, "usergrades")?.first() {
            for item in optional_array(user, "gradeitems")? {
                items.push(json!({"name": soft(item, "itemname", 300),
                    "kind": item["itemtype"], "module": item["itemmodule"],
                    "grade": soft(item, "gradeformatted", 100),
                    "range": soft(item, "rangeformatted", 100),
                    "percentage": soft(item, "percentageformatted", 50),
                    "feedback": soft(item, "feedback", 2000)}));
            }
        }
        Ok(json!({"course_id": course_id, "items": items,
            "warnings": result.get("warnings").cloned().unwrap_or(json!([])),
            "note": "Calificaciones visibles para el alumno según Moodle; pueden faltar las que el profesor no ha publicado."}))
    }

    async fn forums(&self, course_id: i64) -> Result<Value, String> {
        if course_id <= 0 {
            return Err("course_id debe ser positivo.".into());
        }
        let client = self.client(Some("materials"))?;
        let result = client
            .call(
                "mod_forum_get_forums_by_courses",
                &[("courseids[0]", course_id.to_string())],
            )
            .await?;
        let forums = array(&result)?
            .iter()
            .map(|forum| {
                Ok(json!({"id": id(forum, "id")?, "name": soft(forum, "name", usize::MAX),
                    "type": forum["type"], "description": soft(forum, "intro", 1000),
                    "discussions": forum["numdiscussions"],
                    "url": format!("{}/mod/forum/view.php?id={}", client.base_url, id(forum, "cmid")?)}))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(json!({"course_id": course_id, "forums": forums,
            "note": "El foro de tipo news contiene los avisos del profesor."}))
    }

    async fn forum_discussions(&self, params: Forum) -> Result<Value, String> {
        if params.forum_id <= 0 {
            return Err("forum_id debe ser positivo.".into());
        }
        let client = self.client(Some("materials"))?;
        let result = client
            .call(
                "mod_forum_get_forum_discussions",
                &[
                    ("forumid", params.forum_id.to_string()),
                    ("sortorder", "-1".into()),
                    ("page", params.page.to_string()),
                    ("perpage", FORUM_PAGE.to_string()),
                ],
            )
            .await?;
        if !result.is_object() {
            return Err(FORMAT_ERROR.into());
        }
        let discussions = optional_array(&result, "discussions")?
            .iter()
            .map(|item| {
                Ok(json!({"discussion_id": id(item, "discussion")?,
                    "title": soft(item, "name", 300),
                    "author": soft(item, "userfullname", 200),
                    "created_utc": utc(&item["created"])?,
                    "modified_utc": utc(&item["timemodified"])?,
                    "replies": item["numreplies"], "pinned": item["pinned"],
                    "message": soft(item, "message", 4000)}))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let full_page = discussions.len() >= FORUM_PAGE as usize;
        Ok(json!({"forum_id": params.forum_id, "page": params.page,
            "next_page": if full_page { Some(params.page + 1) } else { None },
            "discussions": discussions}))
    }

    async fn discussion_posts(&self, discussion_id: i64) -> Result<Value, String> {
        if discussion_id <= 0 {
            return Err("discussion_id debe ser positivo.".into());
        }
        let client = self.client(Some("materials"))?;
        let result = client
            .call(
                "mod_forum_get_discussion_posts",
                &[
                    ("discussionid", discussion_id.to_string()),
                    ("sortby", "created".into()),
                    ("sortdirection", "ASC".into()),
                ],
            )
            .await?;
        if !result.is_object() {
            return Err(FORMAT_ERROR.into());
        }
        let posts = optional_array(&result, "posts")?
            .iter()
            .map(|post| {
                Ok(json!({"id": id(post, "id")?, "reply_to": post["parentid"],
                    "subject": soft(post, "subject", 300),
                    "author": soft(&post["author"], "fullname", 200),
                    "created_utc": utc(&post["timecreated"])?,
                    "message": soft(post, "message", 4000)}))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(json!({"discussion_id": discussion_id, "posts": posts}))
    }

    async fn search(&self, params: Search) -> Result<Value, String> {
        let terms: Vec<String> = fold(&params.query)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if terms.is_empty() || params.course_id.is_some_and(|id| id <= 0) {
            return Err("Indica un texto a buscar y, si lo usas, un course_id positivo.".into());
        }
        let client = self.client(Some("materials"))?;
        let courses: Vec<(i64, Option<String>)> = match params.course_id {
            Some(course_id) => vec![(course_id, None)],
            None => {
                let info = client.call(FUNCTIONS[0], &[]).await?;
                let enrolled = client
                    .call(
                        FUNCTIONS[1],
                        &[("userid", id(&info, "userid")?.to_string())],
                    )
                    .await?;
                array(&enrolled)?
                    .iter()
                    .take(SEARCH_COURSES)
                    .map(|course| {
                        Ok((
                            id(course, "id")?,
                            Some(plain(course, "fullname", usize::MAX)?),
                        ))
                    })
                    .collect::<Result<_, String>>()?
            }
        };
        let matches_all = |text: &str| {
            let text = fold(text);
            terms.iter().all(|term| text.contains(term.as_str()))
        };
        // Debe coincidir algo del propio elemento; el nombre de la sección solo ayuda a
        // completar la búsqueda («tema 1 derivadas»), no basta por sí solo.
        let own_match = |own: &str, section: &str| {
            matches_all(&format!("{section} {own}"))
                && terms.iter().any(|term| fold(own).contains(term.as_str()))
        };
        let mut found = Vec::new();
        let mut truncated = false;
        let mut skipped = 0usize;
        'courses: for chunk in courses.chunks(SEARCH_PARALLEL) {
            // Los cursos de cada tanda se consultan a la vez; el orden de los resultados no cambia.
            let mut tasks = tokio::task::JoinSet::new();
            for (position, (course_id, _)) in chunk.iter().enumerate() {
                let (client, course_id) = (client.clone(), *course_id);
                tasks.spawn(async move { (position, client.course_sections(course_id).await) });
            }
            let mut fetched = Vec::new();
            while let Some(joined) = tasks.join_next().await {
                fetched.push(joined.map_err(|_| "La búsqueda se interrumpió.".to_string())?);
            }
            fetched.sort_by_key(|(position, _)| *position);
            for (position, sections) in fetched {
                let (course_id, course_name) = &chunk[position];
                let sections = match sections {
                    Ok(sections) => sections,
                    // Buscando en todas las asignaturas, una sin acceso no debe impedir las demás.
                    Err(_) if params.course_id.is_none() => {
                        skipped += 1;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                for section in array(&sections)? {
                    let section_name = plain(section, "name", usize::MAX)?;
                    let section_text =
                        format!("{section_name} {}", plain(section, "summary", 4000)?);
                    if matches_all(&section_text) {
                        found.push(json!({"kind": "section", "course_id": course_id,
                            "course_name": course_name, "section": section_name,
                            "section_id": section["id"]}));
                    }
                    for module in optional_array(section, "modules")? {
                        let name = plain(module, "name", usize::MAX)?;
                        let description = plain(module, "description", 4000)?;
                        let files: Vec<_> = optional_array(module, "contents")?
                            .iter()
                            .enumerate()
                            .filter(|(_, item)| item["type"] == "file")
                            .collect();
                        let module_matches =
                            own_match(&format!("{name} {description}"), &section_name);
                        let listed: Vec<Value> = files
                            .iter()
                            .filter(|(_, item)| {
                                module_matches
                                    || own_match(
                                        &format!(
                                            "{name} {}",
                                            item["filename"].as_str().unwrap_or("")
                                        ),
                                        &section_name,
                                    )
                            })
                            .map(|(index, item)| json!({"file_index": index, "filename": item["filename"]}))
                            .collect();
                        if module_matches || !listed.is_empty() {
                            found.push(json!({"kind": "module", "course_id": course_id,
                                "course_name": course_name, "section": section_name,
                                "module_id": id(module, "id")?, "name": name, "type": module["modname"],
                                "url": module["url"], "files": listed}));
                        }
                        if found.len() >= SEARCH_LIMIT {
                            truncated = true;
                            break 'courses;
                        }
                    }
                }
            }
        }
        Ok(
            json!({"query": params.query, "results": found, "truncated": truncated,
            "courses_without_access": skipped,
            "note": "Busca en nombres y descripciones de secciones y módulos y en nombres de archivo; no dentro del contenido de los archivos. Léelos con read_course_file."}),
        )
    }

    async fn events(&self) -> Result<Value, String> {
        let client = self.client(Some("calendar"))?;
        let result = client.call(FUNCTIONS[4], &[]).await?;
        if !result.is_object() {
            return Err(FORMAT_ERROR.into());
        }
        let events = optional_array(&result, "events")?.iter().map(|event| {
            Ok(json!({"id": id(event, "id")?, "name": plain(event, "name", usize::MAX)?,
                "description": plain(event, "description", 4000)?, "starts_at_utc": utc(&event["timestart"])?,
                "url": event["url"], "course": event["course"]["fullname"], "type": event["eventtype"]}))
        }).collect::<Result<Vec<_>, String>>()?;
        Ok(json!({"events": events,
            "note": "El horizonte y el número de eventos dependen de los ajustes de Moodle; no es todo el calendario."}))
    }
}

#[tool_router]
impl MoodleServer {
    #[tool(
        description = "Comprueba la conexión, la identidad y las funciones de consulta del token.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn moodle_status(&self) -> CallToolResult {
        self.result(self.status().await)
    }

    #[tool(
        description = "Lista las asignaturas de la cuenta conectada, con identificadores y enlaces.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn list_courses(&self) -> CallToolResult {
        self.result(self.courses().await)
    }

    #[tool(
        description = "Lee temario y recursos por secciones. Continúa con next_section_start; lee los archivos con read_course_file.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_course_context(&self, Parameters(params): Parameters<Sections>) -> CallToolResult {
        self.result(self.context(params).await)
    }

    #[tool(
        description = "Consulta enunciados y plazos de tareas; no indica si están entregadas ni incluye notas.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn list_assignments(&self, Parameters(params): Parameters<Course>) -> CallToolResult {
        self.result(self.assignments(params.course_id).await)
    }

    #[tool(
        description = "Lee próximos eventos de la cuenta. Moodle determina el horizonte del calendario.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_upcoming_events(&self) -> CallToolResult {
        self.result(self.events().await)
    }

    #[tool(
        description = "Lee un PDF, Word (DOCX), PowerPoint (PPTX), Excel (XLSX), HTML o texto de un módulo del curso; también las páginas de Moodle. Usa los IDs e índices de get_course_context y pagina con next_offset.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn read_course_file(&self, Parameters(params): Parameters<ReadFile>) -> CallToolResult {
        let result = async { files::read(self.client(Some("materials"))?, params).await }.await;
        self.result(result)
    }

    #[tool(
        description = "Estado de entrega de una tarea (usa el id de list_assignments): si está entregada, calificada, nota y comentarios del profesor.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_assignment_status(
        &self,
        Parameters(params): Parameters<Assignment>,
    ) -> CallToolResult {
        self.result(self.assignment_status(params.assignment_id).await)
    }

    #[tool(
        description = "Calificaciones del alumno en un curso: cada elemento con su nota, rango, porcentaje y comentario.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_grades(&self, Parameters(params): Parameters<Course>) -> CallToolResult {
        self.result(self.grades(params.course_id).await)
    }

    #[tool(
        description = "Lista los foros de un curso. El de tipo news contiene los avisos del profesor.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn list_forums(&self, Parameters(params): Parameters<Course>) -> CallToolResult {
        self.result(self.forums(params.course_id).await)
    }

    #[tool(
        description = "Lee las discusiones de un foro, las más recientes primero, con su mensaje inicial. Pagina con next_page.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_forum_discussions(&self, Parameters(params): Parameters<Forum>) -> CallToolResult {
        self.result(self.forum_discussions(params).await)
    }

    #[tool(
        description = "Lee todos los mensajes de una discusión de foro, en orden cronológico.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_discussion_posts(
        &self,
        Parameters(params): Parameters<Discussion>,
    ) -> CallToolResult {
        self.result(self.discussion_posts(params.discussion_id).await)
    }

    #[tool(
        description = "Busca un texto en los nombres y descripciones de secciones, módulos y archivos de un curso (o de todas las asignaturas). No busca dentro de los archivos.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn search_course(&self, Parameters(params): Parameters<Search>) -> CallToolResult {
        self.result(self.search(params).await)
    }

    #[tool(
        description = "Lee a la vez hasta 6 archivos de un mismo curso (primer fragmento de cada uno). Úsala para resumir varios materiales; sigue con read_course_file y next_offset.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn read_course_files(&self, Parameters(params): Parameters<ReadFiles>) -> CallToolResult {
        let result =
            async { files::read_many(self.client(Some("materials"))?, params).await }.await;
        self.result(result)
    }

    #[tool(
        description = "Guarda un archivo del curso en la carpeta elegida por el usuario, solo cuando pida descargarlo o guardarlo. No abre archivos ni sobrescribe los existentes.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn download_course_file(
        &self,
        Parameters(params): Parameters<CourseFile>,
    ) -> CallToolResult {
        let result = async {
            let client = self.client(Some("downloads"))?;
            let settings = self
                .permissions_path
                .as_ref()
                .map_err(Clone::clone)?
                .with_file_name("downloads.json");
            let default = self.download_folder.as_ref().map_err(Clone::clone)?;
            files::download(client, params, &settings, default).await
        }
        .await;
        self.result(result)
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct Study {
    course_id: String,
    pregunta: Option<String>,
}

#[prompt_router]
impl MoodleServer {
    #[prompt(
        description = "Prepara una consulta que carga contexto y materiales de una asignatura."
    )]
    async fn estudiar_curso(&self, Parameters(params): Parameters<Study>) -> Vec<PromptMessage> {
        let pregunta = params
            .pregunta
            .as_deref()
            .unwrap_or("Ayúdame a estudiar esta asignatura");
        vec![PromptMessage::new_text(
            Role::User,
            format!(
                "{INSTRUCTIONS}\nCurso solicitado: {}.\nPetición del usuario: {pregunta}\nCarga el contexto del curso y lee los materiales relevantes. Separa lo que aparece en las fuentes de tu explicación y cita sus enlaces.",
                params.course_id
            ),
        )]
    }
}

#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = Self::prompt_router())]
impl ServerHandler for MoodleServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new(
            "mudel-ai-connect",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(INSTRUCTIONS)
    }
}

#[cfg(test)]
mod tests;
