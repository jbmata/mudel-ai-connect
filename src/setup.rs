#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use eframe::egui;
use mudel_ai_connect_server::{
    credentials::{self, Kind},
    files,
    installer::{self, Client, Paths},
    updates::{self, Release},
};

#[path = "setup/theme.rs"]
mod theme;

const LOGO: &[u8] = include_bytes!("../assets/logo-setup.png");
/// Icono de la ventana y de la barra de tareas: con fondo blanco, para que se vea sobre barras oscuras.
const ICON: &[u8] = include_bytes!("../assets/logo-icon.png");

enum Event {
    Progress(String),
    Done(Result<String, String>),
}

struct Row {
    client: Client,
    selected: bool,
    state: String,
}

struct Setup {
    paths: Paths,
    url: String,
    token: String,
    saved_account: bool,
    rows: Vec<Row>,
    allowed: [bool; 5],
    folder: Option<PathBuf>,
    preferences: bool,
    confirm_remove: bool,
    token_help: bool,
    message: String,
    error: bool,
    task: Option<Receiver<Event>>,
    update_check: Option<Receiver<Option<Release>>>,
    newer: Option<Release>,
    logo: Option<egui::TextureHandle>,
    focused: bool,
    open_apps: Vec<&'static str>,
    confirm_close: bool,
    closing: bool,
}

impl Setup {
    fn new(paths: Paths, ctx: &egui::Context) -> Self {
        let logo = eframe::icon_data::from_png_bytes(LOGO).ok().map(|icon| {
            ctx.load_texture(
                "mudel-logo",
                egui::ColorImage::from_rgba_unmultiplied(
                    [icon.width as usize, icon.height as usize],
                    &icon.rgba,
                ),
                egui::TextureOptions::LINEAR,
            )
        });
        let mut setup = Self {
            paths,
            url: "https://sifo.comillas.edu".into(),
            token: String::new(),
            saved_account: false,
            rows: Vec::new(),
            allowed: [true; 5],
            folder: None,
            preferences: false,
            confirm_remove: false,
            token_help: false,
            message: String::new(),
            error: false,
            task: None,
            update_check: Some(check_for_updates(ctx)),
            newer: None,
            logo,
            focused: true,
            open_apps: Vec::new(),
            confirm_close: false,
            closing: false,
        };
        match credentials::read(&setup.paths.data, Kind::Installed) {
            Ok(Some(account)) => {
                setup.url = account.url;
                setup.saved_account = true;
            }
            Ok(None) => (),
            Err(error) => setup.fail(error),
        }
        setup.refresh();
        setup
    }

    fn refresh(&mut self) {
        let previous: std::collections::HashMap<_, _> = self
            .rows
            .iter()
            .map(|row| (row.client.key.clone(), row.selected))
            .collect();
        self.rows = self
            .paths
            .clients()
            .into_iter()
            .map(|client| Row {
                selected: previous
                    .get(&client.key)
                    .copied()
                    .unwrap_or(client.selected),
                state: self.paths.state(&client).into(),
                client,
            })
            .collect();
        if !self.open_apps.is_empty() {
            self.open_apps = installer::running_apps(&self.selected_clients());
        }
    }

