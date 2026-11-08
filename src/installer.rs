use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};
use toml_edit::{DocumentMut, InlineTable, Item};

use crate::{
    credentials::{self, Credentials, Kind},
    moodle::MoodleClient,
};

pub const NAME: &str = "mudel-ai-connect";
const OLD_NAME: &str = "moodle-ai-connect";
pub static SERVER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/server.bin"));
pub const EXE: &str = if cfg!(windows) {
    "mudel-ai-connect-server.exe"
} else {
    "mudel-ai-connect-server"
};
const IO_ERROR: &str =
    "No se pudieron guardar los cambios. Comprueba los permisos y cierra las aplicaciones de IA.";
const CONFIG_ERROR: &str =
    "La configuración de un cliente no es válida. Se ha conservado; revísala antes de instalar.";
const MAX_CONFIG: u64 = 4 * 1024 * 1024;

#[derive(Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub data: PathBuf,
    pub local: PathBuf,
    pub roaming: PathBuf,
    pub macos: bool,
}

#[derive(Clone, Copy)]
pub enum Format {
    Json { stdio: bool },
    Toml,
}

#[derive(Clone)]
pub struct Client {
    pub key: String,
    pub name: String,
    pub path: PathBuf,
    pub format: Format,
    pub selected: bool,
}

impl Paths {
    pub fn current() -> Result<Self, String> {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .ok_or(IO_ERROR)?;
        Ok(Self {
            data: credentials::data_dir()?,
            local: std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData/Local")),
            roaming: std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData/Roaming")),
            home,
            macos: cfg!(target_os = "macos"),
        })
    }

    pub fn server_dir(&self) -> PathBuf {
        self.data.join("bin/mudel-ai-connect-server")
    }
    pub fn server(&self) -> PathBuf {
        self.server_dir().join(EXE)
    }
    fn legacy(&self) -> PathBuf {
        self.data.with_file_name(OLD_NAME)
    }

    fn desktop_dirs(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        if self.macos {
            let directory = self.home.join("Library/Application Support/Claude");
            return (
                if directory.is_dir() {
                    vec![directory]
                } else {
                    vec![]
                },
                vec![],
            );
        }
        let current = directories(&self.local.join("Claude-Data"), "claude");
        let mut legacy = vec![self.roaming.join("Claude")];
        legacy.extend(
            directories(&self.local.join("Packages"), "Claude_")
                .into_iter()
                .map(|path| path.join("LocalCache/Roaming/Claude")),
        );
        legacy.retain(|path| path.is_dir());
        if current.is_empty() {
            (legacy, vec![])
        } else {
            (current, legacy)
        }
    }

    pub fn clients(&self) -> Vec<Client> {
        let mut clients: Vec<_> = self
            .desktop_dirs()
            .0
            .into_iter()
            .enumerate()
            .map(|(index, directory)| Client {
                key: format!("claude-desktop-{index}"),
                name: "Claude Desktop".into(),
                path: directory.join("claude_desktop_config.json"),
                format: Format::Json { stdio: false },
                selected: true,
            })
            .collect();
        if self.home.join(".claude.json").is_file() {
            clients.push(Client {
                key: "claude-code".into(),
                name: "Claude Code".into(),
                path: self.home.join(".claude.json"),
                format: Format::Json { stdio: true },
                selected: true,
            });
        }
        if self.home.join(".codex").is_dir() {
            let path = self.home.join(".codex/config.toml");
            let plugin = read_optional(&path)
                .ok()
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|text| text.parse::<DocumentMut>().ok())
                .is_some_and(|doc| {
                    doc.get("plugins")
                        .and_then(Item::as_table_like)
                        .is_some_and(|table| {
                            table.iter().any(|(key, _)| {
                                matches!(key.split('@').next(), Some("moodle" | NAME | OLD_NAME))
                            })
                        })
                });
            clients.push(Client {
                key: "codex".into(),
                name: "ChatGPT Work local / Codex".into(),
                path,
                format: Format::Toml,
                selected: !plugin,
            });
        }
        clients
    }

    fn all_clients(&self) -> Vec<Client> {
        let mut clients = self.clients();
        clients.extend(self.desktop_dirs().1.into_iter().map(|directory| Client {
            key: "claude-desktop-old".into(),
            name: "Claude Desktop (anterior)".into(),
            path: directory.join("claude_desktop_config.json"),
            format: Format::Json { stdio: false },
            selected: false,
        }));
        clients
    }

    pub fn installed(&self) -> bool {
        self.server_dir().exists() || self.data.join(Kind::Installed.filename()).exists()
    }

    pub fn state(&self, client: &Client) -> &'static str {
        let Some(bytes) = read_optional(&client.path).ok().flatten() else {
            return "Sin conectar";
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return "Revisar configuración";
        };
        match configured_command(&text, client.format) {
            Ok(Some(command))
                if Path::new(&command) == self.server()
                    && server_matches(&self.server_dir(), SERVER) =>
            {
                "Conectado"
            }
            Ok(Some(_)) => "Actualizar",
            Ok(None) => "Sin conectar",
            Err(_) => "Revisar configuración",
        }
    }
}

