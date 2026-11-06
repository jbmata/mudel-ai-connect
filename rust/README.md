# Mudel AI Connect nativo

Aplicación Rust para Windows y macOS, en `migration/rust`: servidor MCP, lanzador
del plugin, ventana e instalador. Los paquetes nativos no necesitan Python.
La implementación Python se conserva como referencia y para comprobar compatibilidad.
Los paquetes siguen siendo experimentales hasta probarlos con un campus real.

Ya están portadas las siete herramientas `moodle_status`, `list_courses`,
`get_course_context`, `list_assignments`, `get_upcoming_events`, `read_course_file`
y `download_course_file`, y el prompt `estudiar_curso`, usando el
[SDK oficial de MCP para Rust](https://github.com/modelcontextprotocol/rust-sdk).
Conservan los nombres, los parámetros y los resultados estructurados de Python.
Una misma muestra de Moodle y sus resultados de referencia comprueban ambos servidores.
La lectura PDF usa un extractor distinto: el espaciado del texto puede variar.

## Instalar y usar

Los paquetes están en `dist/native/<sistema-arquitectura>/`, o en los artefactos
`mudel-native-*` del workflow Tests de GitHub Actions:

- Windows x64: abre `Mudel-AI-Connect-Setup.exe`.
- Mac Intel o Apple Silicon: extrae el ZIP de tu arquitectura y abre
  `Mudel AI Connect.app`. Conserva la app para editar permisos o desinstalar después.
  Puedes moverla a tu carpeta de Aplicaciones antes de abrirla.
- El ZIP `mudel-ai-connect-plugin-native-*` contiene el plugin para esa misma plataforma.
  Su README explica la configuración con el lanzador nativo y la cuenta separada.

En la ventana, introduce el campus y el token, marca los clientes y pulsa **Instalar**.
El token se oculta. Si dejas el campo vacío, solo se reutiliza una cuenta guardada
por el programa para ese mismo campus. Cierra las aplicaciones de IA cuando se
solicite y abre una sesión nueva al terminar.

El instalador comprueba Moodle antes de tocar la instalación y vuelve a comprobar
el servidor instalado con la cuenta guardada. Conserva otros servidores y ajustes
de los clientes; crea copias privadas de sus configuraciones. Si falla, restaura
el servidor, las configuraciones y los bytes originales de las credenciales.
Si un bloqueo impide restaurar o limpiar algo, indica dónde quedaron las copias.
No cierra ni mata aplicaciones automáticamente.

Detecta Claude Desktop (incluidas las carpetas nuevas de Windows), Claude Code y
ChatGPT Work local / Codex. En Mac, Claude Desktop usa `~/Library/Application Support/Claude`.
El botón **Permisos** edita las cinco categorías y permite elegir la carpeta de
descargas con el diálogo del sistema. **Copiar diagnóstico** no incluye la clave.
**Versiones y actualizaciones** abre la lista de releases; no hay consulta automática.

**Desinstalar** quita el servidor, las entradas de los clientes y el token del programa.
Conserva la cuenta del plugin y las preferencias. Migra las entradas y el servidor
del nombre anterior `moodle-ai-connect`, conservando su entorno Python y su cuenta
del plugin. El token no se anula en Moodle al borrarlo del equipo.

No se requieren privilegios de administrador. Los paquetes de desarrollo no tienen
firma de desarrollador de Windows ni notarización de Apple; la app Mac usa firma local.
Las compilaciones de Mac se dirigen a macOS 11 o superior y se comprueban en CI
con equipos Intel y Apple Silicon; falta validar versiones antiguas de macOS.

## Generar el instalador y el plugin

Con Rust y PowerShell 7 (`pwsh`, también en Mac), desde la raíz:

```powershell
./rust/scripts/package.ps1
```

En este equipo Windows sin Visual C++ Build Tools:

```powershell
$env:RUSTFLAGS = '-C link-self-contained=yes -C linker=rust-lld'
./rust/scripts/package.ps1 -Toolchain stable-x86_64-pc-windows-gnu
```

El script compila primero el servidor y después lo incluye en el instalador.
Comprueba que el servidor incluido funciona y coincide con el del plugin. Los ZIP
se generan con una lista de archivos públicos; no incluyen entornos ni credenciales.
Cada plataforma recibe su instalador, plugin, licencia, guía y `SHA256SUMS.txt`.
El instalador incluye la licencia MIT del proyecto dentro del servidor instalado.
Las dependencias conservan sus propias licencias.

Para verificar los paquetes ya generados:

```text
python rust/tests/test_package.py
```

Python se usa aquí para las pruebas de desarrollo, no para ejecutar la aplicación.

## Compilar y comprobar

Se necesita Rust 1.89 o superior y las herramientas de compilación de la plataforma.
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
cargo test --manifest-path rust/Cargo.toml --locked --features desktop
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
GitHub Actions prueba Rust en Windows, Mac Intel y Mac Apple Silicon y guarda sus paquetes.

## Cuenta y permisos

El servidor puede usar una cuenta guardada sin Python:

- Windows: lee primero `credentials.dpapi.json` del instalador. Guarda el token con
  [DPAPI para el usuario actual](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata),
  en el mismo formato que Python. `--setup` actualiza esta cuenta y conserva aparte
  el archivo `credentials.json` del plugin.
- macOS: guarda juntos la URL y el token en el Llavero, mediante la
  [API nativa](https://docs.rs/security-framework/3.7.0/security_framework/passwords/index.html),
  con servicio `mudel-ai-connect` y cuenta `moodle`. No crea un archivo con el token.
  El sistema puede solicitar permiso para acceder a esa entrada.
- Si no hay cuenta nativa, admite el `credentials.json` existente de Python.
  Solo lo lee; no lo elimina ni modifica. Un almacén nativo corrupto o inaccesible
  produce un error, sin cambiar a otra cuenta.

El lanzador del plugin añade `--plugin`: usa `credentials.json` con DPAPI en Windows
y la entrada `plugin` del Llavero en Mac. Puede leer la cuenta previa de Python.
La cuenta `moodle` del instalador permanece separada. Para configurar o comprobar
el plugin, añade `--plugin` a `--setup` o `--check`. Sus manifiestos ya lo incluyen
en el arranque MCP; no crean entornos ni instalan dependencias.

Para configurar la cuenta, ejecuta el binario en un terminal:

```powershell
# Windows, desde la raíz del repositorio
.\rust\target\debug\mudel-ai-connect-server.exe --setup
.\rust\target\debug\mudel-ai-connect-server.exe --check
```

```sh
# macOS
./rust/target/debug/mudel-ai-connect-server --setup
./rust/target/debug/mudel-ai-connect-server --check
```

`--setup` pide la URL y después el token con entrada oculta. Valida su formato y
guarda la cuenta; `--check` comprueba después la conexión con Moodle. Para procesos
que ya manejan secretos, también admite dos líneas por stdin: URL y token.
No pongas tokens en argumentos, historial del shell, archivos versionados ni conversaciones.

`MOODLE_URL` y `MOODLE_TOKEN` heredadas tienen prioridad sobre la cuenta guardada,
incluso si están vacías. Deben definirse juntas; una variable aislada produce un error
para evitar enviar el token guardado a otro campus. Elimina ambas del entorno para
comprobar la cuenta guardada. No se leen archivos `.env`.
Puede anunciar sus herramientas sin cuenta; las consultas devolverán un error.

Lee los mismos permisos que `local/plugin_runtime.py`, en cada consulta:

- Windows: `%LOCALAPPDATA%/mudel-ai-connect/permissions.json`.
- macOS: `$XDG_DATA_HOME/mudel-ai-connect/permissions.json`, o
  `~/.local/share/mudel-ai-connect/permissions.json` si no está definido.

Sin archivo se permiten las consultas, como en Python. Un archivo inválido bloquea
las consultas; un permiso desactivado se respeta. `moodle_status` sigue disponible
para diagnóstico. La ruta de macOS se conserva para compartir los permisos y la
carpeta de descargas con Python; el token nuevo se guarda en el Llavero.

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

## Validación antes de publicar

- Probar la instalación completa y una consulta real desde cada cliente en equipos Windows y Mac.
- Firmar los paquetes de distribución y notarizar la app de macOS.

No se ha publicado una release nativa ni se han cambiado las configuraciones reales
de los clientes durante las pruebas de desarrollo.