    fn selected_clients(&self) -> Vec<Client> {
        self.rows
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.client.clone())
            .collect()
    }

    fn close_apps(&mut self, ctx: &egui::Context) {
        let clients = self.selected_clients();
        self.confirm_close = false;
        self.closing = true;
        self.start(ctx, move |_| installer::close_apps(&clients));
    }

    fn fail(&mut self, error: String) {
        self.message = error;
        self.error = true;
    }

    fn start(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(mpsc::Sender<Event>) -> Result<String, String> + Send + 'static,
    ) {
        self.message = "Preparando…".into();
        self.error = false;
        let (sender, receiver) = mpsc::channel();
        self.task = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(sender.clone())))
                    .unwrap_or_else(|_| {
                        Err(
                    "La operación se interrumpió. Reabre el instalador y revisa el diagnóstico."
                        .into(),
                )
                    });
            let _ = sender.send(Event::Done(result));
            ctx.request_repaint();
        });
    }

    fn install(&mut self, ctx: &egui::Context) {
        let clients: Vec<_> = self
            .rows
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.client.clone())
            .collect();
        if clients.is_empty() {
            self.fail("Marca al menos una aplicación.".into());
            return;
        }
        let paths = self.paths.clone();
        let url = self.url.clone();
        let token = std::mem::take(&mut self.token);
        self.start(ctx, move |sender| {
            let account = installer::resolve_account(&paths, &url, &token)?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "No se pudo iniciar la comprobación de conexión.")?;
            runtime.block_on(installer::install(&paths, &account, &clients, |text| {
                let _ = sender.send(Event::Progress(text.into()));
            }))
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.update_check {
            match receiver.try_recv() {
                Ok(found) => {
                    self.newer = found;
                    self.update_check = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.update_check = None,
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        if let Some(receiver) = &self.task {
            let events: Vec<_> = receiver.try_iter().collect();
            for event in events {
                match event {
                    Event::Progress(message) => self.message = message,
                    Event::Done(result) => {
                        self.task = None;
                        let relevant = result.is_ok() || self.closing;
                        self.closing = false;
                        self.error = result.is_err();
                        self.message = result.unwrap_or_else(|error| error);
                        self.open_apps = Vec::new();
                        self.refresh();
                        if relevant {
                            self.open_apps = installer::running_apps(&self.selected_clients());
                        }
                        self.saved_account = credentials::read(&self.paths.data, Kind::Installed)
                            .is_ok_and(|account| account.is_some());
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        self.poll(ctx);
        // Al volver a la ventana se revisa qué aplicaciones hay instaladas y su estado.
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(true));
        if focused && !self.focused && self.task.is_none() {
            self.refresh();
        }
        self.focused = focused;
        let busy = self.task.is_some();
        if busy && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        egui::TopBottomPanel::bottom("actions")
            .frame(
                egui::Frame::new()
                    .fill(theme::CARD)
                    .inner_margin(egui::Margin::symmetric(22, 12)),
            )
            .show(ctx, |ui| {
                self.status(ui, ctx, busy);
                ui.add_enabled_ui(!busy && !self.confirm_remove, |ui| {
                    self.actions(ui, ctx);
                });
            });
        let frame = egui::Frame::new()
            .fill(theme::BACKGROUND)
            .inner_margin(egui::Margin::symmetric(22, 16));
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            self.header(ui);
            self.update_notice(ui);
            ui.add_enabled_ui(!busy && !self.confirm_remove, |ui| {
                self.campus_panel(ui);
                ui.add_space(10.0);
                self.apps_panel(ui);
            });
            self.footer(ui, ctx, busy);
        });
        if self.preferences {
            self.show_preferences(ctx, busy);
        }
        if self.confirm_remove {
            self.show_uninstall(ctx);
        }
        if self.token_help {
            self.show_token_help(ctx);
        }
    }

    fn show_token_help(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new("Cómo obtener el token")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.label("En Moodle: tu foto o nombre → Preferencias → Cuenta de usuario → Claves de seguridad. Copia el token de Servicios web, no el de RSS.");
                ui.add_space(4.0);
                ui.label(theme::muted(
                    "Nunca introduzcas tu contraseña de Moodle aquí ni el token en un chat.",
                ));
            });
        self.token_help &= open;
    }

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(
                    egui::Image::new((logo.id(), egui::vec2(44.0, 44.0)))
                        .corner_radius(egui::CornerRadius::same(8)),
                );
            }
            ui.vertical(|ui| {
                ui.heading("Mudel AI Connect");
                ui.label(theme::muted("Conecta tus asignaturas con tu IA"));
            });
            theme::top_right(ui, |ui| {
                ui.label(theme::muted(format!("v{}", env!("CARGO_PKG_VERSION"))));
            });
        });
        ui.add_space(12.0);
    }

    fn update_notice(&self, ui: &mut egui::Ui) {
        let Some(release) = &self.newer else { return };
        theme::notice(ui, theme::MUTED, theme::CARD, |ui| {
            ui.label(format!("Hay una versión nueva ({}).", release.version));
            ui.hyperlink_to("Ver y descargar", &release.url);
        });
        ui.add_space(10.0);
    }

    fn campus_panel(&mut self, ui: &mut egui::Ui) {
        theme::panel(ui, |ui| {
            theme::section(ui, "Moodle");
            let label = theme::field_label(ui, "Dirección");
            ui.add(
                egui::TextEdit::singleline(&mut self.url)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 6)),
            )
            .labelled_by(label.id);
            ui.add_space(4.0);
            let label = theme::field_label(ui, "Token de servicios web");
            ui.add(
                egui::TextEdit::singleline(&mut self.token)
                    .password(true)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 6)),
            )
            .labelled_by(label.id);
            ui.horizontal(|ui| {
                ui.label(theme::muted(
                    "Se guarda cifrado y solo se envía a tu Moodle.",
                ));
                theme::right(ui, |ui| {
                    if theme::link(ui, "¿Cómo lo consigo?").clicked() {
                        self.token_help = true;
                    }
                });
            });
        });
    }

    fn apps_panel(&mut self, ui: &mut egui::Ui) {
        theme::panel(ui, |ui| {
            theme::section(ui, "Aplicaciones");
            if self.rows.is_empty() {
                ui.label(theme::muted(
                    "Abre una vez Claude Desktop, Claude Code o Codex y vuelve a esta ventana.",
                ));
            }
            for row in &mut self.rows {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut row.selected, &row.client.name);
                    theme::right(ui, |ui| theme::status(ui, &row.state));
                });
                if !row.client.selected && row.client.key == "codex" {
                    ui.label(theme::muted(
                        "Ya tiene el plugin; no necesita otra conexión.",
                    ));
                }
            }
        });
    }

    fn actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let installed = self.paths.installed() || self.saved_account;
        let title = if installed {
            "Actualizar / reparar"
        } else {
            "Instalar"
        };
        ui.horizontal(|ui| {
            if theme::secondary(ui, "Permisos").clicked() {
                self.allowed = match installer::read_permissions(&self.paths) {
                    Ok(values) => values,
                    Err(error) => {
                        self.fail(error);
                        [false; 5]
                    }
                };
                match installer::download_folder(&self.paths) {
                    Ok(folder) => self.folder = folder,
                    Err(error) => {
                        self.folder = None;
                        self.fail(error);
                    }
                }
                self.preferences = true;
            }
            ui.add_enabled_ui(installed, |ui| {
                if theme::danger(ui, "Desinstalar").clicked() {
                    self.confirm_remove = true;
                }
            });
            theme::right(ui, |ui| {
                if theme::primary(ui, title).clicked() {
                    self.install(ctx);
                }
            });
        });
    }

    fn status(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        if busy {
            theme::notice(ui, theme::MUTED, theme::CARD, |ui| {
                ui.spinner();
                ui.label(&self.message);
            });
            ui.label(theme::muted(
                "Espera a que termine antes de cerrar la ventana.",
            ));
            ui.add_space(8.0);
        } else if !self.message.is_empty() {
            let (color, fill) = if self.error {
                (theme::RED, theme::RED_SOFT)
            } else {
                (theme::GREEN, theme::GREEN_SOFT)
            };
            theme::notice(ui, color, fill, |ui| {
                ui.vertical(|ui| {
                    ui.label(&self.message);
                    if !self.open_apps.is_empty() {
                        self.close_prompt(ui, ctx);
                    }
                });
            });
            ui.add_space(8.0);
        }
    }

    /// Ofrece cerrar las aplicaciones abiertas para que carguen la conexión, con confirmación.
    fn close_prompt(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal_wrapped(|ui| {
            if self.confirm_close {
                ui.label("¿Cerrar sin guardar borradores?");
                if theme::link(ui, "Sí, cerrar").clicked() {
                    self.close_apps(ctx);
                }
                if theme::link(ui, "No").clicked() {
                    self.confirm_close = false;
                }
            } else {
                ui.label(format!("Abiertas: {}.", self.open_apps.join(", ")));
                if theme::link(ui, "Cerrar ahora").clicked() {
                    self.confirm_close = true;
                }
            }
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                if theme::link(ui, "Copiar diagnóstico").clicked() {
                    ctx.copy_text(installer::diagnostics(&self.paths));
                    self.message = "Diagnóstico copiado: no contiene el token.".into();
                    self.error = false;
                }
            });
            ui.label(theme::muted("·"));
            if theme::link(ui, "Versiones y actualizaciones").clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(updates::RELEASES_PAGE));
            }
        });
        ui.label(theme::muted(
            "Proyecto independiente, sin relación con Moodle ni con tu universidad.",
        ));
    }

    fn show_uninstall(&mut self, ctx: &egui::Context) {
        egui::Window::new("Desinstalar Mudel AI Connect")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(400.0);
                ui.label("Se borrarán el servidor y el token de esta instalación. Se conservarán tus permisos y la cuenta separada del plugin.");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if theme::danger(ui, "Desinstalar").clicked() {
                        self.confirm_remove = false;
                        let paths = self.paths.clone();
                        self.start(ctx, move |_| installer::uninstall(&paths));
                    }
                    if theme::secondary(ui, "Cancelar").clicked() {
                        self.confirm_remove = false;
                    }
                });
            });
    }

    fn show_preferences(&mut self, ctx: &egui::Context, busy: bool) {
        let mut open = true;
        egui::Window::new("Consultas permitidas")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(440.0)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.label("Elige qué puede consultar y descargar la IA en este equipo.");
                    ui.add_space(4.0);
                    for (index, (_, label)) in installer::PERMISSIONS.iter().enumerate() {
                        ui.checkbox(&mut self.allowed[index], *label);
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    theme::field_label(ui, "Carpeta de descargas");
                    let folder = self
                        .folder
                        .clone()
                        .or_else(|| files::default_download_folder().ok());
                    ui.label(
                        folder
                            .map(|folder| folder.display().to_string())
                            .unwrap_or_else(|| "no disponible".into()),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if theme::secondary(ui, "Cambiar carpeta…").clicked()
                            && let Some(folder) = rfd::FileDialog::new()
                                .set_title("Carpeta para las descargas")
                                .pick_folder()
                        {
                            self.folder = Some(folder);
                        }
                        if theme::secondary(ui, "Usar la predeterminada").clicked() {
                            self.folder = None;
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(theme::muted("Los permisos se aplican a las próximas consultas. El estado de conexión sigue disponible para diagnóstico."));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if theme::primary(ui, "Guardar").clicked() {
                            let paths = self.paths.clone();
                            let allowed = self.allowed;
                            let folder = self.folder.clone();
                            self.preferences = false;
                            self.start(ctx, move |_| {
                                installer::save_preferences(&paths, allowed, folder.as_deref())?;
                                Ok("Permisos guardados. Se aplican a las próximas consultas.".into())
                            });
                        }
                        if theme::secondary(ui, "Cancelar").clicked() {
                            self.preferences = false;
                        }
                    });
                });
            });
        self.preferences &= open;
    }
}

