use std::{io::Write, sync::LazyLock, time::Duration};

use regex::Regex;
use reqwest::{Client, Url, redirect::Policy};
use scraper::Html;
use serde_json::{Value, json};

pub const FUNCTIONS: [&str; 5] = [
    "core_webservice_get_site_info",
    "core_enrol_get_users_courses",
    "core_course_get_contents",
    "mod_assign_get_assignments",
    "core_calendar_get_calendar_upcoming_view",
];
pub const MAX_BYTES: usize = 10 * 1024 * 1024;
const NETWORK_ERROR: &str =
    "No se pudo consultar Moodle. Comprueba la conexión, la URL y los permisos del token.";
pub const FORMAT_ERROR: &str = "Formato de respuesta de Moodle inesperado.";

#[derive(Clone)]
pub struct MoodleClient {
    pub base_url: String,
    token: String,
    http: Client,
}

impl MoodleClient {
    pub async fn status(&self) -> Result<Value, String> {
        let info = self.call(FUNCTIONS[0], &[]).await?;
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
        Ok(self.clean(json!({
            "site": info["sitename"], "url": self.base_url, "user": info["fullname"],
            "user_id": info["userid"], "available_functions": available, "missing_functions": missing,
            "file_download_enabled": info["downloadfiles"]
        })))
    }

    pub fn new(url: &str, token: &str) -> Result<Self, String> {
        let (base_url, token) = validate_account(url, token)?;
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| NETWORK_ERROR)?;
        Ok(Self {
            base_url: base_url.into(),
            token: token.into(),
            http,
        })
    }

    pub async fn call(&self, function: &str, params: &[(&str, String)]) -> Result<Value, String> {
        if !FUNCTIONS.contains(&function) {
            return Err("Esta función no está permitida por el MCP de consulta.".into());
        }
        let mut form = params.to_vec();
        form.extend([
            ("wstoken", self.token.clone()),
            ("wsfunction", function.into()),
            ("moodlewsrestformat", "json".into()),
        ]);
        let mut body = Vec::new();
        self.stream(
            &format!("{}/webservice/rest/server.php", self.base_url),
            &form,
            &mut body,
            MAX_BYTES,
        )
        .await?;
        self.parse_response(&body, function)
    }

    async fn stream(
        &self,
        url: &str,
        form: &[(&str, String)],
        out: &mut (impl Write + Send),
        limit: usize,
    ) -> Result<String, String> {
        let mut response = self
            .http
            .post(url)
            .form(form)
            .send()
            .await
            .map_err(|_| NETWORK_ERROR)?;
        if response.status().is_redirection() {
            return Err("Moodle redirige la petición. Comprueba la URL del campus y el acceso a servicios web.".into());
        }
        if !response.status().is_success() {
            return Err(NETWORK_ERROR.into());
        }
        if response
            .content_length()
            .is_some_and(|size| size > limit as u64)
        {
            return Err(format!(
                "La respuesta supera el límite de {} MiB.",
                limit / (1024 * 1024)
            ));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut size = 0;
        while let Some(chunk) = response.chunk().await.map_err(|_| NETWORK_ERROR)? {
            if chunk.len() > limit - size {
                return Err(format!(
                    "La respuesta supera el límite de {} MiB.",
                    limit / (1024 * 1024)
                ));
            }
            out.write_all(&chunk)
                .map_err(|_| "No se pudo guardar el archivo en la carpeta de descargas.")?;
            size += chunk.len();
        }
        Ok(content_type)
    }

    pub fn file_request_url(&self, file_url: &str) -> Result<Url, String> {
        let invalid =
            "Solo se admiten archivos del endpoint webservice/pluginfile.php del mismo Moodle.";
        // Validar la ruta original antes de que Url normalice segmentos como %2e%2e y barras inversas.
        if file_url.contains('\\') || file_url.chars().any(char::is_control) {
            return Err(invalid.into());
        }
        let raw_path = file_url
            .split_once("://")
            .ok_or(invalid)?
            .1
            .split_once('/')
            .map(|(_, rest)| format!("/{}", rest.split(['?', '#']).next().unwrap_or("")))
            .ok_or(invalid)?;
        let decoded = percent_encoding::percent_decode_str(&raw_path)
            .decode_utf8()
            .map_err(|_| invalid)?;
        if decoded.contains('\\')
            || decoded.chars().any(char::is_control)
            || decoded.split('/').any(|part| matches!(part, "." | ".."))
        {
            return Err(invalid.into());
        }
        let base = Url::parse(&self.base_url).map_err(|_| invalid)?;
        let mut url = Url::parse(file_url).map_err(|_| invalid)?;
        if url.origin() != base.origin() || !url.username().is_empty() || url.password().is_some() {
            return Err("El archivo no pertenece al mismo servidor Moodle.".into());
        }
        let endpoint = format!(
            "{}/webservice/pluginfile.php",
            base.path().trim_end_matches('/')
        );
        let query: Vec<_> = url
            .query_pairs()
            .filter(|(key, _)| !["token", "wstoken"].contains(&key.to_ascii_lowercase().as_str()))
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        if !(url.path().starts_with(&(endpoint.clone() + "/"))
            || (url.path() == endpoint
                && query
                    .iter()
                    .any(|(key, value)| key == "file" && value.starts_with('/'))))
        {
            return Err(invalid.into());
        }
        url.set_fragment(None);
        url.set_query(None);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        Ok(url)
    }

    pub async fn save(
        &self,
        file_url: &str,
        out: &mut (impl Write + Send),
        limit: usize,
    ) -> Result<String, String> {
        let url = self.file_request_url(file_url)?;
        self.stream(url.as_str(), &[("token", self.token.clone())], out, limit)
            .await
    }

    pub async fn course_file(
        &self,
        course_id: i64,
        module_id: i64,
        file_index: u64,
    ) -> Result<(Value, Value), String> {
        if course_id <= 0 || module_id <= 0 {
            return Err("course_id y module_id deben ser positivos.".into());
        }
        let sections = self
            .call(FUNCTIONS[2], &[("courseid", course_id.to_string())])
            .await?;
        for section in array(&sections)? {
            for module in optional_array(section, "modules")? {
                if module["id"].as_i64() != Some(module_id) {
                    continue;
                }
                let item = usize::try_from(file_index)
                    .ok()
                    .and_then(|index| module["contents"].as_array()?.get(index))
                    .ok_or(
                        "No se encontró ese módulo o índice de archivo en el curso accesible.",
                    )?;
                if item["type"] != "file" || item["fileurl"].as_str().is_none_or(str::is_empty) {
                    return Err("Ese contenido no es un archivo descargable de Moodle.".into());
                }
                return Ok((module.clone(), item.clone()));
            }
        }
        Err("No se encontró ese módulo o índice de archivo en el curso accesible.".into())
    }

    fn parse_response(&self, body: &[u8], function: &str) -> Result<Value, String> {
        let value: Value = serde_json::from_slice(body).map_err(|_| {
            "Moodle no devolvió JSON. Puede haber un inicio de sesión institucional o servicios web desactivados."
        })?;
        if value.get("exception").is_some() || value.get("errorcode").is_some() {
            let code = value["errorcode"]
                .as_str()
                .filter(|code| {
                    !code.is_empty()
                        && code.len() <= 80
                        && code
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                        && !code.contains(&self.token)
                })
                .unwrap_or("unknown");
            return Err(format!(
                "Moodle rechazó {function} ({code}). Revisa la validez del token y las funciones autorizadas."
            ));
        }
        if !value.is_object() && !value.is_array() {
            return Err(FORMAT_ERROR.into());
        }
        Ok(value)
    }

    pub fn clean(&self, value: Value) -> Value {
        static TOKEN_QUERY: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?i)([?&](?:token|wstoken)=)[^&#\s"<>]+"#).expect("regex constante")
        });
        match value {
            Value::String(text) => Value::String(
                TOKEN_QUERY
                    .replace_all(&text.replace(&self.token, "[REDACTED]"), "${1}[REDACTED]")
                    .into_owned(),
            ),
            Value::Array(items) => {
                Value::Array(items.into_iter().map(|item| self.clean(item)).collect())
            }
            Value::Object(items) => Value::Object(
                items
                    .into_iter()
                    .filter_map(|(key, value)| {
                        if ["token", "wstoken", "privatetoken"]
                            .contains(&key.to_ascii_lowercase().as_str())
                        {
                            None
                        } else {
                            Some((key.replace(&self.token, "[REDACTED]"), self.clean(value)))
                        }
                    })
                    .collect(),
            ),
            value => value,
        }
    }
}

