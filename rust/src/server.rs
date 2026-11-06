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
use crate::files::{self, CourseFile, ReadFile};
use crate::moodle::{FORMAT_ERROR, FUNCTIONS, MoodleClient, array, id, optional_array, plain, utc};

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
        Self {
            client: credentials::load()
                .and_then(|account| MoodleClient::new(&account.url, &account.token)),
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
        let client = self.client(None)?;
        let info = client.call(FUNCTIONS[0], &[]).await?;
        if !info.is_object() {
            return Err(FORMAT_ERROR.into());
        }
        let mut available: Vec<_> = optional_array(&info, "functions")?
            .iter()
            .filter_map(|entry| entry["name"].as_str())
            .filter(|name| FUNCTIONS.contains(name))
            .collect();
        available.sort_unstable();
        available.dedup();
        let mut missing: Vec<_> = FUNCTIONS
            .iter()
            .filter(|name| !available.contains(name))
            .collect();
        missing.sort_unstable();
        Ok(client.clean(json!({
            "site": info["sitename"], "url": client.base_url, "user": info["fullname"],
            "user_id": info["userid"], "available_functions": available, "missing_functions": missing,
            "file_download_enabled": info["downloadfiles"]
        })))
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
                let contents = optional_array(module, "contents")?.iter().enumerate().map(|(index, item)| {
                    let mut content = serde_json::Map::new();
                    for key in ["type", "filename", "fileurl", "filesize", "mimetype"] {
                        if let Some(value) = item.get(key) { content.insert(key.into(), value.clone()); }
                    }
                    content.insert("file_index".into(), json!(index));
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
            "note": "Plazos devueltos por Moodle; no se ha consultado el estado de entrega. Verifica posibles prórrogas personales en SIFO."}),
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
        description = "Lee un PDF, HTML o texto de un módulo del curso. Usa los IDs e índices de get_course_context y pagina con next_offset.",
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
