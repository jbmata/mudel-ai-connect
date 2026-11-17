//! Releases de GitHub y actualización del servidor instalado para la próxima sesión.

use std::{io::Write, path::Path, time::Duration};

use semver::Version;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    bundle::{self, EXE, ServerChange},
    credentials,
};

pub const RELEASES_API: &str =
    "https://api.github.com/repos/jbmata/mudel-ai-connect/releases?per_page=20";
pub const RELEASES_PAGE: &str = "https://github.com/jbmata/mudel-ai-connect/releases";
const MAX_BYTES: usize = 1_000_000;
const MAX_SERVER: usize = 200 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

fn version(text: &str) -> Option<Version> {
    Version::parse(text.strip_prefix('v').unwrap_or(text)).ok()
}

pub(crate) fn is_newer(candidate: &str, current: &str) -> bool {
    match (version(candidate), version(current)) {
        (Some(candidate), Some(current)) => candidate.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}

/// Incluye las releases 0.x marcadas como pre-release; nunca los borradores.
fn newest<'a>(releases: &'a Value, current: &str) -> Option<&'a Value> {
    releases
        .as_array()?
        .iter()
        .filter(|release| {
            let Some(tag) = release["tag_name"].as_str() else {
                return false;
            };
            !release["draft"].as_bool().unwrap_or(false)
                && tag.len() <= 80
                && release["html_url"].as_str()
                    == Some(format!("{RELEASES_PAGE}/tag/{tag}").as_str())
                && is_newer(tag, current)
        })
        .max_by(|a, b| {
            version(a["tag_name"].as_str().unwrap())
                .unwrap()
                .cmp_precedence(&version(b["tag_name"].as_str().unwrap()).unwrap())
        })
}

pub fn newer(releases: &Value, current: &str) -> Option<Release> {
    let release = newest(releases, current)?;
    Some(Release {
        version: release["tag_name"].as_str()?.into(),
        url: release["html_url"].as_str()?.into(),
    })
}

fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(5))
        .user_agent(concat!("mudel-ai-connect/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()
}

async fn body(mut response: reqwest::Response, limit: usize) -> Option<Vec<u8>> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > limit as u64)
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

async fn releases(http: &reqwest::Client) -> Option<Value> {
    let response = http
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    serde_json::from_slice(&body(response, MAX_BYTES).await?).ok()
}

/// Sin red o con una respuesta inválida, no se muestra ningún aviso.
pub async fn check(current: &str) -> Option<Release> {
    newer(&releases(&client()?).await?, current)
}

fn asset_name() -> Option<String> {
    let platform = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        return None;
    };
    Some(format!(
        "mudel-ai-connect-server-{platform}-{}{}",
        std::env::consts::ARCH,
        if cfg!(windows) { ".exe" } else { "" }
    ))
}

struct Asset<'a> {
    url: &'a str,
    digest: &'a str,
    size: u64,
}

fn asset<'a>(release: &'a Value, name: &str) -> Option<Asset<'a>> {
    let tag = release["tag_name"].as_str()?;
    version(tag)?;
    let asset = release["assets"]
        .as_array()?
        .iter()
        .find(|asset| asset["name"].as_str() == Some(name))?;
    let url = asset["browser_download_url"].as_str()?;
    let digest = asset["digest"].as_str()?.strip_prefix("sha256:")?;
    let size = asset["size"].as_u64()?;
    (url == format!("{RELEASES_PAGE}/download/{tag}/{name}")
        && asset["state"].as_str() == Some("uploaded")
        && size > 0
        && size <= MAX_SERVER as u64
        && digest.len() == 64
        && digest.bytes().all(|c| c.is_ascii_hexdigit()))
    .then_some(Asset { url, digest, size })
}

fn verified(bytes: &[u8], asset: &Asset<'_>) -> bool {
    bytes.len() as u64 == asset.size
        && format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(asset.digest)
}

/// Solo el servidor instalado: nunca un binario del repositorio o del plugin.
/// El trabajo en segundo plano no demora ni reinicia la conexión MCP actual.
pub fn start() {
    let Ok(data) = credentials::data_dir() else {
        return;
    };
    let target = data.join("bin/mudel-ai-connect-server").join(EXE);
    let Ok(installed) = target.canonicalize() else {
        return;
    };
    if std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        != Some(installed)
    {
        return;
    }
    tokio::spawn(async move {
        if let Err(error) = update(&data).await {
            eprintln!("No se pudo actualizar Mudel AI Connect: {error}");
        }
    });
}

