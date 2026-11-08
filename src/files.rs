use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use rmcp::schemars;
use serde::Deserialize;
use serde_json::{Value, json};
use tempfile::NamedTempFile;
use unicode_normalization::UnicodeNormalization;

use crate::moodle::{FORMAT_ERROR, MAX_BYTES, MoodleClient, text};
use crate::office;

const MAX_DOWNLOAD_BYTES: usize = 200 * 1024 * 1024;
/// Caracteres que devuelve cada lectura, y cada archivo cuando se leen varios a la vez.
const READ_CHARS: usize = 12000;
const BATCH_CHARS: usize = 6000;
pub const MAX_BATCH: usize = 6;
pub(crate) const SAVE_ERROR: &str = "No se pudo guardar el archivo en la carpeta de descargas.";
const DOWNLOAD_ERROR: &str = "Moodle devolvió un error al descargar. Comprueba el permiso de descarga de archivos del servicio.";

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CourseFile {
    #[schemars(range(min = 1))]
    pub course_id: i64,
    #[schemars(range(min = 1))]
    pub module_id: i64,
    #[serde(default)]
    pub file_index: u64,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadFile {
    #[schemars(range(min = 1))]
    pub course_id: i64,
    #[schemars(range(min = 1))]
    pub module_id: i64,
    #[serde(default)]
    pub file_index: u64,
    #[serde(default)]
    pub offset: u64,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchFile {
    #[schemars(range(min = 1))]
    pub module_id: i64,
    #[serde(default)]
    pub file_index: u64,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadFiles {
    #[schemars(range(min = 1))]
    pub course_id: i64,
    #[schemars(length(min = 1, max = 6))]
    pub files: Vec<BatchFile>,
}

pub async fn read(client: &MoodleClient, params: ReadFile) -> Result<Value, String> {
    let (module, item) = client
        .course_file(params.course_id, params.module_id, params.file_index)
        .await?;
    read_item(client, &module, &item, params.offset, READ_CHARS).await
}

/// Lee varios archivos de un mismo curso con una sola consulta de contenidos.
/// Un archivo que falle no cancela los demás: su resultado lleva el error.
pub async fn read_many(client: &MoodleClient, params: ReadFiles) -> Result<Value, String> {
    if params.files.is_empty() || params.files.len() > MAX_BATCH {
        return Err(format!("Indica entre 1 y {MAX_BATCH} archivos."));
    }
    let sections = client.course_sections(params.course_id).await?;
    let mut results = Vec::new();
    for request in &params.files {
        let outcome =
            match MoodleClient::find_file(&sections, request.module_id, request.file_index) {
                Ok((module, item)) => read_item(client, &module, &item, 0, BATCH_CHARS).await,
                Err(error) => Err(error),
            };
        results.push(match outcome {
            Ok(mut value) => {
                value["module_id"] = json!(request.module_id);
                value["file_index"] = json!(request.file_index);
                value["ok"] = json!(true);
                value
            }
            Err(error) => json!({"module_id": request.module_id, "file_index": request.file_index,
                "ok": false, "error": error}),
        });
    }
    Ok(json!({"results": results,
        "note": "Solo se devuelve el primer fragmento de cada archivo; sigue con read_course_file y next_offset."}))
}

/// Descarga un archivo del curso (hasta 10 MiB) y rechaza las respuestas JSON de error de Moodle.
pub(crate) async fn fetch(
    client: &MoodleClient,
    item: &Value,
) -> Result<(Vec<u8>, String), String> {
    if item["filesize"]
        .as_u64()
        .is_some_and(|size| size > MAX_BYTES as u64)
    {
        return Err("El archivo supera el límite de 10 MiB.".into());
    }
    let mut body = Vec::new();
    let content_type = client
        .save(
            item["fileurl"].as_str().ok_or(FORMAT_ERROR)?,
            &mut body,
            MAX_BYTES,
        )
        .await?;
    if content_type.contains("json") {
        return Err(DOWNLOAD_ERROR.into());
    }
    Ok((body, content_type))
}

async fn read_item(
    client: &MoodleClient,
    module: &Value,
    item: &Value,
    offset: u64,
    limit: usize,
) -> Result<Value, String> {
    let name = item["filename"].as_str().ok_or(FORMAT_ERROR)?;
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let office_kind = office::kind(&extension);
    if !["pdf", "txt", "md", "csv", "html", "htm"].contains(&extension.as_str())
        && office_kind.is_none()
    {
        return Err(office::legacy_hint(&extension).map_or_else(
            || "Formato no admitido. Se pueden leer PDF con texto, DOCX, PPTX, XLSX, TXT, MD, CSV y HTML.".to_string(),
            str::to_string,
        ));
    }
    let (body, content_type) = fetch(client, item).await?;
    let mut figures = None;
    let document = if let Some(kind) = office_kind {
        if !body.starts_with(b"PK") {
            return Err(
                "La respuesta no es un documento de Office. Comprueba el acceso al archivo.".into(),
            );
        }
        tokio::task::spawn_blocking(move || office::extract(kind, &body))
            .await
            .map_err(|_| "No se pudo extraer el texto de este documento.")??
    } else if extension == "pdf" {
        if !body.starts_with(b"%PDF-") {
            return Err("La respuesta no es un PDF. Comprueba el acceso al archivo.".into());
        }
        let (text, found) = tokio::task::spawn_blocking(move || {
            let text = pdf_text(&body)?;
            // Solo en el primer fragmento: avisa de las páginas con figuras que el texto no recoge.
            let found = if offset == 0 {
                crate::figures::pdf(&body)
            } else {
                None
            };
            Ok::<_, String>((text, found))
        })
        .await
        .map_err(|_| "No se pudo extraer el texto de este PDF.")??;
        figures = found;
        text
    } else {
        let document = String::from_utf8_lossy(body.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&body))
            .into_owned();
        if ["html", "htm"].contains(&extension.as_str()) {
            text(&document)
        } else if content_type.contains("html") {
            return Err(
                "Se recibió una página HTML en lugar del archivo de texto esperado.".into(),
            );
        } else {
            document
        }
    };
    let total = document.chars().count();
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    let content: String = document.chars().skip(start).take(limit).collect();
    let next = offset
        .checked_add(limit as u64)
        .filter(|end| *end < total as u64);
    let mut result = json!({"filename": name, "source_url": item["fileurl"], "module_url": module["url"],
        "text": content, "total_characters": total, "offset": offset, "next_offset": next});
    if let Some(figures) = figures {
        result["figures"] = figures;
    }
    Ok(client.clean(result))
}

fn pdf_text(body: &[u8]) -> Result<String, String> {
    // Algunos PDF malformados provocan panic en el extractor; no deben cerrar el servidor MCP.
    std::panic::catch_unwind(|| {
        let pdf = lopdf::Document::load_mem(body).map_err(|error| match error {
            lopdf::Error::Decryption(_) => "El PDF está cifrado y no se puede leer sin su clave.",
            _ => "No se pudo extraer el texto de este PDF.",
        })?;
        if pdf.is_encrypted() || pdf.trailer.get(b"Encrypt").is_ok() {
            return Err("El PDF está cifrado y no se puede leer sin su clave.".into());
        }
        let mut document = String::new();
        pdf_extract::output_doc(&pdf, &mut pdf_extract::PlainTextOutput::new(&mut document))
            .map_err(|_| "No se pudo extraer el texto de este PDF.")?;
        if document.trim().is_empty() {
            return Err(
                "El PDF no tiene texto que se pueda extraer; puede ser un escaneo. Usa view_course_file para verlo como imagen y leerlo.".into(),
            );
        }
        Ok(document)
    })
    .map_err(|_| "No se pudo extraer el texto de este PDF.".to_string())?
}

pub fn safe_filename(name: &str, fallback: &str) -> String {
    let normalized: String = name
        .nfc()
        .map(|character| {
            if character <= '\x1f' || character == '\x7f' || "<>:\"/\\|?*".contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let trimmed = normalized.trim().trim_matches('.').trim();
    let mut name = if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    };
    let first = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&first.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            first.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
            })
        })
    {
        name.insert(0, '_');
    }
    if name.chars().count() > 120 {
        let stem = Path::new(&name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&name);
        let suffix = Path::new(&name)
            .extension()
            .and_then(|value| value.to_str())
            .map(|suffix| format!(".{suffix}"))
            .unwrap_or_default();
        let suffix: String = suffix.chars().take(16).collect();
        let stem: String = stem.chars().take(120 - suffix.chars().count()).collect();
        name = format!("{}{suffix}", stem.trim_end_matches([' ', '.']));
    }
    name
}