fn directories(parent: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with(prefix) && entry.path().is_dir()
        })
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    paths
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(IO_ERROR.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| IO_ERROR)?;
    if bytes.len() as u64 > MAX_CONFIG {
        return Err(CONFIG_ERROR.into());
    }
    Ok(Some(bytes))
}

fn configuration(text: &str, format: Format, command: Option<&Path>) -> Result<String, String> {
    match format {
        Format::Json { stdio } => {
            let mut value: Value = if text.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(text).map_err(|_| CONFIG_ERROR)?
            };
            let root = value.as_object_mut().ok_or(CONFIG_ERROR)?;
            let servers = root
                .entry("mcpServers")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or(CONFIG_ERROR)?;
            servers.remove(OLD_NAME);
            servers.remove(NAME);
            if let Some(command) = command {
                let mut entry = json!({"command": command.to_str().ok_or(CONFIG_ERROR)?, "args": [], "env": {}});
                if stdio {
                    entry["type"] = json!("stdio");
                }
                servers.insert(NAME.into(), entry);
            }
            serde_json::to_string_pretty(&value)
                .map(|text| text + "\n")
                .map_err(|_| CONFIG_ERROR.into())
        }
        Format::Toml => {
            let mut doc = text.parse::<DocumentMut>().map_err(|_| CONFIG_ERROR)?;
            if doc.get("mcp_servers").is_none() {
                if command.is_none() {
                    return Ok(text.into());
                }
                doc["mcp_servers"] = Item::Table(toml_edit::Table::new());
            }
            let servers = doc
                .get_mut("mcp_servers")
                .and_then(Item::as_table_like_mut)
                .ok_or(CONFIG_ERROR)?;
            servers.remove(OLD_NAME);
            servers.remove(NAME);
            if let Some(command) = command {
                let mut entry = InlineTable::new();
                entry.insert("command", command.to_str().ok_or(CONFIG_ERROR)?.into());
                entry.insert("args", toml_edit::Array::new().into());
                servers.insert(NAME, toml_edit::value(entry));
            }
            Ok(doc.to_string())
        }
    }
}

fn configured_command(text: &str, format: Format) -> Result<Option<String>, String> {
    match format {
        Format::Json { .. } => {
            let value: Value = serde_json::from_str(text).map_err(|_| CONFIG_ERROR)?;
            Ok(value
                .get("mcpServers")
                .and_then(|v| v.get(NAME))
                .and_then(|v| v.get("command"))
                .and_then(Value::as_str)
                .map(str::to_string))
        }
        Format::Toml => {
            let doc = text.parse::<DocumentMut>().map_err(|_| CONFIG_ERROR)?;
            Ok(doc
                .get("mcp_servers")
                .and_then(|v| v.get(NAME))
                .and_then(|v| v.get("command"))
                .and_then(Item::as_str)
                .map(str::to_string))
        }
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("La configuración es un enlace simbólico. No se ha sustituido; configúrala manualmente.".into());
    }
    let parent = path.parent().ok_or(IO_ERROR)?;
    fs::create_dir_all(parent).map_err(|_| IO_ERROR)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| IO_ERROR)?;
    temporary.write_all(bytes).map_err(|_| IO_ERROR)?;
    temporary.as_file().sync_all().map_err(|_| IO_ERROR)?;
    temporary.persist(path).map_err(|_| IO_ERROR)?;
    Ok(())
}

