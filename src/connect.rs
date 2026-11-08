//! «Conectar con tu universidad»: obtiene el token iniciando sesión en la página oficial
//! del campus, sin que Mudel vea nunca la contraseña ni el segundo factor.
//!
//! Es el mismo mecanismo que usa la app oficial de Moodle (`admin/tool/mobile/launch.php`):
//! 1. Mudel abre un receptor en `127.0.0.1` (solo accesible desde este equipo), genera un
//!    `passport` aleatorio y abre `launch.php` en el navegador.
//! 2. El usuario inicia sesión allí (SSO, doble factor…).
//! 3. Moodle redirige a `<urlscheme>://token=<base64>`. Como `urlscheme` es la dirección del
//!    receptor local, el navegador hace esa petición a Mudel (los navegadores bloquean en
//!    silencio las redirecciones a programas externos, pero no las que van a `127.0.0.1`).
//! 4. Mudel comprueba que la respuesta pertenece a este campus y a este intento (con el
//!    `passport`) y se queda con el token.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use base64::{Engine, prelude::BASE64_STANDARD};
use md5::{Digest, Md5};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

use crate::moodle::validate_account;

const SCHEME: &str = "mudelai";
const PATH: &str = "/mudel-login";
const MAX_REQUEST_LINE: u64 = 8192;
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

    /// Dirección que se abre en el navegador; `redirect` es la del receptor local.
    pub fn launch_url(&self, redirect: &str) -> String {
        format!(
            "{}/admin/tool/mobile/launch.php?service=moodle_mobile_app&passport={}&urlscheme={}",
            self.base_url,
            self.passport,
            utf8_percent_encode(redirect, NON_ALPHANUMERIC)
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
        let decoded = BASE64_STANDARD
            .decode(payload.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
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

/// Receptor local de la respuesta del navegador. Deja de escuchar al soltarlo.
pub struct Receiver {
    port: u16,
    answers: mpsc::Receiver<String>,
    stop: Arc<AtomicBool>,
}

impl Receiver {
    pub fn start() -> Result<Self, String> {
        let unavailable = || "No se pudo preparar la conexión con el navegador en este equipo.";
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| unavailable())?;
        listener.set_nonblocking(true).map_err(|_| unavailable())?;
        let port = listener.local_addr().map_err(|_| unavailable())?.port();
        let (sender, answers) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        std::thread::spawn(move || serve(&listener, &sender, &flag));
        Ok(Self {
            port,
            answers,
            stop,
        })
    }

    /// Valor de `urlscheme`: Moodle le añade `://token=<respuesta>`.
    pub fn redirect(&self) -> String {
        format!("http://127.0.0.1:{}{PATH}?", self.port)
    }

    /// La dirección recibida del navegador, si ya llegó (con la forma `mudelai://token=…`).
    pub fn try_answer(&self) -> Option<String> {
        self.answers.try_recv().ok()
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(listener: &TcpListener, sender: &mpsc::Sender<String>, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(answer) = handle(stream) {
                    let _ = sender.send(answer);
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Atiende una petición; devuelve la respuesta del campus solo si lo es.
fn handle(mut stream: TcpStream) -> Option<String> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;
    let mut line = String::new();
    BufReader::new((&stream).take(MAX_REQUEST_LINE))
        .read_line(&mut line)
        .ok()?;
    let answer = answer_in(&line);
    let reply = if answer.is_some() {
        page()
    } else {
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
    };
    let _ = stream.write_all(reply.as_bytes());
    answer
}

/// Extrae `mudelai://token=…` de la primera línea de una petición `GET`.
fn answer_in(request_line: &str) -> Option<String> {
    let mut words = request_line.split_whitespace();
    let (method, target) = (words.next()?, words.next()?);
    let query = target.strip_prefix(PATH)?.strip_prefix('?')?;
    let payload = query.strip_prefix("://token=")?;
    (method == "GET" && !payload.is_empty()).then(|| format!("{SCHEME}://token={payload}"))
}

/// Página que ve el usuario en el navegador al terminar. Quita el token de la barra de direcciones.
fn page() -> String {
    let body = "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\">\
<title>Mudel AI Connect</title><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<style>body{font:16px system-ui,sans-serif;background:#f4f4f6;color:#222;display:grid;place-items:center;height:100vh;margin:0}\
main{background:#fff;border:1px solid #ddd;border-radius:8px;padding:28px 36px;text-align:center}\
h1{font-size:20px;margin:0 0 8px}p{margin:0;color:#555}</style></head><body><main>\
<h1>Listo</h1><p>Ya puedes cerrar esta pestaña y volver a Mudel AI Connect.</p></main>\
<script>history.replaceState(null,'','/')</script></body></html>";
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests;
