# Migración del servidor a Rust

Servidor experimental en `migration/rust`. El programa y el plugin distribuidos
siguen usando Python; esta carpeta no cambia las instalaciones existentes.

Ya están portadas las siete herramientas `moodle_status`, `list_courses`,
`get_course_context`, `list_assignments`, `get_upcoming_events`, `read_course_file`
y `download_course_file`, y el prompt `estudiar_curso`, usando el
[SDK oficial de MCP para Rust](https://github.com/modelcontextprotocol/rust-sdk).
Conservan los nombres, los parámetros y los resultados estructurados de Python.
Una misma muestra de Moodle y sus resultados de referencia comprueban ambos servidores.
La lectura PDF usa un extractor distinto: el espaciado del texto puede variar.

## Compilar y comprobar

Se necesita Rust 1.88 o superior y las herramientas de compilación de la plataforma.
En Windows, la cadena MSVC necesita Visual C++ Build Tools. En este equipo se usa
la cadena GNU con el enlazador incluido en Rust, sin cambiar la cadena predeterminada:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
$env:RUSTFLAGS = '-C link-self-contained=yes -C linker=rust-lld'
cargo +stable-x86_64-pc-windows-gnu build --manifest-path rust/Cargo.toml --locked
cargo +stable-x86_64-pc-windows-gnu test --manifest-path rust/Cargo.toml --locked
```

Con una cadena MSVC o macOS preparada, basta con los comandos generales.
Desde la raíz del repositorio:

```text
cargo build --manifest-path rust/Cargo.toml --locked
cargo test --manifest-path rust/Cargo.toml --locked
```

El binario queda en `rust/target/debug/mudel-ai-connect-server` (`.exe` en Windows).
Sin argumentos habla MCP por `stdio`. Con `--check` devuelve el estado de Moodle en
JSON o termina con código 1 y un error en stderr, igual que el servidor Python.
Para crear un binario optimizado, añade `--release` a `cargo build`; queda en `target/release`.

Con las dependencias Python del proyecto instaladas, comprueba también el contrato
de las herramientas y la conexión MCP de un cliente Python al proceso Rust:

```powershell
.\.venv\Scripts\python.exe rust\tests\test_contract.py
```

En macOS usa `.venv/bin/python rust/tests/test_contract.py`.
Las pruebas usan datos ficticios y servidores HTTP locales; no consultan un campus real.
GitHub Actions tiene un trabajo Rust para Windows y macOS.

## Cuenta y permisos

Esta etapa recibe **únicamente** `MOODLE_URL` y `MOODLE_TOKEN` del entorno heredado.
No lee `.env`, no guarda claves y no descifra las credenciales del instalador.
No pongas tokens en argumentos, archivos versionados ni conversaciones.
Puede iniciarse y anunciar sus herramientas sin cuenta; las consultas devolverán un error.

Lee los mismos permisos que `local/plugin_runtime.py`, en cada consulta:

- Windows: `%LOCALAPPDATA%/mudel-ai-connect/permissions.json`.
- macOS: `$XDG_DATA_HOME/mudel-ai-connect/permissions.json`, o
  `~/.local/share/mudel-ai-connect/permissions.json` si no está definido.

Sin archivo se permiten las consultas, como en Python. Un archivo inválido bloquea
las consultas; un permiso desactivado se respeta. `moodle_status` sigue disponible
para diagnóstico. La ruta de macOS se conserva provisionalmente para compartir los
permisos con Python, hasta migrar el almacenamiento nativo.

Las peticiones son POST al campus HTTPS configurado, sin seguir redirecciones ni
usar proxies del entorno, con 30 segundos de espera y un máximo de 10 MiB por respuesta
de consulta o lectura, y 200 MiB para guardar una descarga en disco.
Solo se admiten las cinco funciones de consulta de Moodle ya autorizadas en Python.
Las respuestas ocultan los tokens y no propagan los mensajes del servidor en errores.

## Archivos

`read_course_file` admite PDF con texto, HTML, TXT, MD y CSV. Devuelve fragmentos de
12.000 caracteres con `next_offset`. Rechaza PDF cifrados o sin texto extraíble;
no incorpora OCR. Los adjuntos deben pertenecer al curso y al mismo campus, por el
endpoint `webservice/pluginfile.php`; el token viaja en el cuerpo POST.

`download_course_file` solo guarda archivos de asignaturas de la cuenta conectada.
Respeta `downloads.json`, guardado por el instalador Python, y usa la carpeta
Descargas del sistema si no hay elección o si la elegida ya no existe. En Windows
consulta la carpeta conocida del usuario, también si se ha movido a OneDrive.
La IA no puede elegir una ruta. Los nombres se normalizan y las descargas se escriben
en un temporal dentro de la carpeta de destino. La publicación no sobrescribe archivos,
incluso con descargas concurrentes: reutiliza contenido idéntico o añade un número.
Los temporales se eliminan si hay errores.

Se conserva el bloqueo de programas y scripts de Windows y se añaden formatos
ejecutables de macOS, como `.app`, `.command`, `.dmg`, `.pkg` y `.sh`. Los archivos
se marcan con `Zone.Identifier` en Windows y `com.apple.quarantine` en macOS.
Si el sistema de archivos no permite esa marca, el resultado incluye un aviso.
La herramienta no abre ni ejecuta los archivos descargados.

## Lo que falta antes de sustituir Python

- Conservar el acceso a las credenciales DPAPI de Windows y añadir el Llavero de macOS.
- Migrar el lanzador, el instalador y la interfaz; actualizar manifiestos y empaquetado.
- Validar en equipos Windows y Mac y con Moodle real antes de distribuir esta versión.

Esta etapa no es todavía un reemplazo del servidor de producción.