fn blocked(name: &str) -> bool {
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    [
        "appx",
        "bat",
        "cmd",
        "com",
        "cpl",
        "exe",
        "hta",
        "img",
        "iso",
        "jse",
        "js",
        "lnk",
        "msc",
        "msi",
        "msix",
        "pif",
        "ps1",
        "reg",
        "scf",
        "scr",
        "url",
        "vbe",
        "vbs",
        "vhd",
        "vhdx",
        "wsf",
        "wsh",
        "app",
        "command",
        "dmg",
        "pkg",
        "sh",
        "scpt",
        "applescript",
        "workflow",
        "webloc",
    ]
    .contains(&extension.as_str())
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or_else(|| "No se pudo localizar la carpeta de descargas del usuario.".into())
}

pub fn default_download_folder() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath},
        };
        let mut pointer = std::ptr::null_mut();
        // La API devuelve una cadena UTF-16 terminada en cero, que se libera con CoTaskMemFree.
        unsafe {
            if SHGetKnownFolderPath(&FOLDERID_Downloads, 0, std::ptr::null_mut(), &mut pointer) >= 0
                && !pointer.is_null()
            {
                let mut length = 0;
                while *pointer.add(length) != 0 {
                    length += 1;
                }
                let folder =
                    std::ffi::OsString::from_wide(std::slice::from_raw_parts(pointer, length));
                CoTaskMemFree(pointer.cast());
                return Ok(PathBuf::from(folder).join("Mudel AI Connect"));
            }
        }
    }
    Ok(home()?.join("Downloads/Mudel AI Connect"))
}

