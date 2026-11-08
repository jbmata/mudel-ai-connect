//! Extracción de texto de documentos modernos de Office (DOCX, PPTX y XLSX).
//!
//! Son ZIP con XML. Se leen sin ejecutar nada y con límites estrictos de tamaño para que
//! un archivo malicioso («bomba ZIP») no agote la memoria.

use std::io::{Cursor, Read};

use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};
use zip::{ZipArchive, result::ZipError};

const INVALID: &str =
    "No es un documento de Office válido, o está protegido con contraseña y no se puede leer.";
const TOO_BIG: &str = "El documento ocupa demasiado una vez descomprimido para leerlo.";
const MAX_ENTRY: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: u64 = 160 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_CHARS: usize = 3_000_000;
const MAX_ROWS: usize = 50_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Docx,
    Pptx,
    Xlsx,
}

/// Formato moderno que se sabe leer, por extensión.
pub fn kind(extension: &str) -> Option<Kind> {
    match extension {
        "docx" => Some(Kind::Docx),
        "pptx" => Some(Kind::Pptx),
        "xlsx" => Some(Kind::Xlsx),
        _ => None,
    }
}

/// Aviso para los formatos binarios antiguos, que no se pueden leer.
pub fn legacy_hint(extension: &str) -> Option<&'static str> {
    match extension {
        "doc" => Some("Es un documento .doc antiguo; guárdalo como .docx para poder leerlo."),
        "ppt" => Some("Es una presentación .ppt antigua; guárdala como .pptx para poder leerla."),
        "xls" => Some("Es una hoja .xls antigua; guárdala como .xlsx para poder leerla."),
        _ => None,
    }
}

pub fn extract(kind: Kind, body: &[u8]) -> Result<String, String> {
    // Un documento malformado no debe cerrar el servidor MCP.
    let result = std::panic::catch_unwind(|| {
        let mut pack = Pack::open(body)?;
        match kind {
            Kind::Docx => docx(&mut pack),
            Kind::Pptx => pptx(&mut pack),
            Kind::Xlsx => xlsx(&mut pack),
        }
    })
    .map_err(|_| String::from(INVALID))??;
    let text = tidy(&result);
    if text.is_empty() {
        return Err("El documento no contiene texto que se pueda leer.".into());
    }
    Ok(text)
}

struct Pack<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    remaining: u64,
}

impl<'a> Pack<'a> {
    fn open(body: &'a [u8]) -> Result<Self, String> {
        let archive = ZipArchive::new(Cursor::new(body)).map_err(|_| INVALID)?;
        if archive.len() > MAX_ENTRIES {
            return Err(TOO_BIG.into());
        }
        Ok(Self {
            archive,
            remaining: MAX_TOTAL,
        })
    }

    fn names(&self) -> Vec<String> {
        self.archive
            .file_names()
            .flatten()
            .map(|name| name.into_owned())
            .collect()
    }

    fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let file = match self.archive.by_name(name) {
            Ok(file) => file,
            Err(ZipError::FileNotFound) => return Ok(None),
            Err(_) => return Err(INVALID.into()),
        };
        let limit = MAX_ENTRY.min(self.remaining);
        if file.size() > limit {
            return Err(TOO_BIG.into());
        }
        let mut bytes = Vec::new();
        // La cabecera puede mentir sobre el tamaño: se cuenta lo que se descomprime de verdad.
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| INVALID)?;
        if bytes.len() as u64 > limit {
            return Err(TOO_BIG.into());
        }
        self.remaining -= bytes.len() as u64;
        Ok(Some(bytes))
    }
}