struct Change {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    backup: bool,
}

impl Change {
    fn client(client: &Client, command: Option<&Path>) -> Result<Self, String> {
        let before = read_optional(&client.path)?;
        let text =
            std::str::from_utf8(before.as_deref().unwrap_or_default()).map_err(|_| CONFIG_ERROR)?;
        let after = if command.is_none() && before.is_none() {
            None
        } else {
            Some(configuration(text, client.format, command)?.into_bytes())
        };
        Ok(Self {
            path: client.path.clone(),
            before,
            after,
            backup: true,
        })
    }

    fn apply(&self) -> Result<(), String> {
        if read_optional(&self.path)? != self.before {
            return Err("Un cliente cambió su configuración durante la instalación. Ciérralo y vuelve a intentarlo.".into());
        }
        if self.before == self.after {
            return Ok(());
        }
        if self.backup
            && let Some(before) = &self.before
        {
            let backup = self.path.with_file_name(format!(
                "{}.mudel-ai-connect-backup",
                self.path.file_name().ok_or(IO_ERROR)?.to_string_lossy()
            ));
            if !backup.exists() {
                let mut file = tempfile::NamedTempFile::new_in(self.path.parent().ok_or(IO_ERROR)?)
                    .map_err(|_| IO_ERROR)?;
                file.write_all(before).map_err(|_| IO_ERROR)?;
                file.persist_noclobber(backup).map_err(|_| IO_ERROR)?;
            }
        }
        replace(&self.path, self.after.as_deref())
    }

    fn rollback(&self) -> Result<(), String> {
        if read_optional(&self.path)? != self.after {
            return Err(IO_ERROR.into());
        }
        replace(&self.path, self.before.as_deref())
    }
}

fn replace(path: &Path, bytes: Option<&[u8]>) -> Result<(), String> {
    if let Some(bytes) = bytes {
        return atomic_write(path, bytes);
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(IO_ERROR.into()),
    }
}

fn lock(directory: &Path) -> Result<File, String> {
    fs::create_dir_all(directory).map_err(|_| IO_ERROR)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("install.lock"))
        .map_err(|_| IO_ERROR)?;
    file.try_lock().map_err(
        |_| "Hay otra instalación o cambio de permisos en marcha. Espera a que termine.",
    )?;
    Ok(file)
}

fn server_matches(directory: &Path, payload: &[u8]) -> bool {
    !payload.is_empty()
        && fs::read(directory.join(EXE))
            .is_ok_and(|bytes| Sha256::digest(bytes) == Sha256::digest(payload))
}

struct ServerChange {
    target: PathBuf,
    backup: PathBuf,
    had_old: bool,
    published: bool,
    /// El servidor anterior estaba en uso: solo se apartó su ejecutable y se conserva la carpeta.
    in_place: bool,
}

