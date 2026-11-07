//! Aspecto de la ventana: sobrio, a la manera de un instalador de escritorio.

use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, FontDefinitions, FontFamily, FontId, Frame, Layout,
    Margin, Response, RichText, Stroke, TextStyle, Ui, Vec2,
};

pub const PINK: Color32 = Color32::from_rgb(249, 1, 124);
pub const PINK_DARK: Color32 = Color32::from_rgb(190, 0, 94);
pub const INK: Color32 = Color32::from_rgb(28, 28, 32);
pub const MUTED: Color32 = Color32::from_rgb(100, 102, 112);
pub const BACKGROUND: Color32 = Color32::from_rgb(243, 243, 245);
pub const CARD: Color32 = Color32::WHITE;
pub const BORDER: Color32 = Color32::from_rgb(220, 220, 226);
pub const GREEN: Color32 = Color32::from_rgb(21, 128, 61);
pub const GREEN_SOFT: Color32 = Color32::from_rgb(235, 247, 239);
pub const AMBER: Color32 = Color32::from_rgb(160, 82, 8);
pub const RED: Color32 = Color32::from_rgb(176, 24, 64);
pub const RED_SOFT: Color32 = Color32::from_rgb(253, 240, 243);

/// Familia con más peso para títulos. Si el sistema no la ofrece, usa la normal.
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

/// Usa la tipografía del sistema cuando se puede (Segoe UI en Windows); si no, la incluida.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    #[cfg_attr(not(windows), allow(unused_mut))]
    let (mut regular, mut heavy) = (fallback.clone(), fallback);
    #[cfg(windows)]
    {
        let folder = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into())
            .join("Fonts");
        for (file, key, target) in [
            ("segoeui.ttf", "system-regular", &mut regular),
            ("segoeuib.ttf", "system-bold", &mut heavy),
        ] {
            if let Ok(bytes) = std::fs::read(folder.join(file)) {
                fonts.font_data.insert(
                    key.into(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                target.insert(0, key.into());
            }
        }
    }
    fonts.families.insert(FontFamily::Proportional, regular);
    fonts.families.insert(bold(), heavy);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::ThemePreference::Light);
    ctx.all_styles_mut(|style| {
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = BACKGROUND;
        visuals.window_fill = CARD;
        visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.window_corner_radius = CornerRadius::same(8);
        visuals.extreme_bg_color = CARD;
        visuals.hyperlink_color = PINK_DARK;
        visuals.selection.bg_fill = PINK.gamma_multiply(0.35);
        visuals.selection.stroke = Stroke::new(1.0_f32, INK);
        visuals.override_text_color = Some(INK);
        let radius = CornerRadius::same(4);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        for (widget, fill, stroke) in [
            (
                &mut visuals.widgets.inactive,
                CARD,
                Color32::from_rgb(196, 196, 205),
            ),
            (
                &mut visuals.widgets.hovered,
                Color32::from_rgb(246, 246, 249),
                Color32::from_rgb(150, 150, 162),
            ),
            (
                &mut visuals.widgets.active,
                Color32::from_rgb(236, 236, 241),
                Color32::from_rgb(120, 120, 134),
            ),
        ] {
            widget.weak_bg_fill = fill;
            widget.bg_fill = fill;
            widget.bg_stroke = Stroke::new(1.0_f32, stroke);
            widget.corner_radius = radius;
            widget.fg_stroke = Stroke::new(1.0_f32, INK);
        }
        visuals.widgets.noninteractive.corner_radius = radius;
        visuals.widgets.open.corner_radius = radius;
        style.visuals = visuals;
        style.spacing.item_spacing = Vec2::new(10.0, 7.0);
        style.spacing.button_padding = Vec2::new(14.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.scroll = egui::style::ScrollStyle::solid();
        style.text_styles.extend([
            (TextStyle::Heading, FontId::new(21.0, bold())),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
        ]);
    });
}

/// Panel blanco con borde fino y esquinas poco redondeadas.
pub fn panel<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Título de un panel.
pub fn section(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(FontId::new(15.0, bold())));
    ui.add_space(2.0);
}

/// Etiqueta sobre un campo.
pub fn field_label(ui: &mut Ui, text: &str) -> Response {
    ui.label(RichText::new(text).small().color(MUTED))
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).small().color(MUTED)
}

/// Botón principal de la barra inferior.
pub fn primary(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        Button::new(
            RichText::new(text)
                .font(FontId::new(14.0, bold()))
                .color(Color32::WHITE),
        )
        .fill(PINK)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(170.0, 34.0)),
    )
}

pub fn secondary(ui: &mut Ui, text: &str) -> Response {
    ui.add(Button::new(text).min_size(Vec2::new(0.0, 32.0)))
}

/// Acción destructiva: texto rojo, mismo contorno que los demás botones.
pub fn danger(ui: &mut Ui, text: &str) -> Response {
    ui.add(Button::new(RichText::new(text).color(RED)).min_size(Vec2::new(0.0, 32.0)))
}

/// Enlace de texto discreto, sin recuadro.
pub fn link(ui: &mut Ui, text: &str) -> Response {
    ui.add(Button::new(RichText::new(text).small().color(MUTED)).frame(false))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Estado de una aplicación: punto y texto, sin recuadro.
pub fn status(ui: &mut Ui, text: &str) {
    let color = match text {
        "Conectado" => GREEN,
        "Actualizar" => PINK_DARK,
        "Sin conectar" => MUTED,
        _ => AMBER,
    };
    ui.label(RichText::new(format!("●  {text}")).small().color(color));
}

/// Aviso de resultado o de progreso.
pub fn notice(ui: &mut Ui, color: Color32, fill: Color32, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| add(ui));
        });
}

/// Coloca un elemento arriba a la derecha sin estirarlo a lo alto de la fila.
pub fn top_right<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.with_layout(Layout::top_down(Align::Max), add).inner
}

/// Alinea un elemento a la derecha de la fila actual.
pub fn right<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.with_layout(Layout::right_to_left(Align::Center), add)
        .inner
}
