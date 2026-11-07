//! Aviso de versiones nuevas: consulta la lista pública de releases de GitHub.
//!
//! Solo lee; nunca descarga ni ejecuta nada. Si no hay red o la respuesta no es la
//! esperada, no se avisa de nada.

use std::time::Duration;

use serde_json::Value;

pub const RELEASES_API: &str =
    "https://api.github.com/repos/jbmata/mudel-ai-connect/releases?per_page=20";
pub const RELEASES_PAGE: &str = "https://github.com/jbmata/mudel-ai-connect/releases";
const MAX_BYTES: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

/// Hasta tres números de una etiqueta como `v0.3.0-beta.1`; las partes que
/// faltan valen 0. `None` si la etiqueta no contiene ningún número.
fn version_key(text: &str) -> Option<[u64; 3]> {
    let mut key = [0; 3];
    let mut found = 0;
    for digits in text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .take(3)
    {
        key[found] = digits.parse().ok()?;
        found += 1;
    }
    (found > 0).then_some(key)
}

/// La release publicada más reciente si es más nueva que `current`.
///
/// Usa la lista y no `/releases/latest`, que omite las betas marcadas como pre-release.
pub fn newer(releases: &Value, current: &str) -> Option<Release> {
    let current = version_key(current)?;
    let mut best: Option<([u64; 3], Release)> = None;
    for release in releases.as_array()? {
        let (Some(tag), Some(url)) = (release["tag_name"].as_str(), release["html_url"].as_str())
        else {
            continue;
        };
        if release["draft"].as_bool().unwrap_or(false)
            || !url.starts_with("https://github.com/")
            || tag.chars().count() > 40
        {
            continue;
        }
        let Some(key) = version_key(tag) else {
            continue;
        };
        if best.as_ref().is_none_or(|(top, _)| key > *top) {
            best = Some((
                key,
                Release {
                    version: tag.to_string(),
                    url: url.to_string(),
                },
            ));
        }
    }
    best.filter(|(key, _)| *key > current)
        .map(|(_, release)| release)
}

/// Consulta GitHub. Cualquier fallo (red, límite de peticiones, formato) devuelve `None`.
pub async fn check(current: &str) -> Option<Release> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent(format!("mudel-ai-connect/{current}"))
        .build()
        .ok()?;
    let mut response = http
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BYTES {
            return None;
        }
    }
    newer(&serde_json::from_slice(&body).ok()?, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(tag: &str) -> Value {
        json!({"tag_name": tag, "html_url": format!("https://github.com/jbmata/mudel-ai-connect/releases/tag/{tag}")})
    }

    #[test]
    fn picks_the_highest_release_including_betas_and_compares_numerically() {
        let releases = json!([
            release("v0.9.0-beta"),
            release("v0.10.1"),
            release("v0.2.0")
        ]);
        let found = newer(&releases, "0.2.0").unwrap();
        assert_eq!(found.version, "v0.10.1");
        assert!(found.url.ends_with("/tag/v0.10.1"));
    }

    #[test]
    fn nothing_is_reported_for_equal_or_older_versions() {
        let releases = json!([release("v0.2.0"), release("v0.1.0")]);
        assert_eq!(newer(&releases, "0.2.0"), None);
        assert_eq!(newer(&releases, "0.2.1"), None);
        assert_eq!(newer(&json!([]), "0.2.0"), None);
    }

    #[test]
    fn drafts_foreign_urls_and_malformed_entries_are_ignored() {
        let mut draft = release("v9.0.0");
        draft["draft"] = json!(true);
        let mut foreign = release("v8.0.0");
        foreign["html_url"] = json!("https://evil.example/v8.0.0");
        let releases = json!([
            draft,
            foreign,
            {"tag_name": "latest", "html_url": "https://github.com/x/y"},
            {"tag_name": 3},
            "texto",
            release("v0.3.0"),
        ]);
        assert_eq!(newer(&releases, "0.2.0").unwrap().version, "v0.3.0");
        assert_eq!(newer(&json!({"message": "rate limited"}), "0.2.0"), None);
    }
}