impl ServerChange {
    fn apply(target: &Path, payload: Option<&[u8]>) -> Result<Option<Self>, String> {
        if payload.is_some_and(|bytes| server_matches(target, bytes))
            || (payload.is_none() && !target.exists())
        {
            return Ok(None);
        }
        if fs::symlink_metadata(target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(IO_ERROR.into());
        }
        let parent = target.parent().ok_or(IO_ERROR)?;
        fs::create_dir_all(parent).map_err(|_| IO_ERROR)?;
        let staging = tempfile::Builder::new()
            .prefix("mudel-new-")
            .tempdir_in(parent)
            .map_err(|_| IO_ERROR)?;
        if let Some(payload) = payload {
            atomic_write(&staging.path().join(EXE), payload)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(staging.path().join(EXE), fs::Permissions::from_mode(0o700))
                    .map_err(|_| IO_ERROR)?;
            }
            atomic_write(
                &staging.path().join("LICENSE"),
                include_bytes!("../LICENSE"),
            )?;
        }
        let backup = tempfile::Builder::new()
            .prefix("mudel-previous-")
            .tempdir_in(parent)
            .map_err(|_| IO_ERROR)?
            .keep();
        let mut change = Self {
            target: target.into(),
            backup,
            had_old: target.exists(),
            published: false,
            in_place: false,
        };
        if change.had_old && fs::rename(target, change.backup.join("server")).is_err() {
            // Windows no deja renombrar una carpeta con un programa en marcha, pero sí su
            // ejecutable: se aparta solo ese archivo y el programa sigue con su copia.
            if !target.join(EXE).is_file()
                || fs::rename(target.join(EXE), change.backup.join("aside")).is_err()
            {
                let _ = fs::remove_dir(&change.backup);
                return Err(IO_ERROR.into());
            }
            change.in_place = true;
        }
        if change.in_place {
            if payload.is_some() {
                change.published = true;
                let placed = fs::rename(staging.path().join(EXE), target.join(EXE)).is_ok()
                    && atomic_write(&target.join("LICENSE"), include_bytes!("../LICENSE")).is_ok();
                if !placed {
                    change.rollback()?;
                    return Err(IO_ERROR.into());
                }
            } else {
                // Desinstalar: se borra lo que no esté en uso.
                let _ = remove_tree(target);
            }
        } else if payload.is_some() {
            if fs::rename(staging.path(), target).is_err() {
                change.rollback()?;
                return Err(IO_ERROR.into());
            }
            change.published = true;
        }
        Ok(Some(change))
    }

    fn rollback(&self) -> Result<(), String> {
        if self.in_place {
            if self.published {
                let _ = fs::remove_file(self.target.join(EXE));
            }
            fs::create_dir_all(&self.target).map_err(|_| IO_ERROR)?;
            fs::rename(self.backup.join("aside"), self.target.join(EXE)).map_err(|_| IO_ERROR)?;
            return fs::remove_dir(&self.backup).map_err(|_| IO_ERROR.into());
        }
        if self.published {
            remove_tree(&self.target)?;
        }
        if self.had_old {
            fs::rename(self.backup.join("server"), &self.target).map_err(|_| IO_ERROR)?;
        }
        fs::remove_dir(&self.backup).map_err(|_| IO_ERROR)?;
        Ok(())
    }

    fn finish(&self) -> Result<(), String> {
        // Si la copia anterior sigue en uso (una app con el servidor abierto), no es un fallo:
        // `sweep` la borrará en la siguiente ejecución.
        let _ = remove_tree(&self.backup);
        Ok(())
    }
}

fn remove_tree(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    #[allow(clippy::permissions_set_readonly_false)] // En Windows quita el atributo; no cambia las ACL.
    fn writable(path: &Path) -> std::io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Ok(());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                writable(&entry?.path())?;
            }
        }
        if metadata.permissions().readonly() {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)?;
        }
        Ok(())
    }
    #[cfg(windows)]
    writable(path).map_err(|_| IO_ERROR)?;
    fs::remove_dir_all(path).map_err(|_| IO_ERROR.into())
}