async fn update(data: &Path) -> Result<(), String> {
    {
        let _guard = bundle::lock(data)?;
        bundle::sweep_previous(&data.join("bin"));
    }
    let Some(name) = asset_name() else {
        return Ok(());
    };
    let Some(http) = client() else { return Ok(()) };
    let Some(releases) = releases(&http).await else {
        return Ok(());
    };
    let Some(release) = newest(&releases, env!("CARGO_PKG_VERSION")) else {
        return Ok(());
    };
    let Some(asset) = asset(release, &name) else {
        return Ok(());
    };
    let Ok(response) = http.get(asset.url).send().await else {
        return Ok(());
    };
    let Some(bytes) = body(response, asset.size as usize).await else {
        return Ok(());
    };
    if !verified(&bytes, &asset) {
        return Err(
            "La descarga no coincide con su huella SHA-256; se conserva la versión anterior."
                .into(),
        );
    }
    apply(data, &bytes, release["tag_name"].as_str().unwrap()).await?;
    Ok(())
}

pub(crate) async fn binary_version(binary: &Path) -> Option<String> {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--version")
        .kill_on_drop(true)
        .env_remove("MOODLE_URL")
        .env_remove("MOODLE_TOKEN");
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).ok()?.trim();
    let text = text.strip_prefix("mudel-ai-connect-server ")?;
    version(text)?;
    Some(text.into())
}

