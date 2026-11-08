//! Ver figuras de los materiales: páginas de PDF, imágenes incrustadas en PPTX, DOCX y XLSX, e
//! imágenes sueltas. Se devuelven como imágenes para que el modelo pueda leer circuitos,
//! gráficas y esquemas que no están en el texto.
//!
//! Las páginas de PDF se dibujan con `hayro` (Rust puro, sin ejecutar nada del documento).
//! Las imágenes incrustadas se reducen y se recodifican, con límites de tamaño.

use std::{
    io::{Cursor, Write},
    path::Path,
    sync::Arc,
    time::Duration,
};

use hayro::{
    PixmapSettings, RenderCache, RenderSettings,
    hayro_interpret::InterpreterSettings,
    hayro_syntax::{LoadPdfError, Pdf},
    render,
    vello_cpu::color::palette::css::WHITE,
};
use image::{
    DynamicImage, GenericImageView, ImageFormat, ImageReader, Limits, RgbImage, RgbaImage,
    codecs::jpeg::JpegEncoder, imageops::FilterType,
};
use rmcp::schemars;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    files::{self, fetch},
    moodle::{FORMAT_ERROR, MoodleClient},
    office::{self, Kind},
};

/// Páginas, diapositivas o imágenes que se pueden pedir en una sola consulta.
pub const MAX_PARTS: usize = 4;
/// Lado mayor, en píxeles, de cada imagen devuelta.
const MAX_SIDE: u32 = 1400;
/// Lado menor a partir del cual una imagen incrustada se considera un adorno (logos, viñetas).
const MIN_SIDE: u32 = 48;
/// Tamaño máximo de cada imagen y de todas las de una respuesta.
const MAX_IMAGE_BYTES: usize = 1_200_000;
const MAX_TOTAL_BYTES: usize = 4_000_000;
/// Lado máximo que se acepta al decodificar, para no agotar la memoria con una imagen enorme.
const MAX_DECODE_SIDE: u32 = 12_000;
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);
const UNSUPPORTED: &str =
    "Solo se pueden ver PDF, PPTX, DOCX, XLSX e imágenes (PNG, JPG, GIF y WEBP).";

