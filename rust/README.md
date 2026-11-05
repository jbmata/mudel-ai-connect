# Migración del servidor a Rust

Primera etapa experimental en `migration/rust`. El programa y el plugin distribuidos
siguen usando Python; esta carpeta no cambia las instalaciones existentes.

Ya están portadas las cinco consultas `moodle_status`, `list_courses`,
`get_course_context`, `list_assignments` y `get_upcoming_events`, usando el
[SDK oficial de MCP para Rust](https://github.com/modelcontextprotocol/rust-sdk).
Conservan los nombres, los parámetros y los resultados estructurados de Python.
Una misma muestra de Moodle y sus resultados de referencia comprueban ambos servidores.

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
usar proxies del entorno, con 30 segundos de espera y un máximo de 10 MiB por respuesta.
Solo se admiten las cinco funciones de consulta de Moodle ya autorizadas en Python.
Las respuestas ocultan los tokens y no propagan los mensajes del servidor en errores.

## Lo que falta antes de sustituir Python

- Portar `read_course_file`, `download_course_file` y el prompt `estudiar_curso`.
- Conservar el acceso a las credenciales DPAPI de Windows y añadir el Llavero de macOS.
- Migrar el lanzador, el instalador y la interfaz; actualizar manifiestos y empaquetado.
- Validar en equipos Windows y Mac y con Moodle real antes de distribuir esta versión.

Esta etapa no es todavía un reemplazo del servidor de producción.