enum Ev<'a> {
    Start(&'a BytesStart<'a>),
    Empty(&'a BytesStart<'a>),
    End(&'a str),
    Text(&'a str),
}

fn local<'a>(element: &'a BytesStart<'_>) -> &'a str {
    element.local_name().into_inner()
}

/// Valor de un atributo por su nombre sin prefijo (`r:id` → `id`).
fn attribute(element: &BytesStart<'_>, wanted: &str) -> Option<String> {
    element
        .attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| attribute.key.local_name().into_inner() == wanted)
        .and_then(|attribute| {
            attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

/// Recorre un XML sin cargarlo entero. `visit` devuelve `false` para parar.
fn walk(xml: &[u8], mut visit: impl FnMut(Ev<'_>) -> bool) -> Result<(), String> {
    let mut reader = Reader::from_reader(xml);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let keep_going = match reader.read_event_into(&mut buffer).map_err(|_| INVALID)? {
            Event::Eof => break,
            Event::Start(element) => visit(Ev::Start(&element)),
            Event::Empty(element) => visit(Ev::Empty(&element)),
            Event::End(element) => visit(Ev::End(element.local_name().into_inner())),
            Event::Text(text) => visit(Ev::Text(&text)),
            Event::CData(data) => visit(Ev::Text(&data)),
            Event::GeneralRef(reference) => {
                let resolved = match &*reference {
                    "amp" => "&".to_string(),
                    "lt" => "<".to_string(),
                    "gt" => ">".to_string(),
                    "quot" => "\"".to_string(),
                    "apos" => "'".to_string(),
                    _ => reference
                        .resolve_char_ref()
                        .ok()
                        .flatten()
                        .map(String::from)
                        .unwrap_or_default(),
                };
                visit(Ev::Text(&resolved))
            }
            _ => true,
        };
        if !keep_going {
            break;
        }
    }
    Ok(())
}

/// Quita espacios sobrantes y colapsa las líneas en blanco repetidas.
fn tidy(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            if lines.last().is_some_and(|last| !last.is_empty()) {
                lines.push("");
            }
        } else {
            lines.push(line);
        }
    }
    lines.join("\n").trim().to_string()
}

/// Texto de un cuerpo WordprocessingML o DrawingML: párrafos (`p`), runs (`r`), texto (`t`)
/// y tablas (`tbl`, `tr`, `tc`). Es el mismo vocabulario en DOCX y en PPTX.
fn rich_text(xml: &[u8], out: &mut String) -> Result<(), String> {
    let (mut in_text, mut in_run, mut tables) = (false, false, 0usize);
    walk(xml, |event| {
        match event {
            Ev::Start(element) => match local(element) {
                "t" => in_text = true,
                "r" => in_run = true,
                "tbl" => tables += 1,
                "tab" if in_run => out.push('\t'),
                "br" | "cr" => out.push('\n'),
                _ => (),
            },
            Ev::Empty(element) => match local(element) {
                "tab" if in_run => out.push('\t'),
                "br" | "cr" => out.push('\n'),
                _ => (),
            },
            Ev::End(name) => match name {
                "t" => in_text = false,
                "r" => in_run = false,
                "p" => out.push(if tables > 0 { ' ' } else { '\n' }),
                "tc" => {
                    out.truncate(out.trim_end_matches(' ').len());
                    out.push_str(" | ");
                }
                "tr" => {
                    if out.ends_with(" | ") {
                        out.truncate(out.len() - 3);
                    }
                    out.push('\n');
                }
                "tbl" => tables = tables.saturating_sub(1),
                _ => (),
            },
            Ev::Text(text) => {
                if in_text {
                    out.push_str(text);
                }
            }
        }
        out.len() < MAX_CHARS
    })
}

fn docx(pack: &mut Pack) -> Result<String, String> {
    let xml = pack.read("word/document.xml")?.ok_or(INVALID)?;
    let mut out = String::new();
    rich_text(&xml, &mut out)?;
    if let Some(marker) = figure_marker(&xml) {
        out.push_str(&format!(
            "
{marker}
"
        ));
    }
    Ok(out)
}

/// Aviso de que un trozo del documento tiene imágenes o dibujos que el texto no recoge.
fn figure_marker(xml: &[u8]) -> Option<String> {
    let (ids, unrendered) = embedded_in(xml).ok()?;
    let mut parts = Vec::new();
    if !ids.is_empty() {
        let noun = if ids.len() == 1 {
            "imagen"
        } else {
            "imágenes"
        };
        parts.push(format!("{} {noun}", ids.len()));
    }
    if unrendered > 0 {
        parts.push("dibujos o gráficos".to_string());
    }
    (!parts.is_empty()).then(|| {
        format!(
            "[figuras: {}; se ven con view_course_file]",
            parts.join(" y ")
        )
    })
}

/// Resuelve `target` relativo al directorio de `from` (`ppt/slides` + `../notesSlides/x.xml`).
fn resolve(from_directory: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        from_directory
            .split('/')
            .filter(|p| !p.is_empty())
            .collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => (),
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Pares (Id, Target, Type) de un archivo de relaciones.
fn relationships(xml: &[u8]) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    let _ = walk(xml, |event| {
        if let Ev::Start(element) | Ev::Empty(element) = event
            && local(element) == "Relationship"
        {
            found.push((
                attribute(element, "Id").unwrap_or_default(),
                attribute(element, "Target").unwrap_or_default(),
                attribute(element, "Type").unwrap_or_default(),
            ));
        }
        true
    });
    found
}

fn number_in(name: &str, prefix: &str) -> Option<u64> {
    name.strip_prefix(prefix)?
        .strip_suffix(".xml")?
        .parse()
        .ok()
}

fn pptx(pack: &mut Pack) -> Result<String, String> {
    let mut slides: Vec<(u64, String)> = pack
        .names()
        .into_iter()
        .filter_map(|name| number_in(&name, "ppt/slides/slide").map(|n| (n, name)))
        .collect();
    if slides.is_empty() {
        return Err(INVALID.into());
    }
    slides.sort();
    let mut out = String::new();
    for (number, name) in slides {
        out.push_str(&format!("\n--- Diapositiva {number} ---\n"));
        let mut text = String::new();
        let mut marker = None;
        if let Some(xml) = pack.read(&name)? {
            rich_text(&xml, &mut text)?;
            marker = figure_marker(&xml);
        }
        if tidy(&text).is_empty() {
            out.push_str("(sin texto)\n");
        } else {
            out.push_str(&text);
        }
        if let Some(marker) = marker {
            out.push_str(&format!("{marker}\n"));
        }
        let rels = pack.read(&format!("ppt/slides/_rels/slide{number}.xml.rels"))?;
        let notes = rels.map(|bytes| {
            relationships(&bytes)
                .into_iter()
                .find(|(_, _, kind)| kind.ends_with("/notesSlide"))
                .map(|(_, target, _)| resolve("ppt/slides", &target))
        });
        if let Some(Some(path)) = notes
            && let Some(xml) = pack.read(&path)?
        {
            let mut notes = String::new();
            rich_text(&xml, &mut notes)?;
            // Los números de diapositiva de las notas no aportan nada.
            let notes: Vec<&str> = notes
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.chars().all(|c| c.is_ascii_digit()))
                .collect();
            if !notes.is_empty() {
                out.push_str("\nNotas del ponente:\n");
                out.push_str(&notes.join("\n"));
                out.push('\n');
            }
        }
        if out.len() >= MAX_CHARS {
            break;
        }
    }
    Ok(out)
}