impl eframe::App for Setup {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
    }
}

/// Consulta en segundo plano si hay una versión nueva; sin red, simplemente no avisa.
fn check_for_updates(ctx: &egui::Context) -> Receiver<Option<Release>> {
    let (sender, receiver) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let found = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
            .and_then(|runtime| runtime.block_on(updates::check(env!("CARGO_PKG_VERSION"))));
        let _ = sender.send(found);
        ctx.request_repaint();
    });
    receiver
}

fn main() -> eframe::Result {
    if std::env::args().skip(1).eq(["--package-check"]) {
        match installer::check_package() {
            Ok(value) => {
                println!("{value}");
                return Ok(());
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    if std::env::args().len() != 1 {
        eprintln!("Uso: mudel-ai-connect-setup [--package-check]");
        std::process::exit(1);
    }
    let paths = match Paths::current() {
        Ok(paths) => paths,
        Err(error) => {
            rfd::MessageDialog::new()
                .set_title("Mudel AI Connect")
                .set_description(&error)
                .set_level(rfd::MessageLevel::Error)
                .show();
            return Ok(());
        }
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([540.0, 680.0])
        .with_resizable(false)
        .with_maximize_button(false);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON) {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "Mudel AI Connect",
        eframe::NativeOptions {
            viewport,
            centered: true,
            ..Default::default()
        },
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(Setup::new(paths, &cc.egui_ctx)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_and_preferences_render_without_a_display_or_real_credentials() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = Paths {
            home: temporary.path().into(),
            data: temporary.path().join("data"),
            local: temporary.path().join("local"),
            roaming: temporary.path().join("roaming"),
            macos: false,
        };
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        // Evita consultar el Llavero real al probar la vista.
        let mut setup = Setup {
            paths,
            url: "https://campus.example".into(),
            token: "fake123".into(),
            saved_account: false,
            rows: vec![],
            allowed: [true; 5],
            folder: None,
            preferences: true,
            confirm_remove: true,
            token_help: true,
            message: "Prueba".into(),
            error: false,
            task: None,
            update_check: None,
            newer: Some(Release {
                version: "v9.9.9".into(),
                url: "https://github.com/jbmata/mudel-ai-connect/releases/tag/v9.9.9".into(),
            }),
            logo: None,
            focused: true,
            open_apps: vec!["Claude Desktop", "ChatGPT / Codex"],
            confirm_close: true,
            closing: false,
        };
        for size in [[460.0, 560.0], [560.0, 800.0]] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                    ..Default::default()
                },
                |ctx| setup.show(ctx),
            );
            assert!(!output.shapes.is_empty());
        }
    }
}