pub(crate) fn download_root(settings: &Path, default: &Path) -> (PathBuf, Option<String>) {
    let invalid = || {
        (default.to_owned(), Some("No se pudo leer la carpeta elegida; se ha guardado en Descargas. Vuelve a elegirla en Permisos.".into()))
    };
    let content = match fs::read(settings) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (default.to_owned(), None);
        }
        Err(_) => return invalid(),
    };
    let chosen = serde_json::from_slice::<Value>(&content)
        .ok()
        .and_then(|value| value["folder"].as_str().map(PathBuf::from));
    match chosen {
        Some(folder) if folder.is_absolute() && folder.is_dir() => (folder, None),
        Some(folder) if folder.is_absolute() => (
            default.to_owned(),
            Some(format!(
                "No se encontró la carpeta elegida ({}); se ha guardado en Descargas.",
                folder.display()
            )),
        ),
        _ => invalid(),
    }
}

fn same_contents(temporary: &NamedTempFile, existing: &Path) -> std::io::Result<bool> {
    if !fs::symlink_metadata(existing)?.file_type().is_file() {
        return Ok(false);
    }
    let mut existing = File::open(existing)?;
    let mut source = temporary.reopen()?;
    let length = source.metadata()?.len();
    if length != existing.metadata()?.len() {
        return Ok(false);
    }
    let mut left = [0; 65536];
    let mut right = [0; 65536];
    let mut remaining = length;
    while remaining > 0 {
        let size = remaining.min(left.len() as u64) as usize;
        source.read_exact(&mut left[..size])?;
        existing.read_exact(&mut right[..size])?;
        if left[..size] != right[..size] {
            return Ok(false);
        }
        remaining -= size as u64;
    }
    Ok(true)
}

pub(crate) fn place(
    mut temporary: NamedTempFile,
    wanted: &Path,
) -> Result<(PathBuf, bool), String> {
    let stem = wanted
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or(SAVE_ERROR)?;
    let suffix = wanted
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    for number in 0..1000 {
        let candidate = if number == 0 {
            wanted.to_owned()
        } else {
            wanted.with_file_name(format!("{stem} ({number}){suffix}"))
        };
        match temporary.persist_noclobber(&candidate) {
            Ok(_) => return Ok((candidate, true)),
            Err(error) => {
                if error.error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(SAVE_ERROR.into());
                }
                temporary = error.file;
                if same_contents(&temporary, &candidate).map_err(|_| SAVE_ERROR)? {
                    return Ok((candidate, false));
                }
            }
        }
    }
    Err("Hay demasiadas copias con ese nombre en la carpeta de descargas.".into())
}

