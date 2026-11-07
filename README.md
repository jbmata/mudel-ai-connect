# Mudel AI Connect

Consulta asignaturas, estudia con sus materiales y organiza tareas desde un cliente
de IA compatible con MCP. Está preparado para [SIFO de Comillas](https://sifo.comillas.edu/)
y utiliza los permisos de la cuenta conectada. Nada cambia en Moodle: las herramientas
leen, y la de descarga solo guarda archivos en tu carpeta Descargas.

Aplicación nativa para Windows y macOS con Apple Silicon, escrita en Rust y sin
necesidad de Python: ventana de instalación, servidor MCP y plugin.
Las versiones v0.1 y v0.2, escritas en Python, siguen en sus
[releases](https://github.com/jbmata/mudel-ai-connect/releases).

El flujo es: **pregunta → IA + skill → MCP → Moodle → respuesta con fuentes**.
La IA consulta los datos cuando los necesita; no carga todo el campus en cada
conversación ni entrena un modelo con sus contenidos.

## Aviso y privacidad

Mudel AI Connect es un proyecto independiente: **no es oficial** ni está afiliado a
Moodle Pty Ltd ni a ninguna universidad. Moodle es una marca registrada de Moodle Pty Ltd.

Qué sale de tu PC:

- **Tu token de Moodle** solo se envía a tu Moodle. La versión nativa lo guarda en el Llavero
  de macOS o cifrado con tu usuario de Windows
  (DPAPI): no sirve si alguien copia el archivo a otro equipo ni lo pueden leer otros usuarios
  del PC, pero sí un programa que se ejecute con tu usuario. El programa nunca pide ni guarda
  tu contraseña de Moodle.
- **Desinstalar borra el token de tu PC, no lo anula en Moodle.** Si crees que alguien lo ha
  visto, restablécelo en Moodle → Preferencias → Claves de seguridad.
- **Lo que la IA lee de tus asignaturas** pasa por el proveedor de esa IA (Anthropic,
  OpenAI…), como cualquier conversación. Revisa la política de tu universidad antes de
  usarlo con materiales del curso.
- **Avisos de versión nueva:** al abrirse, la ventana consulta la lista pública de
  releases de GitHub para avisarte. GitHub ve tu IP, como en cualquier descarga; no se
  envía nada tuyo y no se descarga nada sin que lo hagas tú.
- No hay telemetría. Si algo falla, **Copiar diagnóstico** te da un texto para pedir ayuda,
  con estados y rutas de tu equipo pero nunca tu token.

Los cambios de cada versión y sus límites conocidos están en [CHANGELOG.md](CHANGELOG.md).

## Instalar

Descarga los paquetes de las [releases](https://github.com/jbmata/mudel-ai-connect/releases)
(o los artefactos `mudel-native-*` del workflow Tests de GitHub Actions):

- **Windows:** abre `Mudel-AI-Connect-Setup.exe`.
- **Mac con Apple Silicon:** extrae `Mudel-AI-Connect-Setup-macos-aarch64.zip` y abre
  `Mudel AI Connect.app`.
- **Plugin:** `mudel-ai-connect-plugin-native-*.zip` contiene el plugin para Codex,
  ChatGPT Work local y Claude Code, con las skills. Su README explica la configuración.

En la ventana indica tu campus y tu token, marca los clientes (Claude Desktop, Claude
Code, ChatGPT Work local / Codex) y pulsa **Instalar**. Su botón **Permisos** permite
elegir si la IA puede consultar asignaturas, materiales, tareas y calendario, y si puede
descargar archivos; el MCP bloquea lo que esté desactivado. El PC debe estar encendido
mientras uses la conexión.

Los paquetes aún **no están firmados**: Windows SmartScreen y algunos antivirus pueden
avisar o marcarlos como sospechosos por heurística. Comprueba la huella con
`SHA256SUMS.txt`. Los pasos para ChatGPT Work local están en [docs/CHATGPT.md](docs/CHATGPT.md).

```text
MCP-MOODLE/
├── src/       servidor MCP, instalador, ventana, credenciales y actualizaciones
├── tests/     pruebas de proceso y datos de referencia
├── plugin/    skills, manifiestos y guía del plugin
├── scripts/   empaquetado y verificación de los paquetes
├── docs/      guía de desarrollo y de ChatGPT Work local
├── assets/    logos
└── dist/      paquetes generados (no se versionan)
```

## Herramientas y skills

| Herramienta | Función |
| --- | --- |
| `moodle_status` | Comprobar la conexión y las funciones disponibles. |
| `list_courses` | Listar las asignaturas y sus identificadores. |
| `get_course_context` | Consultar secciones, descripciones y materiales, con paginación. |
| `list_assignments` | Leer enunciados y plazos de tareas. |
| `get_assignment_status` | Estado de entrega de una tarea: entregada, calificada, nota y comentarios del profesor. |
| `get_grades` | Calificaciones de un curso, con su rango, porcentaje y comentario. |
| `get_upcoming_events` | Consultar los próximos eventos del calendario. |
| `read_course_file` | Extraer texto de PDF, Word (DOCX), PowerPoint (PPTX), Excel (XLSX), TXT, MD, CSV y HTML del curso, incluidas las páginas y los libros de Moodle. |
| `read_course_files` | Leer a la vez hasta 6 archivos de un curso (primer fragmento de cada uno). |
| `search_course` | Buscar un texto en secciones, módulos y nombres de archivo de un curso o de todas tus asignaturas. |
| `list_forums` | Listar los foros de un curso; el de tipo `news` tiene los avisos del profesor. |
| `get_forum_discussions` | Leer las discusiones de un foro con su mensaje inicial. |
| `get_discussion_posts` | Leer los mensajes de una discusión de foro. |
| `download_course_file` | Guardar un archivo del curso, en cualquier formato, en tu carpeta Descargas. |

- [`mudel-ai-connect-study`](plugin/skills/mudel-ai-connect-study/SKILL.md): consulta, estudio con fuentes y
  planificación. Se incluye en el plugin.
- [`setup`](plugin/skills/setup/SKILL.md): instalación y diagnóstico de la conexión local.

El prompt MCP `estudiar_curso` y las instrucciones del servidor también orientan
la consulta de materiales y las citas para clientes que no cargan skills.

Ejemplos de uso:

- «Consulta Moodle y dime qué asignaturas tengo».
- «Lee los apuntes del tema 2 de Control Avanzado y explícamelos con fuentes».
- «Consulta las tareas y el calendario y ayúdame a planificar la semana».
- «Descárgame las diapositivas del tema 3 de Control Avanzado».

## Clave de Moodle

Utiliza un token de servicios web autorizado por tu campus. Inicia sesión y abre
**Tu foto o nombre → Preferencias → Cuenta de usuario → Claves de seguridad**.
Copia el token de servicios web, no el de RSS. En SIFO, el servicio utilizado es
**Moodle mobile web service**. El instalador incluye estos pasos en el desplegable
**Cómo obtener el token**.

Introdúcelo en el instalador local o en el terminal de configuración del plugin.
No lo pegues en el chat ni lo incluyas en un paquete o en el repositorio.
Si el campus no ofrece una clave o una función necesaria, su soporte debe autorizar
el acceso. El MCP utiliza estas funciones:

```text
core_webservice_get_site_info
core_enrol_get_users_courses
core_course_get_contents
mod_assign_get_assignments
core_calendar_get_calendar_upcoming_view
```

También necesita permiso para descargar archivos del servicio.

## Desarrollo y pruebas

Se necesita Rust 1.89 o superior. Desde la raíz del repositorio:

```powershell
cargo test --locked --features desktop
./scripts/package.ps1
./scripts/verify-package.ps1
```

Las pruebas usan cuentas ficticias, respuestas simuladas de Moodle y servidores HTTP
locales; no consultan un campus real. GitHub Actions ejecuta formato, Clippy, pruebas y
empaquetado en Windows y Mac con Apple Silicon en cada push y pull request.
Los detalles (cadena de compilación, cuentas, archivos y validación) están en
[docs/DESARROLLO.md](docs/DESARROLLO.md).

## Publicación y licencia

Los instaladores y ZIP se distribuyen mediante GitHub Releases; `dist/` queda fuera
del código versionado. Los pasos están en [RELEASE.md](RELEASE.md).

Este proyecto se distribuye con [licencia MIT](LICENSE). Las dependencias conservan
sus propias licencias.

## Límites

- No cambia nada en Moodle: no entrega trabajos, envía mensajes ni modifica cursos.
- Las descargas van a `Descargas\Mudel AI Connect\<asignatura>` o a la carpeta que
  elijas en **Permisos**, sin sobrescribir archivos, hasta 200 MiB y sin programas ni
  scripts (`.exe`, `.bat`, `.js`…). La IA no puede elegir la carpeta.
- `list_assignments` no indica si has entregado: usa `get_assignment_status`. Verifica las prórrogas
  personales en Moodle. Las fechas se devuelven en UTC.
- El calendario muestra el horizonte que devuelve Moodle, no todos los eventos.
- Los avisos de falta de permiso indican resultados parciales, no ausencia de tareas.
- Las secciones tienen paginación y los archivos se leen en fragmentos de 12.000
  caracteres. Cada respuesta descargada admite hasta 10 MiB.
- Admite PDF con texto, DOCX, PPTX, XLSX, HTML, TXT, MD y CSV. Sin OCR, `.doc`/`.ppt`/`.xls` antiguos, vídeo ni archivos
  alojados fuera del campus.
- Los adjuntos que no aparezcan en `core_course_get_contents` no se descargan.
- No hay índice local ni memoria persistente de los contenidos consultados.
- Requiere un cliente con ejecución MCP local y los permisos de la cuenta.

Implementado con el [SDK oficial de MCP para Rust](https://github.com/modelcontextprotocol/rust-sdk)
y la [API de servicios externos de Moodle](https://moodledev.io/docs/5.0/apis/subsystems/external).
