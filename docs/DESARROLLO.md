# Guía de desarrollo

Mudel AI Connect es una aplicación Rust para Windows y macOS con Apple Silicon:
servidor MCP, lanzador del plugin, ventana e instalador. Reconoce las cuentas y preferencias
guardadas por versiones anteriores.
Los paquetes siguen siendo experimentales hasta publicarlos firmados.

Las siete herramientas básicas son `moodle_status`, `list_courses`,
`get_course_context`, `list_assignments`, `get_upcoming_events`, `read_course_file`
y `download_course_file`, y el prompt `estudiar_curso`, usando el
[SDK oficial de MCP para Rust](https://github.com/modelcontextprotocol/rust-sdk).
Una muestra de Moodle y sus resultados de referencia (`tests/fixtures/`) comprueban el servidor.
El espaciado del texto extraído de un PDF puede variar según el documento.

## Instalar y usar

Los paquetes están en `dist/native/<sistema-arquitectura>/`, o en los artefactos
`mudel-native-*` del workflow Tests de GitHub Actions:

- Windows x64: abre `Mudel-AI-Connect-Setup.exe`.
- Mac con Apple Silicon: extrae `Mudel-AI-Connect-Setup-macos-aarch64.zip` y abre
  `Mudel AI Connect.app`. Conserva la app para editar permisos o desinstalar después.
  Puedes moverla a tu carpeta de Aplicaciones antes de abrirla.
- El ZIP `mudel-ai-connect-plugin-native-*` contiene el plugin para esa misma plataforma.
  Su README explica la configuración con el lanzador nativo y la cuenta separada.

En la ventana, introduce el campus y el token, marca los clientes y pulsa **Instalar**.
El token se oculta. Si dejas el campo vacío, solo se reutiliza una cuenta guardada
por el programa para ese mismo campus. No hace falta cerrar nada antes: si una app de
IA está usando el servidor, se aparta su copia y la app sigue con ella hasta que abras
una sesión nueva; si Claude Desktop está abierto, avisa al terminar de que debe
cerrarse del todo y abrirse de nuevo para cargar la conexión.

El instalador comprueba Moodle antes de tocar la instalación y vuelve a comprobar
el servidor instalado con la cuenta guardada. Conserva otros servidores y ajustes
de los clientes; crea copias privadas de sus configuraciones. Si falla, restaura
el servidor, las configuraciones y los bytes originales de las credenciales.
Si un bloqueo impide restaurar o limpiar algo, indica dónde quedaron las copias.
No cierra ni mata aplicaciones. Las copias anteriores que siguen en uso se borran en la
siguiente ejecución del instalador.

Detecta Claude Desktop (incluidas las carpetas nuevas de Windows), Claude Code y
ChatGPT Work local / Codex. En Mac, Claude Desktop usa `~/Library/Application Support/Claude`.
El botón **Permisos** edita las cinco categorías y permite elegir la carpeta de
descargas con el diálogo del sistema. **Copiar diagnóstico** no incluye la clave.
Al abrirse, la ventana consulta en segundo plano la lista pública de releases de GitHub
(`src/updates.rs`) y muestra un aviso con enlace si hay una versión más nueva, incluidas las
betas. Solo lee: no descarga ni ejecuta nada, y si no hay red no avisa. Es la única conexión
que hace el programa aparte de las consultas a tu Moodle. **Versiones y actualizaciones** abre
la lista de releases.

**Desinstalar** quita el servidor, las entradas de los clientes y el token del programa.
Conserva la cuenta del plugin y las preferencias. Migra las entradas y el servidor
del nombre anterior `moodle-ai-connect`, conservando su cuenta
del plugin. El token no se anula en Moodle al borrarlo del equipo.

No se requieren privilegios de administrador. Los paquetes de desarrollo no tienen
firma de desarrollador de Windows ni notarización de Apple; la app Mac usa firma local.
Las compilaciones de Mac se dirigen a macOS 11 o superior y se comprueban en CI
con Apple Silicon; falta validar versiones antiguas de macOS.

## Generar el instalador y el plugin

Con Rust y PowerShell 7 (`pwsh`, también en Mac), desde la raíz:

```powershell
./scripts/package.ps1
```

En este equipo Windows sin Visual C++ Build Tools:

```powershell
$env:RUSTFLAGS = '-C link-self-contained=yes -C linker=rust-lld'
./scripts/package.ps1 -Toolchain stable-x86_64-pc-windows-gnu
```

El script compila primero el servidor y después lo incluye en el instalador.
En Windows con MSVC enlaza el runtime de C de forma estática para no exigir
que el usuario instale Visual C++ Redistributable.
Comprueba que el servidor incluido funciona y coincide con el del plugin. Los ZIP
se generan con una lista de archivos públicos; no incluyen entornos ni credenciales.
Cada plataforma recibe su instalador, plugin, licencia, guía y `SHA256SUMS.txt`.
El instalador incluye la licencia MIT del proyecto dentro del servidor instalado.
Las dependencias conservan sus propias licencias.

Para verificar los paquetes ya generados (contenido del plugin, servidor incluido en
el instalador y huellas):

```powershell
./scripts/verify-package.ps1
```

## Compilar y comprobar

Se necesita Rust 1.89 o superior y las herramientas de compilación de la plataforma.
En Windows, la cadena MSVC necesita Visual C++ Build Tools. En este equipo se usa
la cadena GNU con el enlazador incluido en Rust, sin cambiar la cadena predeterminada:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
$env:RUSTFLAGS = '-C link-self-contained=yes -C linker=rust-lld'
cargo +stable-x86_64-pc-windows-gnu build --locked
cargo +stable-x86_64-pc-windows-gnu test --locked
```

Con una cadena MSVC o macOS preparada, basta con los comandos generales.
Desde la raíz del repositorio:

```text
cargo build --locked
cargo test --locked
cargo test --locked --features desktop
```

El binario queda en `target/debug/mudel-ai-connect-server` (`.exe` en Windows).
Sin argumentos habla MCP por `stdio`. Con `--check` devuelve el estado de Moodle en
JSON o termina con código 1 y un error en stderr.
Para crear un binario optimizado, añade `--release` a `cargo build`; queda en `target/release`.

`cargo test` incluye las pruebas del contrato de las herramientas y las de proceso
(`tests/process.rs`), que lanzan el binario real: modo `--check`, cuenta guardada y una
sesión MCP por stdio.
Las pruebas usan datos ficticios y servidores HTTP locales; no consultan un campus real.
GitHub Actions prueba Rust en Windows y Mac con Apple Silicon y guarda sus paquetes.

## Cuenta y permisos

El servidor puede usar una cuenta guardada:

- Windows: lee primero `credentials.dpapi.json` del instalador. Guarda el token con
  [DPAPI para el usuario actual](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata),
  en el formato de versiones anteriores. `--setup` actualiza esta cuenta y conserva aparte
  el archivo `credentials.json` del plugin.
- macOS: guarda juntos la URL y el token en el Llavero, mediante la
  [API nativa](https://docs.rs/security-framework/3.7.0/security_framework/passwords/index.html),
  con servicio `mudel-ai-connect` y cuenta `moodle`. No crea un archivo con el token.
  El sistema puede solicitar permiso para acceder a esa entrada.
- El plugin admite su `credentials.json` existente de versiones anteriores. Solo lo lee; no lo
  elimina ni modifica. El programa usa su propia cuenta y nunca recurre a la del
  plugin. Un almacén corrupto o inaccesible produce un error, sin cambiar de cuenta.

El lanzador del plugin añade `--plugin`: usa `credentials.json` con DPAPI en Windows
y la entrada `plugin` del Llavero en Mac. Puede leer la cuenta de versiones anteriores.
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
./target/debug/mudel-ai-connect-server --setup
./target/debug/mudel-ai-connect-server --check
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

Lee los permisos guardados, en cada consulta:

- Windows: `%LOCALAPPDATA%/mudel-ai-connect/permissions.json`.
- macOS: `$XDG_DATA_HOME/mudel-ai-connect/permissions.json`, o
  `~/.local/share/mudel-ai-connect/permissions.json` si no está definido.

Sin archivo se permiten las consultas, como antes. Un archivo inválido bloquea
las consultas; un permiso desactivado se respeta. `moodle_status` sigue disponible
para diagnóstico. La ruta de macOS se conserva para compartir los permisos y la
carpeta de descargas con las versiones anteriores; el token nuevo se guarda en el Llavero.

Las peticiones son POST al campus HTTPS configurado, sin seguir redirecciones ni
usar proxies del entorno, con 30 segundos de espera y un máximo de 10 MiB por respuesta
de consulta o lectura, y 200 MiB para guardar una descarga en disco.
Solo se admiten las cinco funciones de consulta básicas de Moodle y las cinco opcionales descritas más abajo.
Las respuestas ocultan los tokens y no propagan los mensajes del servidor en errores.

## Archivos

`read_course_file` admite PDF con texto, DOCX, PPTX, XLSX, HTML, TXT, MD y CSV. Devuelve fragmentos de
12.000 caracteres con `next_offset`. Rechaza PDF cifrados o sin texto extraíble;
no incorpora OCR. Los adjuntos deben pertenecer al curso y al mismo campus, por el
endpoint `webservice/pluginfile.php`; el token viaja en el cuerpo POST.

`download_course_file` solo guarda archivos de asignaturas de la cuenta conectada.
Respeta `downloads.json`, guardado por el instalador anterior, y usa la carpeta
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

## Conectar con tu universidad

El botón de la ventana (`src/connect.rs`) obtiene el token iniciando sesión en la página del campus,
como la app oficial de Moodle (`admin/tool/mobile/launch.php`):

1. Genera un `passport` aleatorio de 16 bytes y abre `launch.php?service=moodle_mobile_app&passport=…&urlscheme=mudelai`
   en el navegador.
2. Durante el intento registra `mudelai://` en `HKCU\Software\Classes` (solo el usuario actual) y lo
   elimina al terminar, al cancelar, al agotarse los 5 minutos o al cerrar la ventana.
3. Moodle redirige a `mudelai://token=<base64>`. Windows lanza otra copia de `Mudel-AI-Connect-Setup.exe`
   con esa dirección; esa copia solo deja el texto en `login-callback.txt` (carpeta de datos) y termina.
4. La ventana que espera lo lee, comprueba que `siteid == md5(campus + passport)` (la respuesta
   pertenece a este campus y a este intento, así que una web o un programa ajeno no puede colar un
   token) y que el token es alfanumérico, y lanza la instalación con él. El token lo guarda
   `installer::install`, como cuando se escribe a mano.

Limitaciones: solo Windows (en macOS habría que recibir la dirección por eventos de la app); el
campus debe ofrecer el servicio móvil y aceptar el esquema `mudelai://`; y **no se ha probado de
punta a punta con un inicio de sesión real**, porque requiere las credenciales y el segundo
factor del usuario. Sí están probados el cálculo de la firma, el rechazo de respuestas ajenas, el
registro del esquema y que Windows lance la segunda copia con la dirección (que llega con una «/»
añadida al final, caso que se contempla).

## Herramientas adicionales

Todas son de solo lectura y usan las cinco categorías de permisos que ya existían: no se
añadieron categorías porque el archivo de permisos se valida con exactamente esas cinco.

| Herramienta | Permiso | Función de Moodle |
| --- | --- | --- |
| `read_course_file` con Office | Materiales | `src/office.rs` (DOCX, PPTX y XLSX, que son ZIP con XML) |
| `read_course_files` | Materiales | Una sola consulta de contenidos para hasta 6 archivos |
| `search_course` | Materiales | Filtra `core_course_get_contents`; en todos los cursos consulta de 6 en 6 |
| `get_assignment_status` | Tareas | `mod_assign_get_submission_status` |
| `get_grades` | Tareas | `gradereport_user_get_grade_items` |
| `list_forums`, `get_forum_discussions`, `get_discussion_posts` | Materiales | `mod_forum_get_forums_by_courses`, `mod_forum_get_forum_discussions`, `mod_forum_get_discussion_posts` |

Las cinco funciones de la segunda mitad son **opcionales**: `moodle_status` informa en
`optional_available_functions` y `optional_missing_functions` de cuáles autoriza el campus.

La lectura de Office limita lo que descomprime (64 MiB por parte, 160 MiB en total y 20.000
entradas) para resistir «bombas ZIP». Las fechas de Excel salen como número de serie, porque no
se aplica el formato de la celda. Los libros de Moodle (`mod_book`) añaden `chapter_title` a cada
capítulo en `get_course_context`; no se ha podido probar con un libro real porque el campus de
pruebas no tiene ninguno, solo con datos de ejemplo.

## Validación antes de publicar

Comprobado con un campus real (SIFO de Comillas, Windows): `--check` y las herramientas
`moodle_status`, `list_courses`, `get_course_context`, `list_assignments`,
`get_upcoming_events` y `read_course_file` por una sesión MCP real. Después se comprobaron
la lectura de un `.docx`, un `.pptx` y un `.xlsx` reales, `read_course_files`, `search_course`,
`get_assignment_status`, `get_grades` y los foros (con un foro de avisos con mensajes). Los avisos
«No access rights in module context» de `list_assignments` son respuestas normales de
Moodle para módulos ocultos.

Pendiente:

- Probar `download_course_file`, la instalación completa y una consulta desde cada cliente
  (Claude Desktop, Claude Code, ChatGPT Work local / Codex) en Windows y en Mac.
- Firmar los paquetes de distribución y notarizar la app de macOS. Sin firma, algunos
  antivirus pueden marcar el instalador como sospechoso (detección heurística).

No se ha publicado una release nativa ni se han cambiado las configuraciones reales
de los clientes durante las pruebas de desarrollo.