fn shared_strings(xml: &[u8]) -> Result<Vec<String>, String> {
    let mut strings = Vec::new();
    let (mut current, mut in_text, mut phonetic) = (String::new(), false, false);
    walk(xml, |event| {
        match event {
            Ev::Start(element) => match local(element) {
                "si" => current.clear(),
                "t" => in_text = true,
                "rPh" => phonetic = true,
                _ => (),
            },
            Ev::End(name) => match name {
                "t" => in_text = false,
                "rPh" => phonetic = false,
                "si" => strings.push(std::mem::take(&mut current)),
                _ => (),
            },
            Ev::Text(text) => {
                if in_text && !phonetic {
                    current.push_str(text);
                }
            }
            Ev::Empty(_) => (),
        }
        strings.len() < 2_000_000
    })?;
    Ok(strings)
}

/// Índice de columna (0 = A) de una referencia como `AB12`.
fn column(reference: &str) -> usize {
    reference
        .bytes()
        .take_while(u8::is_ascii_alphabetic)
        .fold(0, |total, byte| {
            total * 26 + usize::from(byte.to_ascii_uppercase() - b'A' + 1)
        })
        .saturating_sub(1)
}

fn sheet_text(xml: &[u8], strings: &[String], out: &mut String) -> Result<(), String> {
    let mut row: Vec<(usize, String)> = Vec::new();
    let mut cell = (0usize, String::new(), String::new()); // columna, tipo, texto
    let (mut in_value, mut in_inline, mut in_cell) = (false, false, false);
    let mut rows = 0usize;
    walk(xml, |event| {
        match event {
            Ev::Start(element) => match local(element) {
                "row" => row.clear(),
                "c" => {
                    in_cell = true;
                    cell = (
                        attribute(element, "r").map_or(row.len(), |r| column(&r)),
                        attribute(element, "t").unwrap_or_default(),
                        String::new(),
                    );
                }
                "v" if in_cell => in_value = true,
                "t" if in_cell => in_inline = true,
                _ => (),
            },
            Ev::End(name) => match name {
                "v" => in_value = false,
                "t" => in_inline = false,
                "c" => {
                    in_cell = false;
                    let value = match cell.1.as_str() {
                        "s" => cell
                            .2
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| strings.get(index).cloned())
                            .unwrap_or_default(),
                        "b" => if cell.2.trim() == "1" {
                            "VERDADERO"
                        } else {
                            "FALSO"
                        }
                        .into(),
                        _ => std::mem::take(&mut cell.2),
                    };
                    if !value.trim().is_empty() {
                        row.push((cell.0, value.replace(['\t', '\n', '\r'], " ")));
                    }
                }
                "row" => {
                    if !row.is_empty() {
                        let mut position = 0;
                        for (index, value) in row.drain(..) {
                            while position < index {
                                out.push('\t');
                                position += 1;
                            }
                            out.push_str(&value);
                            out.push('\t');
                            position += 1;
                        }
                        out.truncate(out.trim_end_matches('\t').len());
                        out.push('\n');
                    }
                    rows += 1;
                }
                _ => (),
            },
            Ev::Text(text) => {
                if in_value || in_inline {
                    cell.2.push_str(text);
                }
            }
            Ev::Empty(_) => (),
        }
        rows < MAX_ROWS && out.len() < MAX_CHARS
    })
}

