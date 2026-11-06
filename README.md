# Mudel AI Connect

Consulta asignaturas, estudia con sus materiales y organiza tareas desde un cliente
de IA compatible con MCP. Está preparado para [SIFO de Comillas](https://sifo.comillas.edu/)
y utiliza los permisos de la cuenta conectada. Nada cambia en Moodle: las herramientas
leen, y la de descarga solo guarda archivos en tu carpeta Descargas.

Aplicación nativa para Windows y macOS con Apple Silicon en la rama `migration/rust`, con interfaz,
instalador y plugin sin Python: [instalación y compilación](rust/README.md).
La versión Python anterior se conserva: [programa](local/README.md) · [plugin](local/PLUGIN.md).

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
- **Versiones y actualizaciones** abre las releases de GitHub en la versión nativa.
  El instalador Python anterior consulta esa lista automáticamente al abrirse.
- No hay telemetría. Si algo falla, **Copiar diagnóstico** te da un texto para pedir ayuda,
  con estados y rutas de tu equipo pero nunca tu token.

Los cambios de cada versión y sus límites conocidos están en [CHANGELOG.md](CHANGELOG.md).

## Programa local

El [programa local](local/README.md) incluye un instalador `.exe` para Windows y
un plugin por `stdio`. El MCP se ejecuta en tu PC y consulta Moodle directamente.
Funciona con Claude Desktop, Claude Code, ChatGPT Work local y Codex.
ChatGPT Work local y Codex comparten configuración y aparecen juntos en una casilla
del instalador. El PC debe estar encendido mientras utilizas la conexión.

El núcleo nativo está en [`rust/`](rust/README.md); [`shared/`](shared/moodle_mcp.py)
conserva el servidor Python para comprobar la compatibilidad de las siete herramientas.
El instalador guarda la clave en el almacén seguro del sistema.
Su botón **Permisos** permite elegir si la IA puede consultar asignaturas,
materiales, tareas y calendario, y si puede descargar archivos; el MCP bloquea lo
que esté desactivado.

```text
MCP-MOODLE/
├── shared/    núcleo MCP: moodle_mcp.py, logo y dependencias
├── local/     instalador, plugin, skills y empaquetado
├── rust/      aplicación nativa, instalador y plugin para Windows/macOS
├── run_tests.py
└── dist/      paquetes generados (no se versionan)
```

Consulta la [guía de instalación](local/README.md) y los pasos para
[ChatGPT Work local](local/CHATGPT.md).

## Herramientas y skills

| Herramienta | Función |
| --- | --- |
| `moodle_status` | Comprobar la conexión y las funciones disponibles. |
| `list_courses` | Listar las asignaturas y sus identificadores. |
| `get_course_context` | Consultar secciones, descripciones y materiales, con paginación. |
| `list_assignments` | Leer enunciados y plazos de tareas. |
| `get_upcoming_events` | Consultar los próximos eventos del calendario. |
| `read_course_file` | Extraer texto de PDF, TXT, MD, CSV y HTML del curso. |
| `download_course_file` | Guardar un archivo del curso, en cualquier formato, en tu carpeta Descargas. |

- [`mudel-ai-connect-study`](local/skills/mudel-ai-connect-study/SKILL.md): consulta, estudio con fuentes y
  planificación. Se incluye en el plugin local.
- [`setup`](local/skills/setup/SKILL.md): instalación y diagnóstico de la conexión local.

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

La migración experimental del servidor a Rust está en [`rust/`](rust/README.md),
en la rama `migration/rust`. El instalador y el plugin siguen usando Python.

Desde la carpeta del proyecto:

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r local\requirements.txt
.\.venv\Scripts\python.exe run_tests.py
```

`run_tests.py` ejecuta las pruebas de `shared` y `local`. Usan cuentas ficticias
y respuestas simuladas de Moodle. `local/requirements.txt` incluye las dependencias
del núcleo MCP.

GitHub Actions comprueba Python y los paquetes nativos de Windows y
Mac con Apple Silicon en cada push y pull request. Las pruebas Rust y la generación
de paquetes se explican en [rust/README.md](rust/README.md).

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
- Los enunciados y plazos no indican si has entregado. Verifica las prórrogas
  personales en Moodle. Las fechas se devuelven en UTC.
- El calendario muestra el horizonte que devuelve Moodle, no todos los eventos.
- Los avisos de falta de permiso indican resultados parciales, no ausencia de tareas.
- Las secciones tienen paginación y los archivos se leen en fragmentos de 12.000
  caracteres. Cada respuesta descargada admite hasta 10 MiB.
- Admite PDF con texto, HTML, TXT, MD y CSV. Sin OCR, DOCX, PPTX, vídeo ni archivos
  alojados fuera del campus.
- Los adjuntos que no aparezcan en `core_course_get_contents` no se descargan.
- No hay índice local ni memoria persistente de los contenidos consultados.
- Requiere un cliente con ejecución MCP local y los permisos de la cuenta.

Implementado con el [SDK de MCP para Python](https://py.sdk.modelcontextprotocol.io/)
y la [API de servicios externos de Moodle](https://moodledev.io/docs/5.0/apis/subsystems/external).
