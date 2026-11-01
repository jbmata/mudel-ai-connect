# Plugin Moodle para tu IA

Consulta asignaturas, estudia con sus materiales y organiza tareas desde un cliente
de IA compatible con MCP. Incluye el conector de Moodle, las skills y el logo.
Está preparado para [SIFO de Comillas](https://sifo.comillas.edu/) y utiliza los
permisos de la cuenta conectada. Las herramientas son de lectura.

El flujo es: **pregunta → IA + skill → MCP → Moodle → respuesta con fuentes**.
La IA consulta los datos cuando los necesita; no carga todo el campus en cada
conversación ni entrena un modelo con sus contenidos.

## Elegir la conexión

| Modalidad | Uso | Guía |
| --- | --- | --- |
| Remota | ChatGPT y Claude mediante HTTPS y OAuth; funciona con el PC apagado. | [Despliegue y mantenimiento](DEPLOYMENT.md) |
| Local | Plugin por `stdio` para Codex y Claude Code; requiere Python 3.11+. | [Instalación del plugin](PLUGIN.md) |

La versión remota vincula cada conexión a su propia cuenta de Moodle y cifra sus
credenciales en el servidor. El plugin local guarda la configuración en el perfil
del usuario, fuera de la carpeta del plugin. Cada guía explica su almacenamiento
y configuración. Instalar un paquete no publica el plugin en un directorio.

## Herramientas y skills

| Herramienta | Función |
| --- | --- |
| `moodle_status` | Comprobar la conexión y las funciones disponibles. |
| `list_courses` | Listar las asignaturas y sus identificadores. |
| `get_course_context` | Consultar secciones, descripciones y materiales, con paginación. |
| `list_assignments` | Leer enunciados y plazos de tareas. |
| `get_upcoming_events` | Consultar los próximos eventos del calendario. |
| `read_course_file` | Extraer texto de PDF, TXT, MD, CSV y HTML del curso. |

- [`moodle-study`](skills/moodle-study/SKILL.md): consulta, estudio con fuentes y
  planificación. Se incluye en ambos paquetes.
- [`setup`](skills/setup/SKILL.md): instalación y diagnóstico de la conexión local.
  Se incluye en el paquete local.

El prompt MCP `estudiar_curso` y las instrucciones del servidor también orientan
la consulta de materiales y las citas para clientes que no cargan skills.

Ejemplos de uso:

- «Consulta Moodle y dime qué asignaturas tengo».
- «Lee los apuntes del tema 2 de Control Avanzado y explícamelos con fuentes».
- «Consulta las tareas y el calendario y ayúdame a planificar la semana».

## Clave de Moodle

Utiliza una clave de servicios web autorizada por tu campus. En SIFO, abre
**Preferencias → Claves de seguridad** o [la página de claves](https://sifo.comillas.edu/user/managetoken.php)
después de iniciar sesión. El servicio utilizado es **Moodle mobile web service**.

Introduce la clave en el formulario HTTPS de la conexión remota o en el instalador
local descrito en `PLUGIN.md`. No la pegues en el chat ni la incluyas en el paquete
o el repositorio. Si el campus no ofrece una clave o una función necesaria, su
soporte debe autorizar el acceso. El MCP utiliza estas funciones:

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
.\.venv\Scripts\python.exe -m pip install -r requirements-server.txt
.\.venv\Scripts\python.exe -m unittest discover -s tests -v
```

`requirements.txt` basta para el plugin local. Las pruebas remotas necesitan
`requirements-server.txt`; utilizan cuentas ficticias y respuestas simuladas de
Moodle. La suite comprueba las herramientas, los paquetes sin secretos, el arranque
por `stdio`, OAuth, PKCE, CSRF, cifrado, revocación y aislamiento de cuentas.

## Generar los paquetes

```powershell
.\.venv\Scripts\python.exe scripts/build_plugin.py
.\.venv\Scripts\python.exe scripts/build_plugin.py --remote-url https://mcp.example.com/mcp
```

Sustituye la URL de ejemplo por la del servidor desplegado. Se generan:

- `dist/moodle-plugin-0.1.0.zip` y el marketplace `dist/moodle-marketplace` para
  instalar el plugin local siguiendo `PLUGIN.md`.
- `dist/moodle-remote-plugin-0.1.0.zip`, con el conector remoto, `moodle-study` y
  el logo. No requiere ejecutar Python en el ordenador del usuario.

Los paquetes se construyen con una lista explícita de archivos públicos. `.env`,
credenciales, datos del servidor, pruebas y temporales quedan fuera de los ZIP.

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
- La instalación, el uso en móvil y la publicación dependen del cliente y de las
  políticas de la cuenta. La configuración del servidor está en `DEPLOYMENT.md`.

Implementado con el [SDK de MCP para Python](https://py.sdk.modelcontextprotocol.io/)
y la [API de servicios externos de Moodle](https://moodledev.io/docs/5.0/apis/subsystems/external).