/// Aplicaciones marcadas que están en marcha. Con `kill`, además las fuerza a cerrar.
fn scan_apps(clients: &[Client], kill: bool) -> Vec<&'static str> {
    let selected = |prefix: &str| clients.iter().any(|client| client.key.starts_with(prefix));
    let (desktop, codex) = (selected("claude-desktop"), selected("codex"));
    if !desktop && !codex {
        return Vec::new();
    }
    let system = System::new_with_specifics(
        RefreshKind::nothing()
            .with_processes(ProcessRefreshKind::nothing().with_exe(UpdateKind::Always)),
    );
    let own = std::process::id();
    let mut apps = Vec::new();
    for (pid, process) in system.processes() {
        let Some(exe) = process.exe() else {
            continue;
        };
        let text = exe.to_string_lossy().to_lowercase();
        let stem = exe
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_lowercase());
        let name = if desktop && is_claude_desktop(&text) {
            "Claude Desktop"
        } else if codex && matches!(stem.as_deref(), Some("codex" | "chatgpt")) {
            "ChatGPT / Codex"
        } else {
            continue;
        };
        if pid.as_u32() == own {
            continue;
        }
        if kill {
            process.kill();
        }
        if !apps.contains(&name) {
            apps.push(name);
        }
    }
    apps.sort_unstable();
    apps
}

/// Las aplicaciones solo leen su configuración al arrancar: si alguna está abierta no se
/// bloquea la instalación, pero hay que avisar de que debe reiniciarse.
pub fn running_apps(clients: &[Client]) -> Vec<&'static str> {
    scan_apps(clients, false)
}

/// Cierra a la fuerza las aplicaciones marcadas que estén abiertas (se pierden los borradores
/// sin enviar). Solo se llama cuando el usuario lo pide.
pub fn close_apps(clients: &[Client]) -> Result<String, String> {
    let closed = scan_apps(clients, true);
    if closed.is_empty() {
        return Ok("No había aplicaciones abiertas.".into());
    }
    let mut remaining = closed.clone();
    for _ in 0..15 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        remaining = scan_apps(clients, false);
        if remaining.is_empty() {
            break;
        }
    }
    if remaining.is_empty() {
        Ok(format!(
            "Cerrado: {}. Ábrelo de nuevo para que cargue la conexión.",
            closed.join(", ")
        ))
    } else {
        Err(format!(
            "No se pudo cerrar {}. Ciérralo tú del todo, también desde la bandeja.",
            remaining.join(", ")
        ))
    }
}

fn is_claude_desktop(exe: &str) -> bool {
    (exe.ends_with("\\claude.exe")
        && [
            "\\windowsapps\\claude_",
            "\\anthropicclaude\\",
            "\\programs\\claude\\",
        ]
        .iter()
        .any(|part| exe.contains(part)))
        || exe.contains("/claude.app/contents/macos/")
}

/// Borra sin molestar los restos de instalaciones anteriores: copias que seguían en uso y
/// archivos de versiones que no son la actual.
fn sweep(paths: &Paths) {
    let server_dir = paths.server_dir();
    if let Some(bin) = server_dir.parent() {
        for entry in fs::read_dir(bin).into_iter().flatten().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("mudel-previous-")
            {
                let _ = remove_tree(&entry.path());
            }
        }
    }
    if server_matches(&server_dir, SERVER) {
        for entry in fs::read_dir(&server_dir).into_iter().flatten().flatten() {
            if entry.file_name() != EXE && entry.file_name() != "LICENSE" {
                let _ = remove_tree(&entry.path());
            }
        }
    }
}

pub fn resolve_account(paths: &Paths, url: &str, token: &str) -> Result<Credentials, String> {
    let url = if url.trim().contains("://") {
        url.trim().to_string()
    } else {
        format!("https://{}", url.trim())
    };
    if !token.trim().is_empty() {
        return Credentials::new(&url, token);
    }
    let saved = credentials::read(&paths.data, Kind::Installed)?;
    if let Some(saved) = saved
        && saved.url == url.trim_end_matches('/')
    {
        return Ok(saved);
    }
    Err("Introduce tu token de servicios web. Solo se reutiliza la cuenta guardada para el mismo campus.".into())
}

pub async fn check_account(account: &Credentials) -> Result<Value, String> {
    MoodleClient::new(&account.url, &account.token)?
        .status()
        .await
}

