//! «Conectar con tu universidad»: obtiene el token iniciando sesión en la página oficial
//! del campus, sin que Mudel vea nunca la contraseña ni el segundo factor.
//!
//! Es el mismo mecanismo que usa la app oficial de Moodle (`admin/tool/mobile/launch.php`):
//! 1. Mudel genera un `passport` aleatorio y abre `launch.php` en el navegador.
//! 2. El usuario inicia sesión allí (SSO, doble factor…).
//! 3. Moodle redirige a `mudelai://token=<base64>`; el sistema operativo lanza Mudel con esa
//!    dirección. Esa segunda instancia solo deja la dirección en un archivo y termina.
//! 4. La ventana que espera lee el archivo, comprueba que la respuesta pertenece a este
//!    campus y a este intento (con el `passport`) y se queda con el token.

use std::{
    fs,
    path::{Path, PathBuf},
};

use base64::{Engine, prelude::BASE64_STANDARD};
use md5::{Digest, Md5};
use percent_encoding::percent_decode_str;

use crate::moodle::validate_account;

pub const SCHEME: &str = "mudelai";
/// Por ahora solo Windows sabe recibir la respuesta del navegador.
pub const SUPPORTED: bool = cfg!(windows);
const CALLBACK_FILE: &str = "login-callback.txt";
const MAX_CALLBACK: usize = 4096;
const BAD_ANSWER: &str = "La respuesta del campus no es válida. Vuelve a intentarlo.";

/// Un intento de inicio de sesión: el campus y el `passport` secreto que lo identifica.
pub struct Attempt {
    base_url: String,
    passport: String,
}

impl Attempt {
    pub fn new(url: &str) -> Result<Self, String> {
        let url = url.trim();
        let url = if url.contains("://") {
            url.to_string()
        } else {
            format!("https://{url}")
        };
        let (base_url, _) = validate_account(&url, "x")?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| "No se pudo generar una clave aleatoria en este equipo.")?;
        Ok(Self {
            base_url: base_url.to_string(),
            passport: random.iter().map(|byte| format!("{byte:02x}")).collect(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Dirección que se abre en el navegador.
    pub fn launch_url(&self) -> String {
        format!(
            "{}/admin/tool/mobile/launch.php?service=moodle_mobile_app&passport={}&urlscheme={SCHEME}",
            self.base_url, self.passport
        )
    }

    /// Devuelve el token si `callback` es la respuesta de este campus a este intento.
    pub fn accept(&self, callback: &str) -> Result<String, String> {
        let prefix = format!("{SCHEME}://token=");
        let callback = callback.trim();
        let payload = callback
            .get(..prefix.len())
            .filter(|start| start.eq_ignore_ascii_case(&prefix))
            .map(|_| &callback[prefix.len()..])
            .ok_or(BAD_ANSWER)?;
        let payload = percent_decode_str(payload)
            .decode_utf8()
            .map_err(|_| BAD_ANSWER)?;
        // Algunos navegadores añaden una «/» final a las direcciones personalizadas.
        let candidates = [Some(payload.trim()), payload.trim().strip_suffix('/')];
        let decoded = candidates
            .into_iter()
            .flatten()
            .find_map(|text| String::from_utf8(BASE64_STANDARD.decode(text).ok()?).ok())
            .ok_or(BAD_ANSWER)?;
        let mut parts = decoded.split(":::");
        let (site, token) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        // Moodle firma la respuesta con md5(dirección del campus + passport).
        let expected = format!(
            "{:x}",
            Md5::digest(format!("{}{}", self.base_url, self.passport))
        );
        if site != expected {
            return Err("La respuesta no corresponde a este campus o a este intento. Vuelve a pulsar «Conectar con tu universidad».".into());
        }
        if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(BAD_ANSWER.into());
        }
        Ok(token.to_string())
    }
}

/// Deja la dirección recibida para la ventana que espera (la segunda instancia termina después).
pub fn hand_over(directory: &Path, callback: &str) -> Result<(), String> {
    let starts_ok = callback
        .get(..SCHEME.len() + 3)
        .is_some_and(|start| start.eq_ignore_ascii_case(&format!("{SCHEME}://")));
    if !starts_ok || callback.len() > MAX_CALLBACK {
        return Err(BAD_ANSWER.into());
    }
    fs::create_dir_all(directory).map_err(|_| BAD_ANSWER)?;
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|_| BAD_ANSWER)?;
    std::io::Write::write_all(&mut file, callback.as_bytes()).map_err(|_| BAD_ANSWER)?;
    file.persist(directory.join(CALLBACK_FILE))
        .map_err(|_| BAD_ANSWER)?;
    Ok(())
}

/// Recoge (y borra) la dirección que dejó `hand_over`, si hay alguna.
pub fn take_callback(directory: &Path) -> Option<String> {
    let path = directory.join(CALLBACK_FILE);
    let bytes = fs::read(&path).ok()?;
    let _ = fs::remove_file(&path);
    if bytes.len() > MAX_CALLBACK {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Registra `mudelai://` para el usuario actual mientras dura el intento.
#[cfg(windows)]
pub fn register(scheme: &str, exe: &Path) -> Result<(), String> {
    let key = format!(r"HKCU\Software\Classes\{scheme}");
    let command_key = format!(r"{key}\shell\open\command");
    let command = format!("\"{}\" \"%1\"", exe.display());
    let steps: [Vec<&str>; 3] = [
        vec![&key, "/ve", "/d", "URL:Mudel AI Connect", "/f"],
        vec![&key, "/v", "URL Protocol", "/d", "", "/f"],
        vec![&command_key, "/ve", "/d", &command, "/f"],
    ];
    for step in steps {
        let mut args = vec!["add"];
        args.extend(step);
        if !run_reg(&args) {
            unregister(scheme);
            return Err("No se pudo preparar la conexión con el navegador en este equipo.".into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn register(_scheme: &str, _exe: &Path) -> Result<(), String> {
    Err("Conectar con tu universidad solo está disponible en Windows por ahora.".into())
}

/// Quita el registro. No falla si no existía.
pub fn unregister(scheme: &str) {
    #[cfg(windows)]
    {
        run_reg(&["delete", &format!(r"HKCU\Software\Classes\{scheme}"), "/f"]);
    }
    #[cfg(not(windows))]
    let _ = scheme;
}

#[cfg(windows)]
fn run_reg(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(args)
        .creation_flags(0x0800_0000) // sin ventana de consola
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Carpeta donde se deja la respuesta del navegador.
pub fn callback_directory() -> Result<PathBuf, String> {
    crate::credentials::data_dir()
}

#[cfg(test)]
mod tests;
