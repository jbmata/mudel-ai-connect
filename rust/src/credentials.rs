use std::{
    fs::File,
    io::{BufRead, IsTerminal, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::moodle::validate_account;

const READ_ERROR: &str = "No se pudo leer la cuenta guardada. Vuelve a configurarla con --setup.";
const SAVE_ERROR: &str = "No se pudo guardar la cuenta en el almacén seguro del sistema.";
const INPUT_ERROR: &str = "No se pudo leer la cuenta del terminal.";
const MAX_CONFIG: u64 = 64 * 1024;

#[derive(Deserialize, Serialize)]
pub struct Credentials {
    #[serde(rename = "MOODLE_URL")]
    pub url: String,
    #[serde(rename = "MOODLE_TOKEN")]
    pub token: String,
}

impl Credentials {
    fn new(url: &str, token: &str) -> Result<Self, String> {
        let (url, token) = validate_account(url, token)?;
        Ok(Self {
            url: url.into(),
            token: token.into(),
        })
    }
}

pub fn data_dir() -> Result<PathBuf, String> {
    let home = || {
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .ok_or_else(|| READ_ERROR.to_string())
    };
    let parent = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| home().map(|home| home.join("AppData/Local")))?
    } else {
        // Comparte permisos y descargas con Python aunque la cuenta use el Llavero.
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| home().map(|home| home.join(".local/share")))?
    };
    Ok(parent.join("mudel-ai-connect"))
}

pub fn load() -> Result<Credentials, String> {
    match (std::env::var_os("MOODLE_URL"), std::env::var_os("MOODLE_TOKEN")) {
        (Some(url), Some(token)) => Credentials::new(
            url.to_str().ok_or(READ_ERROR)?, token.to_str().ok_or(READ_ERROR)?,
        ),
        // No combinar el token guardado con una URL del entorno que podría ser de otro campus.
        (Some(_), None) | (None, Some(_)) => Err(
            "Define MOODLE_URL y MOODLE_TOKEN juntas, o elimina ambas para usar la cuenta guardada.".into()
        ),
        (None, None) => {
            #[cfg(target_os = "macos")]
            if let Some(account) = read_keychain("moodle")? {
                return Ok(account);
            }
            read_saved(&data_dir()?)?.ok_or_else(|| "Falta configurar la cuenta de Moodle con --setup.".into())
        }
    }
}

fn read_saved(directory: &Path) -> Result<Option<Credentials>, String> {
    #[cfg(windows)]
    if let Some(account) = read_file(&directory.join("credentials.dpapi.json"), true)? {
        return Ok(Some(account));
    }
    read_file(&directory.join("credentials.json"), false)
}

fn read_file(path: &Path, protected: bool) -> Result<Option<Credentials>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(READ_ERROR.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| READ_ERROR)?;
    if bytes.len() as u64 > MAX_CONFIG {
        return Err(READ_ERROR.into());
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| READ_ERROR)?;
    let url = value
        .get("MOODLE_URL")
        .and_then(|v| v.as_str())
        .ok_or(READ_ERROR)?;
    let token: String = if protected || value.get("MOODLE_TOKEN_DPAPI").is_some() {
        #[cfg(windows)]
        {
            use base64::{Engine, prelude::BASE64_STANDARD};
            let encoded = value
                .get("MOODLE_TOKEN_DPAPI")
                .and_then(|v| v.as_str())
                .ok_or(READ_ERROR)?;
            let bytes = BASE64_STANDARD.decode(encoded).map_err(|_| READ_ERROR)?;
            String::from_utf8(dpapi(&bytes, false)?).map_err(|_| READ_ERROR)?
        }
        #[cfg(not(windows))]
        return Err(READ_ERROR.into());
    } else {
        value
            .get("MOODLE_TOKEN")
            .and_then(|v| v.as_str())
            .ok_or(READ_ERROR)?
            .into()
    };
    Credentials::new(url, &token).map(Some)
}

