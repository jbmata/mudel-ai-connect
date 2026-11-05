use std::{sync::LazyLock, time::Duration};

use regex::Regex;
use reqwest::{Client, Url, redirect::Policy};
use scraper::Html;
use serde_json::Value;

pub const FUNCTIONS: [&str; 5] = [
    "core_webservice_get_site_info",
    "core_enrol_get_users_courses",
    "core_course_get_contents",
    "mod_assign_get_assignments",
    "core_calendar_get_calendar_upcoming_view",
];
const MAX_BYTES: usize = 10 * 1024 * 1024;
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
    pub fn new(url: &str, token: &str) -> Result<Self, String> {
        let base_url = url.trim().trim_end_matches('/');
        let parsed =
            Url::parse(base_url).map_err(|_| "MOODLE_URL debe ser una URL HTTPS del campus.")?;
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
                "Falta MOODLE_TOKEN válido. Es una clave de servicios web, no tu contraseña."
                    .into(),
            );
        }
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
        let mut response = self
            .http
            .post(format!("{}/webservice/rest/server.php", self.base_url))
            .form(&form)
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
            .is_some_and(|size| size > MAX_BYTES as u64)
        {
            return Err("La respuesta supera el límite de 10 MiB.".into());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| NETWORK_ERROR)? {
            if chunk.len() > MAX_BYTES - body.len() {
                return Err("La respuesta supera el límite de 10 MiB.".into());
            }
            body.extend_from_slice(&chunk);
        }
        self.parse_response(&body, function)
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