async fn apply(data: &Path, bytes: &[u8], expected: &str) -> Result<bool, String> {
    let _guard = bundle::lock(data)?;
    let directory = data.join("bin/mudel-ai-connect-server");
    let target = directory.join(EXE);
    // Otra instancia puede haber actualizado o desinstalado mientras se descargaba.
    let Some(installed) = binary_version(&target).await else {
        return Ok(false);
    };
    if !is_newer(expected, &installed) {
        return Ok(false);
    }
    let staging = tempfile::tempdir_in(directory.parent().ok_or(bundle::IO_ERROR)?)
        .map_err(|_| bundle::IO_ERROR)?;
    let binary = staging.path().join(EXE);
    let mut file = std::fs::File::create(&binary).map_err(|_| bundle::IO_ERROR)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| bundle::IO_ERROR)?;
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| bundle::IO_ERROR)?;
    }
    let expected = expected.strip_prefix('v').unwrap_or(expected);
    if binary_version(&binary).await.as_deref() != Some(expected) {
        return Err(
            "El servidor descargado no arranca con la versión esperada; se conserva el anterior."
                .into(),
        );
    }
    if let Some(change) = ServerChange::apply(&directory, Some(bytes))? {
        if binary_version(&target).await.as_deref() != Some(expected) {
            change.rollback().map_err(|_| {
                format!(
                    "No se pudo restaurar el servidor. Copia anterior: {}",
                    change.backup.display()
                )
            })?;
            return Err("Falló la comprobación; se restauró el servidor anterior.".into());
        }
        change.finish()?;
    }
    Ok(true)
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
    fn semantic_versions_handle_prereleases_and_reject_garbage() {
        assert!(is_newer("v0.3.0-beta.2", "0.3.0-beta.1"));
        assert!(is_newer("v0.3.0", "0.3.0-beta.2"));
        assert!(!is_newer("v0.3.0-beta.2", "0.3.0"));
        assert!(!is_newer("v0.3.0+build2", "0.3.0+build1"));
        assert!(!is_newer("release9", "0.2.0"));
        let mut foreign = release("v9.0.0");
        foreign["html_url"] = json!("https://github.com/another/repo/releases/tag/v9.0.0");
        assert!(newer(&json!([foreign]), "0.2.0").is_none());
    }

    #[test]
    fn downloads_require_the_exact_asset_size_and_sha256_from_our_release() {
        let name = "mudel-ai-connect-server-windows-x86_64.exe";
        let bytes = b"server fixture";
        let mut release = release("v0.3.0");
        release["assets"] = json!([{
            "name": name, "state": "uploaded", "size": bytes.len(),
            "digest": format!("sha256:{:x}", Sha256::digest(bytes)),
            "browser_download_url": format!("{RELEASES_PAGE}/download/v0.3.0/{name}")
        }]);
        let found = asset(&release, name).unwrap();
        assert!(verified(bytes, &found));
        assert!(!verified(b"tampered server", &found));
        assert!(!verified(&bytes[..bytes.len() - 1], &found));
        assert!(asset(&release, "another-platform").is_none());
        for (field, value) in [
            ("digest", Value::Null),
            ("digest", json!("sha256:invalid")),
            ("size", json!(MAX_SERVER + 1)),
            ("size", json!(0)),
            ("state", json!("new")),
            (
                "browser_download_url",
                json!("https://evil.example/server.exe"),
            ),
            (
                "browser_download_url",
                json!(format!("{RELEASES_PAGE}/download/v0.4.0/{name}")),
            ),
        ] {
            let mut invalid = release.clone();
            invalid["assets"][0][field] = value;
            assert!(asset(&invalid, name).is_none(), "{field}");
        }
    }

    fn compile_server(root: &Path, tag: &str) -> Vec<u8> {
        let source = root.join("fixture.rs");
        std::fs::write(&source, format!(
            "fn main() {{ assert_eq!(std::env::args().nth(1).as_deref(), Some(\"--version\")); println!(\"mudel-ai-connect-server {tag}\"); }}"
        )).unwrap();
        let binary = root.join(EXE);
        let mut command = std::process::Command::new("rustc");
        // La cadena GNU local usa el enlazador que incluye Rust, sin Visual C++.
        if cfg!(all(windows, target_env = "gnu")) {
            command = std::process::Command::new("rustup");
            command.args([
                "run",
                "stable-x86_64-pc-windows-gnu",
                "rustc",
                "-C",
                "link-self-contained=yes",
                "-C",
                "linker=rust-lld",
            ]);
        }
        let output = command
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(binary).unwrap()
    }

    #[tokio::test]
    async fn update_checks_the_binary_preserves_accounts_and_never_downgrades() {
        let temporary = tempfile::tempdir().unwrap();
        let data = temporary.path().join("data");
        let directory = data.join("bin/mudel-ai-connect-server");
        std::fs::create_dir_all(&directory).unwrap();
        let old = compile_server(temporary.path(), "0.1.0");
        bundle::ServerChange::apply(&directory, Some(&old))
            .unwrap()
            .unwrap()
            .finish()
            .unwrap();
        let new = compile_server(temporary.path(), "0.3.0");
        let account = data.join("credentials.dpapi.json");
        let permissions = data.join("permissions.json");
        std::fs::write(&account, b"untouched account").unwrap();
        std::fs::write(&permissions, b"untouched permissions").unwrap();

        let guard = bundle::lock(&data).unwrap();
        assert!(apply(&data, &new, "v0.3.0").await.is_err());
        drop(guard);

        assert!(apply(&data, b"broken binary", "v0.3.0").await.is_err());
        assert_eq!(std::fs::read(directory.join(EXE)).unwrap(), old);
        assert!(apply(&data, &new, "v0.4.0").await.is_err());
        assert_eq!(
            binary_version(&directory.join(EXE)).await.as_deref(),
            Some("0.1.0")
        );
        assert!(apply(&data, &new, "v0.3.0").await.unwrap());
        assert_eq!(
            binary_version(&directory.join(EXE)).await.as_deref(),
            Some("0.3.0")
        );
        assert!(!apply(&data, &old, "v0.1.0").await.unwrap());
        assert!(!apply(&data, &new, "v0.3.0").await.unwrap());
        assert_eq!(std::fs::read(&account).unwrap(), b"untouched account");
        assert_eq!(
            std::fs::read(&permissions).unwrap(),
            b"untouched permissions"
        );
        std::fs::remove_file(directory.join(EXE)).unwrap();
        assert!(!apply(&data, &new, "v0.3.0").await.unwrap());
        assert!(!directory.join(EXE).exists());
    }

    #[tokio::test]
    async fn http_errors_and_oversized_responses_are_rejected() {
        use std::{io::Read, net::TcpListener};
        for response in [
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n0123456789",
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n0123456789",
            "HTTP/1.1 500 Error\r\nContent-Length: 0\r\n\r\n",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                assert!(stream.read(&mut request).unwrap() > 0);
                stream.write_all(response.as_bytes()).unwrap();
            });
            let response = reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .get(format!("http://{address}"))
                .timeout(Duration::from_secs(5))
                .send()
                .await
                .unwrap();
            assert!(body(response, 4).await.is_none());
            server.join().unwrap();
        }
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