#[cfg(windows)]
fn dpapi(bytes: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into().map_err(|_| READ_ERROR)?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let description: Vec<u16> = "mudel-ai-connect\0".encode_utf16().collect();
    // DPAPI usa el usuario actual, sin entropía adicional, igual que el instalador Python.
    unsafe {
        let success = if protect {
            CryptProtectData(
                &input,
                description.as_ptr(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if success == 0 {
            return Err(READ_ERROR.into());
        }
        let result = if output.cbData == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        };
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

#[cfg(windows)]
fn save_windows(directory: &Path, account: &Credentials) -> Result<(), String> {
    use base64::{Engine, prelude::BASE64_STANDARD};
    let encrypted = dpapi(account.token.as_bytes(), true).map_err(|_| SAVE_ERROR)?;
    let value = serde_json::json!({
        "MOODLE_URL": account.url, "MOODLE_TOKEN_DPAPI": BASE64_STANDARD.encode(encrypted),
    });
    let bytes = serde_json::to_vec(&value).map_err(|_| SAVE_ERROR)?;
    if bytes.len() as u64 > MAX_CONFIG {
        return Err(SAVE_ERROR.into());
    }
    std::fs::create_dir_all(directory).map_err(|_| SAVE_ERROR)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(|_| SAVE_ERROR)?;
    temporary.write_all(&bytes).map_err(|_| SAVE_ERROR)?;
    temporary.as_file().sync_all().map_err(|_| SAVE_ERROR)?;
    temporary
        .persist(directory.join("credentials.dpapi.json"))
        .map_err(|_| SAVE_ERROR)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn read_keychain(account: &str) -> Result<Option<Credentials>, String> {
    use security_framework::passwords::get_generic_password;
    let bytes = match get_generic_password("mudel-ai-connect", account) {
        Ok(bytes) => bytes,
        Err(error) if error.code() == -25300 => return Ok(None), // errSecItemNotFound
        Err(_) => return Err(READ_ERROR.into()),
    };
    if bytes.len() as u64 > MAX_CONFIG {
        return Err(READ_ERROR.into());
    }
    let value: Credentials = serde_json::from_slice(&bytes).map_err(|_| READ_ERROR)?;
    Credentials::new(&value.url, &value.token).map(Some)
}

#[cfg(target_os = "macos")]
fn save_keychain(account: &str, credentials: &Credentials) -> Result<(), String> {
    let bytes = serde_json::to_vec(credentials).map_err(|_| SAVE_ERROR)?;
    if bytes.len() as u64 > MAX_CONFIG {
        return Err(SAVE_ERROR.into());
    }
    security_framework::passwords::set_generic_password("mudel-ai-connect", account, &bytes)
        .map_err(|_| SAVE_ERROR.into())
}

pub fn setup() -> Result<(), String> {
    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    let mut input = stdin.lock();
    if interactive {
        eprint!("URL HTTPS del campus: ");
        std::io::stderr().flush().map_err(|_| INPUT_ERROR)?;
    }
    let url = read_line(&mut input)?;
    let token = if interactive {
        rpassword::prompt_password("Token de servicios web (oculto): ").map_err(|_| INPUT_ERROR)?
    } else {
        read_line(&mut input)?
    };
    let account = Credentials::new(&url, &token)?;
    #[cfg(windows)]
    save_windows(&data_dir()?, &account)?;
    #[cfg(target_os = "macos")]
    save_keychain("moodle", &account)?;
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = account;
        return Err(SAVE_ERROR.into());
    }
    #[cfg(any(windows, target_os = "macos"))]
    Ok(())
}

fn read_line(input: &mut impl BufRead) -> Result<String, String> {
    let mut line = String::new();
    let count = input
        .take(MAX_CONFIG + 1)
        .read_line(&mut line)
        .map_err(|_| INPUT_ERROR)?;
    if count == 0 || count as u64 > MAX_CONFIG {
        return Err(INPUT_ERROR.into());
    }
    Ok(line)
}

#[cfg(test)]
mod tests;