fn xlsx(pack: &mut Pack) -> Result<String, String> {
    let strings = match pack.read("xl/sharedStrings.xml")? {
        Some(xml) => shared_strings(&xml)?,
        None => Vec::new(),
    };
    let workbook = pack.read("xl/workbook.xml")?.ok_or(INVALID)?;
    let rels = pack
        .read("xl/_rels/workbook.xml.rels")?
        .map(|xml| relationships(&xml))
        .unwrap_or_default();
    let mut sheets: Vec<(String, String)> = Vec::new();
    walk(&workbook, |event| {
        if let Ev::Start(element) | Ev::Empty(element) = event
            && local(element) == "sheet"
            && attribute(element, "state").is_none_or(|state| state == "visible")
        {
            let target = attribute(element, "id")
                .and_then(|id| rels.iter().find(|(rid, _, _)| *rid == id))
                .map(|(_, target, _)| resolve("xl", target));
            if let Some(target) = target {
                sheets.push((attribute(element, "name").unwrap_or_default(), target));
            }
        }
        true
    })?;
    if sheets.is_empty() {
        return Err(INVALID.into());
    }
    let mut out = String::new();
    for (name, path) in sheets {
        let Some(xml) = pack.read(&path)? else {
            continue;
        };
        out.push_str(&format!("\n## Hoja: {name}\n"));
        sheet_text(&xml, &strings, &mut out)?;
        if out.len() >= MAX_CHARS {
            break;
        }
    }
    let names = pack.names();
    let images = names
        .iter()
        .filter(|name| name.starts_with("xl/media/"))
        .count();
    let charts = names
        .iter()
        .filter(|name| name.starts_with("xl/charts/chart"))
        .count();
    if images + charts > 0 {
        let mut parts = Vec::new();
        if images > 0 {
            parts.push(format!("{images} imágenes"));
        }
        if charts > 0 {
            parts.push(format!("{charts} gráficos"));
        }
        out.push_str(&format!(
            "
[figuras: {}; las imágenes se ven con view_course_file]
",
            parts.join(" y ")
        ));
    }
    Ok(out)
}

/// Una imagen incrustada en un documento de Office.
pub struct Picture {
    pub label: String,
    pub extension: String,
    pub data: Vec<u8>,
}

/// Lo que se puede mostrar de un documento de Office: sus imágenes incrustadas.
#[derive(Default)]
pub struct Pictures {
    /// Diapositivas (PPTX) o imágenes (DOCX y XLSX) que tiene el documento.
    pub total: usize,
    pub found: Vec<Picture>,
    /// Imágenes en formatos que no se pueden mostrar (EMF, WMF, SVG…).
    pub unsupported: usize,
    /// Gráficos y dibujos hechos con formas de Office, que no se pueden mostrar.
    pub unrendered: usize,
}

const SHOWABLE: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