async fn verify_binary(binary: &Path) -> Result<(), String> {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--check")
        .env_remove("MOODLE_URL")
        .env_remove("MOODLE_TOKEN")
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = tokio::time::timeout(Duration::from_secs(90), command.output())
        .await
        .map_err(|_| "La comprobación del servidor instalado agotó el tiempo de espera.")?
        .map_err(|_| "No se pudo ejecutar el servidor instalado.")?;
    if !output.status.success() {
        return Err("El servidor instalado no pudo conectar con la cuenta guardada. Se restaurará la instalación anterior.".into());
    }
    serde_json::from_slice::<Value>(&output.stdout)
        .map_err(|_| "El servidor instalado no devolvió un estado válido.")?;
    Ok(())
}

pub async fn install(
    paths: &Paths,
    account: &Credentials,
    clients: &[Client],
    progress: impl Fn(&str),
) -> Result<String, String> {
    if SERVER.is_empty() {
        return Err("Este ejecutable de desarrollo no incluye el servidor. Genera el paquete con scripts/package.ps1.".into());
    }
    if clients.is_empty() {
        return Err("Marca al menos una aplicación.".into());
    }
    progress("Probando la conexión con Moodle…");
    let info = check_account(account).await?;
    let _guard = lock(&paths.data)?;
    let changes = installation_changes(paths, clients)?;
    progress("Instalando el servidor y conectando los clientes…");
    let servers = change_servers(paths, Some(SERVER))?;
    let result = apply_installation(paths, account, &changes, || async {
        verify_binary(&paths.server()).await
    })
    .await;
    complete_servers(servers, result)?;
    sweep(paths);
    let mut message = format!(
        "Conectado: {} en {}. Abre una sesión nueva.",
        info["user"].as_str().unwrap_or("tu cuenta"),
        info["site"].as_str().unwrap_or("Moodle")
    );
    if let Some(missing) = info["missing_functions"].as_array()
        && !missing.is_empty()
    {
        message.push_str(&format!(
            " Consultas sin autorización del campus: {}.",
            missing
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(message)
}

fn installation_changes(paths: &Paths, selected: &[Client]) -> Result<Vec<Change>, String> {
    let mut changes = Vec::new();
    for client in paths.all_clients() {
        if selected.iter().any(|selected| selected.path == client.path) {
            changes.push(Change::client(&client, Some(&paths.server()))?);
        } else if client.key == "claude-desktop-old" {
            changes.push(Change::client(&client, None)?);
        } else if let Some(before) = read_optional(&client.path)? {
            let text = std::str::from_utf8(&before).map_err(|_| CONFIG_ERROR)?;
            // Solo migrar la entrada antigua; no desconectar clientes que el usuario no seleccionó.
            let after = remove_legacy(text, client.format)?;
            changes.push(Change {
                path: client.path,
                before: Some(before),
                after: Some(after.into_bytes()),
                backup: true,
            });
        }
    }
    add_legacy_credentials(paths, &mut changes)?;
    Ok(changes)
}

fn remove_legacy(text: &str, format: Format) -> Result<String, String> {
    match format {
        Format::Json { .. } => {
            let mut value: Value = serde_json::from_str(text).map_err(|_| CONFIG_ERROR)?;
            if let Some(servers) = value.get_mut("mcpServers").and_then(Value::as_object_mut) {
                if servers.remove(OLD_NAME).is_none() {
                    return Ok(text.into());
                }
            } else {
                return Ok(text.into());
            }
            serde_json::to_string_pretty(&value)
                .map(|text| text + "\n")
                .map_err(|_| CONFIG_ERROR.into())
        }
        Format::Toml => {
            let mut doc = text.parse::<DocumentMut>().map_err(|_| CONFIG_ERROR)?;
            if let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut) {
                servers.remove(OLD_NAME);
            }
            Ok(doc.to_string())
        }
    }
}

async fn apply_installation<F, Fut>(
    paths: &Paths,
    account: &Credentials,
    changes: &[Change],
    verify: F,
) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let previous = credentials::snapshot(&paths.data, Kind::Installed)?;
    let mut applied = 0;
    let mut saved = false;
    let result = async {
        for change in changes {
            change.apply()?;
            applied += 1;
        }
        saved = true;
        credentials::save(&paths.data, Kind::Installed, account)?;
        verify().await
    }
    .await;
    if let Err(error) = result {
        let mut restored = true;
        if saved {
            restored &=
                credentials::restore(&paths.data, Kind::Installed, previous.as_deref()).is_ok();
        }
        for change in changes[..applied].iter().rev() {
            restored &= change.rollback().is_ok();
        }
        return Err(if restored {
            error
        } else {
            format!(
                "{error} No se pudo restaurar todo; se conservan copias de seguridad en las carpetas originales."
            )
        });
    }
    Ok(())
}

fn change_servers(paths: &Paths, payload: Option<&[u8]>) -> Result<Vec<ServerChange>, String> {
    let mut changes = Vec::new();
    for (target, content) in [
        (paths.server_dir(), payload),
        (paths.legacy().join("bin"), None),
    ] {
        match ServerChange::apply(&target, content) {
            Ok(Some(change)) => changes.push(change),
            Ok(None) => (),
            Err(error) => return complete_servers(changes, Err(error)).map(|()| vec![]),
        }
    }
    Ok(changes)
}

fn complete_servers(changes: Vec<ServerChange>, result: Result<(), String>) -> Result<(), String> {
    let mut failures = Vec::new();
    for change in changes.iter().rev() {
        let outcome = if result.is_err() {
            change.rollback()
        } else {
            change.finish()
        };
        if outcome.is_err() {
            failures.push(change.backup.display().to_string());
        }
    }
    if !failures.is_empty() {
        return Err(format!("{} Revisa las copias del servidor que se conservan en: {}.",
            result.err().unwrap_or_else(|| "La instalación funciona, pero no se pudieron borrar todos los archivos anteriores.".into()), failures.join(", ")));
    }
    result
}

fn add_legacy_credentials(paths: &Paths, changes: &mut Vec<Change>) -> Result<(), String> {
    let path = paths.legacy().join(Kind::Installed.filename());
    if let Some(before) = read_optional(&path)? {
        changes.push(Change {
            path,
            before: Some(before),
            after: None,
            backup: false,
        });
    }
    Ok(())
}

pub fn uninstall(paths: &Paths) -> Result<String, String> {
    let _guard = lock(&paths.data)?;
    let clients = paths.all_clients();
    let mut changes = clients
        .iter()
        .map(|client| Change::client(client, None))
        .collect::<Result<Vec<_>, _>>()?;
    add_legacy_credentials(paths, &mut changes)?;
    let previous = credentials::snapshot(&paths.data, Kind::Installed)?;
    let servers = change_servers(paths, None)?;
    let mut applied = 0;
    let result = (|| {
        for change in &changes {
            change.apply()?;
            applied += 1;
        }
        credentials::remove(&paths.data, Kind::Installed)
    })();
    let result = result.map_err(|error| {
        let mut restored =
            credentials::restore(&paths.data, Kind::Installed, previous.as_deref()).is_ok();
        for change in changes[..applied].iter().rev() {
            restored &= change.rollback().is_ok();
        }
        if restored {
            error
        } else {
            format!("{error} Revisa las copias de seguridad de configuración.")
        }
    });
    complete_servers(servers, result)?;
    Ok("Desconectado. El token se borró de este equipo, pero sigue siendo válido en Moodle hasta que lo restablezcas allí.".into())
}

pub const PERMISSIONS: [(&str, &str); 5] = [
    ("courses", "Asignaturas"),
    ("materials", "Materiales y archivos"),
    ("assignments", "Tareas"),
    ("calendar", "Calendario"),
    ("downloads", "Descargar archivos"),
];

pub fn read_permissions(paths: &Paths) -> Result<[bool; 5], String> {
    let Some(bytes) = read_optional(&paths.data.join("permissions.json"))? else {
        return Ok([true; 5]);
    };
    let mut values: std::collections::BTreeMap<String, bool> =
        serde_json::from_slice(&bytes).map_err(|_| CONFIG_ERROR)?;
    if !values.contains_key("downloads")
        && let Some(&materials) = values.get("materials")
    {
        values.insert("downloads".into(), materials);
    }
    if values.len() != PERMISSIONS.len() {
        return Err(CONFIG_ERROR.into());
    }
    let mut result = [false; 5];
    for (index, (key, _)) in PERMISSIONS.iter().enumerate() {
        result[index] = *values.get(*key).ok_or(CONFIG_ERROR)?;
    }
    Ok(result)
}

pub fn download_folder(paths: &Paths) -> Result<Option<PathBuf>, String> {
    let Some(bytes) = read_optional(&paths.data.join("downloads.json"))? else {
        return Ok(None);
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| CONFIG_ERROR)?;
    let folder = PathBuf::from(value["folder"].as_str().ok_or(CONFIG_ERROR)?);
    if !folder.is_absolute() {
        return Err(CONFIG_ERROR.into());
    }
    Ok(Some(folder))
}

pub fn save_preferences(
    paths: &Paths,
    allowed: [bool; 5],
    folder: Option<&Path>,
) -> Result<(), String> {
    if folder.is_some_and(|folder| !folder.is_absolute() || !folder.is_dir()) {
        return Err("Elige una carpeta existente con una ruta completa.".into());
    }
    let _guard = lock(&paths.data)?;
    let values: serde_json::Map<_, _> = PERMISSIONS
        .iter()
        .enumerate()
        .map(|(index, (key, _))| ((*key).into(), json!(allowed[index])))
        .collect();
    let changes = [
        Change {
            path: paths.data.join("permissions.json"),
            backup: false,
            before: read_optional(&paths.data.join("permissions.json"))?,
            after: Some(serde_json::to_vec(&values).map_err(|_| IO_ERROR)?),
        },
        Change {
            path: paths.data.join("downloads.json"),
            backup: false,
            before: read_optional(&paths.data.join("downloads.json"))?,
            after: folder
                .map(|folder| serde_json::to_vec(&json!({"folder": folder})).map_err(|_| IO_ERROR))
                .transpose()?,
        },
    ];
    changes[0].apply()?;
    if let Err(error) = changes[1].apply() {
        changes[0].rollback()?;
        return Err(error);
    }
    Ok(())
}

pub fn diagnostics(paths: &Paths) -> String {
    let mut text = format!(
        "Mudel AI Connect {} (Rust)\nSistema: {} / {}\nServidor instalado: {}\nServidor actual: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        paths.server().is_file(),
        server_matches(&paths.server_dir(), SERVER)
    );
    for client in paths.clients() {
        text.push_str(&format!("{}: {}\n", client.name, paths.state(&client)));
    }
    text.push_str(&format!(
        "Permisos válidos: {}\n",
        read_permissions(paths).is_ok()
    ));
    text
}

pub fn check_package() -> Result<Value, String> {
    if SERVER.is_empty() {
        return Err("El paquete no contiene el servidor.".into());
    }
    let directory = tempfile::tempdir().map_err(|_| IO_ERROR)?;
    let binary = directory.path().join(EXE);
    atomic_write(&binary, SERVER)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).map_err(|_| IO_ERROR)?;
    }
    let mut command = std::process::Command::new(binary);
    command
        .arg("--check")
        .env("MOODLE_URL", "http://invalid.example")
        .env("MOODLE_TOKEN", "FakePackageTest123");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .output()
        .map_err(|_| "El servidor incluido no se pudo ejecutar.")?;
    if output.status.success() || !String::from_utf8_lossy(&output.stderr).contains("HTTPS") {
        return Err("El servidor incluido no pasó la comprobación del paquete.".into());
    }
    Ok(
        json!({"version": env!("CARGO_PKG_VERSION"), "server_bytes": SERVER.len(), "server_sha256": format!("{:x}", Sha256::digest(SERVER))}),
    )
}

#[cfg(test)]
mod tests;
