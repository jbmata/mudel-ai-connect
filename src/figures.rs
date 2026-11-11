//! Detecta qué páginas de un PDF tienen figuras (imágenes o dibujos de líneas), para avisar al
//! modelo de que el texto no lo cuenta todo y puede usar `view_course_file`.
//!
//! Se interpreta cada página con `hayro` sin dibujarla: solo se cuentan las imágenes y los trazos
//! que no son rectas horizontales ni verticales (las tablas y los subrayados se ignoran).

use std::{sync::Arc, time::Instant};

use hayro::kurbo::{BezPath, PathEl, Point, Rect};
use hayro::{
    hayro_interpret::{
        BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
        InterpreterCache, InterpreterSettings, SoftMask, font::GlyphRun, interpret_page,
        util::TransformExt,
    },
    hayro_syntax::Pdf,
};
use serde_json::{Value, json};

/// Páginas que se analizan como máximo y tiempo máximo, para no retrasar la lectura del texto.
const MAX_PAGES: usize = 60;
const BUDGET_SECONDS: u64 = 6;
/// Trazos curvos o diagonales a partir de los cuales una página se considera con dibujos.
const DRAWING_MARKS: usize = 25;
/// Lado menor a partir del cual una imagen deja de ser un adorno.
const MIN_IMAGE_SIDE: u32 = 48;
/// Páginas que se citan como máximo en el aviso.
const MAX_LISTED: usize = 20;

#[derive(Default)]
struct Counter {
    images: usize,
    marks: usize,
}

impl Device<'_> for Counter {
    fn draw_path(&mut self, path: &BezPath, _: DrawProps<'_>, _: &DrawMode) {
        let mut previous = Point::ZERO;
        for element in path.elements() {
            match *element {
                PathEl::MoveTo(point) => previous = point,
                PathEl::LineTo(point) => {
                    // Las rectas horizontales y verticales son tablas, marcos y subrayados.
                    if (point.x - previous.x).abs() > 0.5 && (point.y - previous.y).abs() > 0.5 {
                        self.marks += 1;
                    }
                    previous = point;
                }
                PathEl::QuadTo(_, point) | PathEl::CurveTo(_, _, point) => {
                    self.marks += 1;
                    previous = point;
                }
                PathEl::ClosePath => (),
            }
        }
    }
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, '_>, _: DrawProps<'_>, _: &DrawMode) {}
    fn draw_image(&mut self, image: Image<'_, '_>, _: ImageDrawProps<'_>) {
        if image.width() >= MIN_IMAGE_SIDE || image.height() >= MIN_IMAGE_SIDE {
            self.images += 1;
        }
    }
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// Resumen de las páginas con figuras de un PDF, o `None` si no se encuentra ninguna o el
/// documento no se puede analizar. Nunca falla: es un aviso, no parte de la lectura.
pub fn pdf(body: &[u8]) -> Option<Value> {
    std::panic::catch_unwind(|| analyse(body)).ok().flatten()
}

/// Cuenta las imágenes y los trazos de dibujo de una página sin dibujarla.
fn count<'a>(
    page: &'a hayro::hayro_syntax::page::Page<'a>,
    cache: &InterpreterCache<'a>,
) -> Counter {
    let (width, height) = page.render_dimensions();
    let target = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
    let mut state = Context::new(
        page.initial_transform(true).to_kurbo(),
        target,
        cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut counter = Counter::default();
    interpret_page(page, &mut state, &mut counter);
    counter
}

impl Counter {
    fn is_figure(&self) -> bool {
        self.images > 0 || self.marks >= DRAWING_MARKS
    }
}

/// De las páginas pedidas (desde 1), las que tienen figuras. Nunca falla.
pub fn flagged(body: &[u8], wanted: &[usize]) -> Vec<usize> {
    std::panic::catch_unwind(|| {
        let Ok(pdf) = Pdf::new(Arc::new(body.to_vec())) else {
            return Vec::new();
        };
        let pages = pdf.pages();
        let cache = InterpreterCache::new();
        let started = Instant::now();
        let mut found = Vec::new();
        for &number in wanted {
            if started.elapsed().as_secs() >= BUDGET_SECONDS {
                break;
            }
            if let Some(page) = number.checked_sub(1).and_then(|index| pages.get(index))
                && count(page, &cache).is_figure()
            {
                found.push(number);
            }
        }
        found
    })
    .unwrap_or_default()
}

fn analyse(body: &[u8]) -> Option<Value> {
    let pdf = Pdf::new(Arc::new(body.to_vec())).ok()?;
    let pages = pdf.pages();
    let started = Instant::now();
    let cache = InterpreterCache::new();
    let mut found = Vec::new();
    let mut analysed = 0;
    for (index, page) in pages.iter().enumerate().take(MAX_PAGES) {
        if started.elapsed().as_secs() >= BUDGET_SECONDS {
            break;
        }
        let counter = count(page, &cache);
        analysed += 1;
        if counter.is_figure() {
            found.push(json!({
                "page": index + 1,
                "images": counter.images,
                "drawings": counter.marks >= DRAWING_MARKS,
            }));
        }
    }
    if found.is_empty() {
        return None;
    }
    let total = found.len();
    found.truncate(MAX_LISTED);
    let mut summary = json!({
        "pages": found,
        "note": "Estas páginas tienen imágenes o dibujos que no salen en el texto. Se adjuntan como imágenes las del fragmento leído; para ver otras usa view_course_file.",
    });
    if total > MAX_LISTED {
        summary["more_pages_with_figures"] = json!(total - MAX_LISTED);
    }
    if analysed < pages.len() {
        summary["analysed_pages"] = json!(analysed);
    }
    Some(summary)
}

#[cfg(test)]
pub(crate) mod tests;