fn extension_of(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Identificadores de relación de las imágenes que cita un XML, en orden de aparición, y
/// cuántos gráficos y dibujos con formas contiene (que no se pueden mostrar).
fn embedded_in(xml: &[u8]) -> Result<(Vec<String>, usize), String> {
    let mut ids: Vec<String> = Vec::new();
    let mut unrendered = 0;
    walk(xml, |event| {
        if let Ev::Start(element) | Ev::Empty(element) = event {
            match local(element) {
                "blip" => {
                    if let Some(id) = attribute(element, "embed")
                        && !ids.contains(&id)
                    {
                        ids.push(id);
                    }
                }
                "chart" | "cxnSp" | "custGeom" | "relIds" | "wsp" | "wpg" => unrendered += 1,
                _ => (),
            }
        }
        true
    })?;
    Ok((ids, unrendered))
}

/// Imágenes incrustadas de las partes pedidas: diapositivas en un PPTX, números de imagen
/// (en orden de aparición) en un DOCX o un XLSX. La numeración empieza en 1.
pub fn pictures(kind: Kind, body: &[u8], wanted: &[usize]) -> Result<Pictures, String> {
    std::panic::catch_unwind(|| {
        let mut pack = Pack::open(body)?;
        match kind {
            Kind::Pptx => slide_pictures(&mut pack, wanted),
            Kind::Docx => document_pictures(&mut pack, wanted),
            Kind::Xlsx => sheet_pictures(&mut pack, wanted),
        }
    })
    .map_err(|_| String::from(INVALID))?
}

/// Lee una imagen del paquete: `None` si su formato no se puede mostrar.
fn picture(pack: &mut Pack, path: &str, label: String) -> Result<Option<Picture>, String> {
    let extension = extension_of(path);
    if !SHOWABLE.contains(&extension.as_str()) {
        return Ok(None);
    }
    Ok(pack.read(path)?.map(|data| Picture {
        label,
        extension,
        data,
    }))
}

fn slide_pictures(pack: &mut Pack, wanted: &[usize]) -> Result<Pictures, String> {
    let mut slides: Vec<(u64, String)> = pack
        .names()
        .into_iter()
        .filter_map(|name| number_in(&name, "ppt/slides/slide").map(|n| (n, name)))
        .collect();
    if slides.is_empty() {
        return Err(INVALID.into());
    }
    slides.sort();
    let mut result = Pictures {
        total: slides.len(),
        ..Pictures::default()
    };
    for &number in wanted {
        let Some((_, name)) = slides.iter().find(|(n, _)| *n == number as u64) else {
            return Err(format!(
                "La diapositiva {number} no existe. Las diapositivas son {}.",
                slides
                    .iter()
                    .map(|(n, _)| n.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        };
        let xml = pack.read(name)?.ok_or(INVALID)?;
        let (ids, unrendered) = embedded_in(&xml)?;
        result.unrendered += unrendered;
        let rels = pack
            .read(&format!("ppt/slides/_rels/slide{number}.xml.rels"))?
            .map(|bytes| relationships(&bytes))
            .unwrap_or_default();
        let mut index = 0;
        for id in ids {
            let Some((_, target, _)) = rels
                .iter()
                .find(|(rid, _, kind)| *rid == id && kind.ends_with("/image"))
            else {
                continue;
            };
            index += 1;
            let path = resolve("ppt/slides", target);
            let label = format!("Diapositiva {number}, imagen {index}");
            match picture(pack, &path, label)? {
                Some(found) => result.found.push(found),
                None => result.unsupported += 1,
            }
        }
    }
    Ok(result)
}

fn document_pictures(pack: &mut Pack, wanted: &[usize]) -> Result<Pictures, String> {
    let xml = pack.read("word/document.xml")?.ok_or(INVALID)?;
    let (ids, unrendered) = embedded_in(&xml)?;
    let rels = pack
        .read("word/_rels/document.xml.rels")?
        .map(|bytes| relationships(&bytes))
        .unwrap_or_default();
    let paths: Vec<String> = ids
        .iter()
        .filter_map(|id| {
            rels.iter()
                .find(|(rid, _, kind)| rid == id && kind.ends_with("/image"))
                .map(|(_, target, _)| resolve("word", target))
        })
        .collect();
    numbered_pictures(pack, &paths, wanted, unrendered)
}

fn sheet_pictures(pack: &mut Pack, wanted: &[usize]) -> Result<Pictures, String> {
    let mut paths: Vec<String> = pack
        .names()
        .into_iter()
        .filter(|name| name.starts_with("xl/media/"))
        .collect();
    paths.sort();
    let charts = pack
        .names()
        .iter()
        .filter(|name| name.starts_with("xl/charts/chart"))
        .count();
    numbered_pictures(pack, &paths, wanted, charts)
}

fn numbered_pictures(
    pack: &mut Pack,
    paths: &[String],
    wanted: &[usize],
    unrendered: usize,
) -> Result<Pictures, String> {
    let mut result = Pictures {
        total: paths.len(),
        unrendered,
        ..Pictures::default()
    };
    for &number in wanted {
        let Some(path) = number.checked_sub(1).and_then(|index| paths.get(index)) else {
            return Err(format!(
                "La imagen {number} no existe. El documento tiene {} imágenes.",
                paths.len()
            ));
        };
        match picture(pack, path, format!("Imagen {number}"))? {
            Some(found) => result.found.push(found),
            None => result.unsupported += 1,
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
