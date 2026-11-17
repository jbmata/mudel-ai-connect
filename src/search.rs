//! Búsqueda de fragmentos en archivos del curso, con texto reutilizable por conexión.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use rmcp::schemars;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::{
    files,
    moodle::{MoodleClient, array, id, optional_array},
};

const PAGE_FILES: usize = 8;
const RESULTS: usize = 5;
const FRAGMENT: usize = 800;
const MAX_TEXT: usize = 2 * 1024 * 1024;
const MAX_CACHE: usize = 16 * 1024 * 1024;
const INDEX_ERROR: &str = "Se interrumpió el índice de búsqueda. Abre una conexión nueva.";

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchFiles {
    #[schemars(range(min = 1))]
    pub course_id: i64,
    /// Palabras que deben aparecer en el mismo fragmento; ignora mayúsculas y tildes.
    #[schemars(length(min = 2, max = 100))]
    pub query: String,
    /// Posición de archivo; continúa con next_file_start para buscar en el resto.
    #[serde(default)]
    pub file_start: u64,
}

type Key = (i64, i64, usize);

struct Entry {
    key: Key,
    stamp: [u8; 32],
    text: Arc<str>,
}

/// Se crea con cada servidor/cuenta y solo se comparte con sus clones MCP.
// shortcut: el índice dura una conexión; persistir si las descargas entre sesiones lo justifican.
#[derive(Default)]
pub(crate) struct Index {
    entries: VecDeque<Entry>,
    bytes: usize,
}

fn stamp(item: &Value) -> Option<[u8; 32]> {
    // Sin fecha y tamaño fiables, hay que volver a descargar para comprobar cambios.
    (item["timemodified"].as_u64()? > 0).then_some(())?;
    item["filesize"].as_u64()?;
    Some(Sha256::digest(item.to_string().as_bytes()).into())
}

impl Index {
    fn get(&mut self, key: Key, item: &Value) -> Option<Arc<str>> {
        let position = self.entries.iter().position(|entry| entry.key == key)?;
        let entry = &self.entries[position];
        if Some(entry.stamp) == stamp(item) {
            return Some(entry.text.clone());
        }
        self.bytes -= self.entries.remove(position)?.text.len();
        None
    }

    fn insert(&mut self, key: Key, item: &Value, text: Arc<str>) {
        let Some(stamp) = stamp(item) else { return };
        if let Some(position) = self.entries.iter().position(|entry| entry.key == key) {
            self.bytes -= self.entries.remove(position).unwrap().text.len();
        }
        while self.bytes + text.len() > MAX_CACHE || self.entries.len() >= 128 {
            self.bytes -= self.entries.pop_front().unwrap().text.len();
        }
        self.bytes += text.len();
        self.entries.push_back(Entry { key, stamp, text });
    }
}

pub(crate) fn fold(text: &str) -> String {
    text.nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

fn fragment(document: &str, terms: &[String]) -> Option<(usize, String)> {
    let chars: Vec<_> = document.chars().collect();
    // El solapamiento conserva las coincidencias que cruzan el borde de un fragmento.
    for start in (0..chars.len()).step_by(FRAGMENT - 200) {
        let text: String = chars[start..(start + FRAGMENT).min(chars.len())]
            .iter()
            .collect();
        let folded = fold(&text);
        if terms.iter().all(|term| folded.contains(term)) {
            return Some((start, text));
        }
    }
    None
}

pub(crate) async fn search(
    client: &MoodleClient,
    index: &Mutex<Index>,
    params: SearchFiles,
) -> Result<Value, String> {
    let length = params.query.trim().chars().count();
    let mut terms: Vec<_> = fold(&params.query)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    terms.sort();
    terms.dedup();
    if params.course_id <= 0 || !(2..=100).contains(&length) || terms.is_empty() {
        return Err("Indica un course_id positivo y una búsqueda de 2 a 100 caracteres.".into());
    }
    let sections = client.course_sections(params.course_id).await?;
    let mut candidates = Vec::new();
    let mut unsupported = 0;
    for section in array(&sections)? {
        for module in optional_array(section, "modules")? {
            if module["uservisible"] == false {
                continue;
            }
            for (position, item) in optional_array(module, "contents")?.iter().enumerate() {
                if item["type"] != "file" {
                    continue;
                }
                if files::readable(item) {
                    candidates.push((module, position, item));
                } else {
                    unsupported += 1;
                }
            }
        }
    }
    let start = usize::try_from(params.file_start).map_err(|_| "file_start fuera de rango.")?;
    if start > candidates.len() {
        return Err("file_start fuera de rango.".into());
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut next = start;
    let mut results = Vec::new();
    let mut errors = Vec::new();
    let mut skipped = 0;
    for (module, position, item) in candidates.iter().skip(start).take(PAGE_FILES) {
        let module_id = id(module, "id")?;
        let key = (params.course_id, module_id, *position);
        let cached = index.lock().map_err(|_| INDEX_ERROR)?.get(key, item);
        let loaded = if let Some(text) = cached {
            Ok(text)
        } else {
            match tokio::time::timeout_at(deadline, files::full_text(client, item)).await {
                Ok(Ok(text)) if text.len() <= MAX_TEXT => {
                    let text: Arc<str> = text.into();
                    index.lock().map_err(|_| INDEX_ERROR)?.insert(key, item, text.clone());
                    Ok(text)
                }
                Ok(Ok(_)) => Err("El texto extraído supera los 2 MiB del índice; usa read_course_file.".into()),
                Ok(Err(error)) => Err(error),
                Err(_) => Err("Se agotó el tiempo de búsqueda en este archivo; puedes leerlo con read_course_file.".into()),
            }
        };
        next += 1;
        match loaded {
            Ok(text) => {
                if let Some((offset, text)) = fragment(&text, &terms) {
                    results.push(
                        json!({"course_id": params.course_id, "module_id": module_id,
                        "file_index": position, "filename": item["filename"],
                        "source_url": item["fileurl"], "module_url": module["url"],
                        "offset": offset, "text": text}),
                    );
                }
            }
            Err(error) => {
                skipped += 1;
                if errors.len() < 3 {
                    errors.push(json!({"module_id": module_id, "file_index": position,
                        "filename": item["filename"], "error": error}));
                }
            }
        }
        if results.len() == RESULTS || tokio::time::Instant::now() >= deadline {
            break;
        }
    }
    let next = (next < candidates.len()).then_some(next);
    Ok(client.clean(json!({"query": params.query, "course_id": params.course_id,
        "results": results, "next_file_start": next, "total_files": candidates.len(),
        "skipped_files": skipped, "unsupported_files": unsupported, "errors": errors,
        "note": "Hasta 5 fragmentos de 800 caracteres, uno por archivo. Continúa con next_file_start aunque no haya coincidencias. Los errores y formatos no admitidos indican búsqueda parcial. Amplía con read_course_file usando module_id, file_index y offset; para figuras usa view_course_file."})))
}

#[cfg(test)]
mod tests;