fn first_page() -> Vec<u32> {
    vec![1]
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewFile {
    #[schemars(range(min = 1))]
    pub course_id: i64,
    #[schemars(range(min = 1))]
    pub module_id: i64,
    #[serde(default)]
    pub file_index: u64,
    /// Páginas de un PDF, diapositivas de un PPTX o números de imagen de un DOCX o XLSX, desde 1.
    /// Hasta 4 por consulta. Por defecto, la primera.
    #[serde(default = "first_page")]
    #[schemars(length(min = 1, max = 4))]
    pub pages: Vec<u32>,
}

/// Una imagen lista para enviar.
pub struct Shown {
    pub label: String,
    pub mime: &'static str,
    pub data: Vec<u8>,
}

/// Resultado de ver un archivo: un resumen en JSON y las imágenes, en el mismo orden que `shown`.
pub struct Views {
    pub info: Value,
    pub images: Vec<Shown>,
}

enum Source {
    Pdf,
    Office(Kind),
    Image,
}

/// Páginas pedidas, sin repetir y en el orden indicado.
fn parts(pages: &[u32]) -> Result<Vec<usize>, String> {
    if pages.is_empty() || pages.len() > MAX_PARTS {
        return Err(format!("Indica entre 1 y {MAX_PARTS} páginas."));
    }
    let mut parts: Vec<usize> = Vec::new();
    for &page in pages {
        if page == 0 {
            return Err("La numeración empieza en 1.".into());
        }
        let page = page as usize;
        if !parts.contains(&page) {
            parts.push(page);
        }
    }
    Ok(parts)
}

pub async fn view(client: &MoodleClient, params: ViewFile) -> Result<Views, String> {
    let wanted = parts(&params.pages)?;
    let (module, item) = client
        .course_file(params.course_id, params.module_id, params.file_index)
        .await?;
    let name = item["filename"].as_str().ok_or(FORMAT_ERROR)?.to_string();
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    let (source, unit) = match extension.as_str() {
        "pdf" => (Source::Pdf, "página"),
        "png" | "jpg" | "jpeg" | "gif" | "webp" => (Source::Image, "imagen"),
        other => match office::kind(other) {
            Some(kind @ Kind::Pptx) => (Source::Office(kind), "diapositiva"),
            Some(kind) => (Source::Office(kind), "imagen"),
            None => {
                return Err(office::legacy_hint(other)
                    .map_or_else(|| UNSUPPORTED.to_string(), str::to_string));
            }
        },
    };
    let (body, _) = fetch(client, &item).await?;
    let task =
        tokio::task::spawn_blocking(move || render_source(&source, &extension, body, &wanted));
    let rendered = tokio::time::timeout(TIMEOUT, task)
        .await
        .map_err(|_| "Dibujar este archivo tardó demasiado y se canceló.")?
        .map_err(|_| "No se pudo dibujar este archivo.")??;
    let info = json!({
        "filename": name,
        "source_url": item["fileurl"],
        "module_url": module["url"],
        "unit": unit,
        "total": rendered.total,
        "shown": rendered.images.iter().map(|image| image.label.as_str()).collect::<Vec<_>>(),
        "notes": rendered.notes,
    });
    Ok(Views {
        info: client.clean(info),
        images: rendered.images,
    })
}

/// Guarda las imágenes en la carpeta de descargas (como `download_course_file`) y devuelve sus
/// rutas, para que el usuario las abra aunque su cliente no muestre las imágenes del chat.
pub async fn save(
    client: &MoodleClient,
    course_id: i64,
    settings: &Path,
    default: &Path,
    views: &Views,
) -> Result<Value, String> {
    let (root, fallback) = files::download_root(settings, default);
    let name = files::course_name(client, course_id).await?;
    let folder = files::course_folder(&root, &name, course_id)?;
    let source = views.info["source_url"].as_str().unwrap_or("").to_owned();
    let stem = Path::new(views.info["filename"].as_str().unwrap_or("figura"))
        .file_stem()
        .map_or_else(
            || "figura".to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );
    let mut paths = Vec::new();
    for image in &views.images {
        let extension = if image.mime == "image/png" {
            "png"
        } else {
            "jpg"
        };
        let wanted = folder.join(files::safe_filename(
            &format!("{stem} - {}.{extension}", image.label),
            "figura",
        ));
        let mut temporary = tempfile::Builder::new()
            .prefix(".mudel-")
            .suffix(".part")
            .tempfile_in(&folder)
            .map_err(|_| files::SAVE_ERROR)?;
        temporary
            .write_all(&image.data)
            .and_then(|()| temporary.flush())
            .map_err(|_| files::SAVE_ERROR)?;
        files::mark_from_internet(temporary.path(), &source);
        let (path, created) = files::place(temporary, &wanted)?;
        if !created {
            files::mark_from_internet(&path, &source);
        }
        paths.push(path.to_string_lossy().into_owned());
    }
    Ok(json!({"paths": paths, "folder": folder.to_string_lossy(), "note": fallback}))
}

struct Rendered {
    total: usize,
    images: Vec<Shown>,
    notes: Vec<String>,
}

fn render_source(
    source: &Source,
    extension: &str,
    body: Vec<u8>,
    wanted: &[usize],
) -> Result<Rendered, String> {
    // Un archivo malformado no debe cerrar el servidor MCP.
    std::panic::catch_unwind(|| match source {
        Source::Pdf => pdf_pages(body, wanted),
        Source::Office(kind) => office_pictures(*kind, &body, wanted),
        Source::Image => single_image(&body, extension),
    })
    .map_err(|_| String::from("No se pudo dibujar este archivo."))?
}

/// Reúne imágenes sin pasar de los límites de tamaño de la respuesta.
struct Budget {
    images: Vec<Shown>,
    notes: Vec<String>,
    bytes: usize,
    omitted: Vec<String>,
}

impl Budget {
    fn new() -> Self {
        Self {
            images: Vec::new(),
            notes: Vec::new(),
            bytes: 0,
            omitted: Vec::new(),
        }
    }

    fn add(&mut self, label: String, image: DynamicImage, photo: bool) -> Result<(), String> {
        let (mime, data) = encode(image, photo)?;
        if self.bytes + data.len() > MAX_TOTAL_BYTES {
            self.omitted.push(label);
        } else {
            self.bytes += data.len();
            self.images.push(Shown { label, mime, data });
        }
        Ok(())
    }

    fn finish(mut self, total: usize) -> Rendered {
        if !self.omitted.is_empty() {
            self.notes.push(format!(
                "Se omitieron por tamaño: {}. Pídelas en otra consulta.",
                self.omitted.join(", ")
            ));
        }
        Rendered {
            total,
            images: self.images,
            notes: self.notes,
        }
    }
}

fn pdf_pages(body: Vec<u8>, wanted: &[usize]) -> Result<Rendered, String> {
    let pdf = Pdf::new(Arc::new(body)).map_err(|error| match error {
        LoadPdfError::Decryption(_) => "El PDF está cifrado y no se puede ver sin su clave.",
        LoadPdfError::Invalid => "No se pudo abrir este PDF.",
    })?;
    let pages = pdf.pages();
    let total = pages.len();
    if let Some(missing) = wanted.iter().find(|&&page| page > total) {
        return Err(format!(
            "La página {missing} no existe: el PDF tiene {total} páginas."
        ));
    }
    let cache = RenderCache::new();
    let mut budget = Budget::new();
    for &number in wanted {
        let page = &pages[number - 1];
        let (width, height) = page.render_dimensions();
        if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
            return Err(format!("La página {number} no tiene un tamaño válido."));
        }
        let scale = (MAX_SIDE as f32 / width.max(height)).min(4.0);
        let pixmap = render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: WHITE,
            },
        );
        let (w, h) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        // El fondo es blanco y opaco: el color premultiplicado coincide con el normal.
        let rgba = RgbaImage::from_raw(w, h, pixmap.data_as_u8_slice().to_vec())
            .ok_or("No se pudo dibujar la página.")?;
        budget.add(
            format!("Página {number}"),
            DynamicImage::ImageRgba8(rgba),
            false,
        )?;
    }
    Ok(budget.finish(total))
}

