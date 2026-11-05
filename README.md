# Mudel AI Connect

Consulta asignaturas, estudia con sus materiales y organiza tareas desde un cliente
de IA compatible con MCP. Está preparado para [SIFO de Comillas](https://sifo.comillas.edu/)
y utiliza los permisos de la cuenta conectada. Las herramientas son de lectura.

Programa local para Windows y plugin con Python 3.11 o superior.
Instalación: [guía del programa](local/README.md) · [guía del plugin](local/PLUGIN.md).

El flujo es: **pregunta → IA + skill → MCP → Moodle → respuesta con fuentes**.
La IA consulta los datos cuando los necesita; no carga todo el campus en cada
conversación ni entrena un modelo con sus contenidos.

## Aviso y privacidad

Mudel AI Connect es un proyecto independiente: **no es oficial** ni está afiliado a
Moodle Pty Ltd ni a ninguna universidad. Moodle es una marca registrada de Moodle Pty Ltd.

Qué sale de tu PC:

- **Tu token de Moodle** solo se envía a tu Moodle. Se guarda cifrado con tu usuario de Windows.
- **Lo que la IA lee de tus asignaturas** pasa por el proveedor de esa IA (Anthropic,
  OpenAI…), como cualquier conversación. Revisa la política de tu universidad antes de
  usarlo con materiales del curso.
- **Al abrirse, el instalador consulta la lista pública de versiones en GitHub** para
  avisarte de actualizaciones. No envía nada tuyo aparte de esa petición.
- No hay telemetría. Si algo falla, **Copiar diagnóstico** te da un texto para pedir ayuda,
  con estados y rutas de tu equipo pero nunca tu token.

Los cambios de cada versión y sus límites conocidos están en [CHANGELOG.md](CHANGELOG.md).

## Programa local

El [programa local](local/README.md) incluye un instalador `.exe` para Windows y
un plugin por `stdio`. El MCP se ejecuta en tu PC y consulta Moodle directamente.
Funciona con Claude Desktop, Claude Code, ChatGPT Work local y Codex.
ChatGPT Work local y Codex comparten configuración y aparecen juntos en una casilla
del instalador. El PC debe estar encendido mientras utilizas la conexión.

El núcleo está en [`shared/`](shared/moodle_mcp.py): el cliente de Moodle y las
seis herramientas. El instalador guarda la clave cifrada en tu perfil de Windows.
Su botón **Permisos** permite elegir si la IA puede consultar asignaturas,
materiales, tareas y calendario; el MCP bloquea las consultas desactivadas.

```text
MCP-MOODLE/
├── shared/    núcleo MCP: moodle_mcp.py, logo y dependencias
├── local/     instalador, plugin, skills y empaquetado
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

- [`mudel-ai-connect-study`](local/skills/mudel-ai-connect-study/SKILL.md): consulta, estudio con fuentes y
  planificación. Se incluye en el plugin local.
- [`setup`](local/skills/setup/SKILL.md): instalación y diagnóstico de la conexión local.

El prompt MCP `estudiar_curso` y las instrucciones del servidor también orientan
la consulta de materiales y las citas para clientes que no cargan skills.

Ejemplos de uso:

- «Consulta Moodle y dime qué asignaturas tengo».
- «Lee los apuntes del tema 2 de Control Avanzado y explícamelos con fuentes».
- «Consulta las tareas y el calendario y ayúdame a planificar la semana».

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

Desde la carpeta del proyecto:

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r local\requirements.txt
.\.venv\Scripts\python.exe run_tests.py
```

`run_tests.py` ejecuta las pruebas de `shared` y `local`. Usan cuentas ficticias
y respuestas simuladas de Moodle. `local/requirements.txt` incluye las dependencias
del núcleo MCP.

GitHub Actions ejecuta las pruebas en Windows con Python 3.11 y comprueba que se
pueda generar el paquete del plugin en cada push y pull request.

## Publicación y licencia

Los instaladores y ZIP se distribuyen mediante GitHub Releases; `dist/` queda fuera
del código versionado. Los pasos están en [RELEASE.md](RELEASE.md).

Este proyecto se distribuye con [licencia MIT](LICENSE). Las dependencias conservan
sus propias licencias.

## Límites

- Solo consulta: no entrega trabajos, envía mensajes ni modifica cursos.
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