pub(crate) fn mark_from_internet(path: &Path, source: &str) -> Option<String> {
    #[cfg(windows)]
    let success = {
        let mut alternate_stream = path.as_os_str().to_os_string();
        alternate_stream.push(":Zone.Identifier");
        fs::write(
            PathBuf::from(alternate_stream),
            format!("[ZoneTransfer]\nZoneId=3\nHostUrl={source}\n"),
        )
        .is_ok()
    };
    #[cfg(target_os = "macos")]
    let success = {
        let _ = source;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        std::process::Command::new("/usr/bin/xattr")
            .args([
                "-w",
                "com.apple.quarantine",
                &format!("0081;{stamp:x};Mudel AI Connect;"),
            ])
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let success = {
        let _ = (path, source);
        true
    };
    if success {
        None
    } else {
        Some(
            "No se pudo marcar el archivo como descargado de Internet en este sistema de archivos."
                .into(),
        )
    }
}

/// Nombre completo de una asignatura de la cuenta conectada.
pub(crate) async fn course_name(client: &MoodleClient, course_id: i64) -> Result<String, String> {
    let info = client.call(crate::moodle::FUNCTIONS[0], &[]).await?;
    let courses = client
        .call(
            crate::moodle::FUNCTIONS[1],
            &[("userid", crate::moodle::id(&info, "userid")?.to_string())],
        )
        .await?;
    let course = crate::moodle::array(&courses)?
        .iter()
        .find(|course| course["id"].as_i64() == Some(course_id))
        .ok_or("Ese curso no está entre las asignaturas de la cuenta conectada.")?;
    Ok(text(course["fullname"].as_str().unwrap_or("")))
}

/// Carpeta de una asignatura dentro de la de descargas: se crea y no puede salirse de ella.
pub(crate) fn course_folder(root: &Path, name: &str, course_id: i64) -> Result<PathBuf, String> {
    let folder = root.join(safe_filename(name, &format!("curso {course_id}")));
    fs::create_dir_all(&folder).map_err(|_| "No se pudo escribir en la carpeta de descargas.")?;
    if !folder
        .canonicalize()
        .map_err(|_| SAVE_ERROR)?
        .starts_with(root.canonicalize().map_err(|_| SAVE_ERROR)?)
    {
        return Err("La carpeta de la asignatura sale de la carpeta de descargas elegida.".into());
    }
    Ok(folder)
}

pub async fn download(
    client: &MoodleClient,
    params: CourseFile,
    settings: &Path,
    default: &Path,
) -> Result<Value, String> {
    if params.course_id <= 0 || params.module_id <= 0 {
        return Err("course_id y module_id deben ser positivos.".into());
    }
    let course_name = course_name(client, params.course_id).await?;
    let (module, item) = client
        .course_file(params.course_id, params.module_id, params.file_index)
        .await?;
    let name = safe_filename(item["filename"].as_str().unwrap_or(""), "archivo");
    if blocked(&name) {
        return Err("Por seguridad no se descargan programas ni scripts. Descárgalo desde Moodle en el navegador.".into());
    }
    if item["filesize"]
        .as_u64()
        .is_some_and(|size| size > MAX_DOWNLOAD_BYTES as u64)
    {
        return Err(
            "El archivo supera el límite de 200 MiB. Descárgalo desde Moodle en el navegador."
                .into(),
        );
    }
    let (root, fallback) = download_root(settings, default);
    let folder = course_folder(&root, &course_name, params.course_id)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".mudel-")
        .suffix(".part")
        .tempfile_in(&folder)
        .map_err(|_| SAVE_ERROR)?;
    let url = item["fileurl"].as_str().ok_or(FORMAT_ERROR)?;
    let content_type = client
        .save(url, temporary.as_file_mut(), MAX_DOWNLOAD_BYTES)
        .await?;
    if content_type.contains("json") {
        return Err(DOWNLOAD_ERROR.into());
    }
    temporary.flush().map_err(|_| SAVE_ERROR)?;
    let source = client
        .clean(json!(url))
        .as_str()
        .ok_or(FORMAT_ERROR)?
        .to_owned();
    let mark_warning = mark_from_internet(temporary.path(), &source);
    let (path, created) = place(temporary, &folder.join(&name))?;
    let mark_warning = if created {
        mark_warning
    } else {
        mark_from_internet(&path, &source)
    };
    let size = path.metadata().map_err(|_| SAVE_ERROR)?.len();
    let note = [
        fallback,
        mark_warning,
        Some("Guardado en este equipo; no se ha abierto ni enviado a ningún sitio.".into()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    Ok(client.clean(json!({"path": path.to_string_lossy(), "filename": path.file_name().map(|name| name.to_string_lossy()), "size_bytes": size,
        "already_downloaded": !created, "source_url": source, "module_url": module["url"], "note": note})))
}

#[cfg(test)]
mod tests;