fn office_pictures(kind: Kind, body: &[u8], wanted: &[usize]) -> Result<Rendered, String> {
    let found = office::pictures(kind, body, wanted)?;
    let mut budget = Budget::new();
    let mut tiny = 0;
    for picture in found.found {
        let image = decode(&picture.data)?;
        let (width, height) = image.dimensions();
        if width < MIN_SIDE && height < MIN_SIDE {
            tiny += 1;
            continue;
        }
        let photo = matches!(picture.extension.as_str(), "jpg" | "jpeg");
        budget.add(picture.label, image, photo)?;
    }
    if budget.images.is_empty() && budget.omitted.is_empty() {
        budget.notes.push(if kind == Kind::Pptx {
            "Estas diapositivas no tienen imágenes incrustadas.".into()
        } else {
            "No hay imágenes incrustadas en lo pedido.".into()
        });
    }
    if found.unsupported > 0 {
        budget.notes.push(format!(
            "{} imágenes están en un formato que no se puede mostrar (EMF, WMF, SVG…).",
            found.unsupported
        ));
    }
    if found.unrendered > 0 {
        budget.notes.push(
            "Hay gráficos o dibujos hechos con formas de Office que no se pueden mostrar; su texto sí sale con read_course_file. Si hace falta verlos, pide el PDF del material."
                .into(),
        );
    }
    if tiny > 0 {
        budget.notes.push(format!(
            "Se ignoraron {tiny} imágenes muy pequeñas (adornos)."
        ));
    }
    Ok(budget.finish(found.total))
}

fn single_image(body: &[u8], extension: &str) -> Result<Rendered, String> {
    let image = decode(body)?;
    let mut budget = Budget::new();
    budget.add("Imagen".into(), image, matches!(extension, "jpg" | "jpeg"))?;
    Ok(budget.finish(1))
}

fn decode(data: &[u8]) -> Result<DynamicImage, String> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|_| "No se pudo leer la imagen.")?;
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| "No se pudo leer la imagen o es demasiado grande.".to_string())
}

/// Reduce la imagen a `MAX_SIDE` y la codifica por debajo de `MAX_IMAGE_BYTES`: PNG para
/// dibujos y páginas, JPEG para fotos o si el PNG pesa demasiado.
fn encode(image: DynamicImage, photo: bool) -> Result<(&'static str, Vec<u8>), String> {
    let mut image = fit(image, MAX_SIDE);
    loop {
        if !photo {
            let mut png = Vec::new();
            image
                .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
                .map_err(|_| "No se pudo preparar la imagen.")?;
            if png.len() <= MAX_IMAGE_BYTES {
                return Ok(("image/png", png));
            }
        }
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 85)
            .encode_image(&flatten(&image))
            .map_err(|_| "No se pudo preparar la imagen.")?;
        if jpeg.len() <= MAX_IMAGE_BYTES {
            return Ok(("image/jpeg", jpeg));
        }
        let (width, height) = image.dimensions();
        if width.max(height) <= 400 {
            return Err("La imagen es demasiado grande para enviarla.".into());
        }
        image = image.resize(width * 7 / 10, height * 7 / 10, FilterType::Triangle);
    }
}

fn fit(image: DynamicImage, side: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width.max(height) > side {
        image.resize(side, side, FilterType::Lanczos3)
    } else {
        image
    }
}

/// Quita la transparencia (JPEG no la admite) pintando sobre blanco.
fn flatten(image: &DynamicImage) -> RgbImage {
    let rgba = image.to_rgba8();
    let mut flat = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = u32::from(pixel[3]);
        let blend = |channel: u8| ((u32::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8;
        flat.put_pixel(
            x,
            y,
            image::Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]),
        );
    }
    flat
}

#[cfg(test)]
mod tests;