pub fn validate_account<'a>(url: &'a str, token: &'a str) -> Result<(&'a str, &'a str), String> {
    let url = url.trim().trim_end_matches('/');
    let parsed = Url::parse(url).map_err(|_| "MOODLE_URL debe ser una URL HTTPS del campus.")?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("MOODLE_URL debe ser HTTPS, sin credenciales ni parámetros.".into());
    }
    let token = token.trim();
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(
            "Falta MOODLE_TOKEN válido. Es una clave de servicios web, no tu contraseña.".into(),
        );
    }
    Ok((url, token))
}

pub fn text(html: &str) -> String {
    let document = Html::parse_fragment(html);
    let mut output = String::new();
    // Pila explícita: el HTML externo puede tener miles de elementos anidados.
    let mut pending = vec![(document.tree.root(), false)];
    while let Some((node, closing)) = pending.pop() {
        if let Some(value) = node.value().as_text() {
            output.push_str(value);
            continue;
        }
        let tag = node.value().as_element().map(|element| element.name());
        if matches!(tag, Some("script" | "style")) {
            continue;
        }
        let block = matches!(tag, Some("p" | "div" | "li" | "tr" | "h1" | "h2" | "h3"));
        if block || (!closing && tag == Some("br")) {
            output.push('\n');
        }
        if !closing {
            if block {
                pending.push((node, true));
            }
            pending.extend(node.children().rev().map(|child| (child, false)));
        }
    }
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn array(value: &Value) -> Result<&Vec<Value>, String> {
    value.as_array().ok_or_else(|| FORMAT_ERROR.into())
}

pub fn optional_array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], String> {
    match value.get(key) {
        None => Ok(&[]),
        Some(value) => Ok(array(value)?.as_slice()),
    }
}

pub fn id(value: &Value, key: &str) -> Result<i64, String> {
    value[key].as_i64().ok_or_else(|| FORMAT_ERROR.into())
}

pub fn plain(value: &Value, key: &str, limit: usize) -> Result<String, String> {
    let input = match value.get(key) {
        None => "",
        Some(value) => value.as_str().ok_or(FORMAT_ERROR)?,
    };
    Ok(text(input).chars().take(limit).collect())
}

pub fn utc(value: &Value) -> Result<Value, String> {
    if value.is_null() || value == 0 {
        return Ok(Value::Null);
    }
    let timestamp = value.as_i64().ok_or(FORMAT_ERROR)?;
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|date| Value::String(date.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)))
        .ok_or_else(|| FORMAT_ERROR.into())
}

#[cfg(test)]
pub(crate) mod tests;
