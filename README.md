# Plugin Moodle para tu IA

El proyecto incluye un plugin local que reúne el MCP y las skills `moodle-study`
y `setup`. El paquete instalable es `dist/moodle-plugin-0.1.0.zip`; su guía está
en [`PLUGIN.md`](PLUGIN.md). La clave de tu cuenta queda fuera del paquete.

También incluye un servidor remoto con HTTPS, OAuth y cuentas separadas. Su
instalación y mantenimiento están en [`DEPLOYMENT.md`](DEPLOYMENT.md).

En este equipo, `moodle@moodle-local` ya está instalado y activado en Codex. Se ha
probado el arranque desde la copia instalada, la conexión a SIFO, el listado de
cursos y el cierre del proceso en Windows. Abre una nueva conversación para cargarlo.
El MCP independiente se ha retirado de la configuración y la skill independiente
se conserva como copia de seguridad en el perfil local, fuera del catálogo de skills.

Para regenerar el ZIP y un marketplace local con solo los archivos públicos:

```powershell
.\.venv\Scripts\python.exe scripts/build_plugin.py
```

El marketplace queda en `dist/moodle-marketplace`; su copia del plugin excluye
`.env`, el entorno virtual, las pruebas y los datos privados. Este directorio
es el que se debe registrar como fuente local, en lugar del árbol de desarrollo.

Para preparar el plugin en este equipo reutilizando la cuenta ya configurada:

```powershell
python plugin_runtime.py setup --from-env .env
codex plugin marketplace add "$PWD\dist\moodle-marketplace"
codex plugin add moodle@moodle-local
```

La instalación guarda la configuración de la cuenta y las dependencias en el
perfil local del usuario. Cada persona configura su propia cuenta. Este paquete
es local; la versión remota se instala por separado en un servidor.

Este MCP conecta clientes de IA compatibles con Moodle mediante sus servicios web. Está preparado
para [SIFO de Comillas](https://sifo.comillas.edu/) y consulta con los permisos
de la cuenta asociada al token.

El flujo es: **tu pregunta → IA + skill → MCP → Moodle → respuesta con fuentes**.
La IA busca los datos cuando los necesita. El MCP no entrena un modelo ni carga
todas las asignaturas automáticamente en cada conversación.

## Qué incluye

| Herramienta | Función |
| --- | --- |
| `moodle_status` | Comprobar conexión, usuario y funciones disponibles. |
| `list_courses` | Ver tus asignaturas y sus identificadores. |
| `get_course_context` | Leer secciones, descripciones y listado de materiales, con paginación. |
| `list_assignments` | Consultar enunciados y plazos de tareas. |
| `get_upcoming_events` | Consultar los próximos eventos del calendario. |
| `read_course_file` | Extraer texto de PDF, TXT, MD, CSV y HTML del curso. |

El prompt `estudiar_curso` orienta a la IA para consultar contexto, leer
materiales y citar las fuentes. Las instrucciones del servidor también describen
este flujo para clientes que no utilizan prompts MCP.

## 1. Instalar

Requiere Python 3.11 o superior. En PowerShell, desde esta carpeta:

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

No hace falta activar el entorno virtual. En este equipo ya está instalado.

## 2. Permitir el acceso a SIFO

El **token** es una clave de acceso para aplicaciones. **No es tu contraseña**.
Para esta integración hace falta un token de los **servicios web de Moodle**;
un token RSS, una cookie de navegador o un token de otra API no sirve.

Como alumno o profesor, inicia sesión en SIFO y abre tu menú de usuario,
normalmente en la esquina superior derecha. Busca **Preferencias → Claves de
seguridad**; el nombre puede variar con el idioma y la configuración del campus.
También puedes abrir directamente [la página de claves de SIFO](https://sifo.comillas.edu/user/managetoken.php)
después de iniciar sesión.
La [documentación de Moodle](https://docs.moodle.org/502/en/Security_keys)
explica que esta opción requiere que el centro haya habilitado los permisos
correspondientes. No se ha confirmado que Comillas los permita en tu cuenta.

Si aparece un token para un servicio web autorizado, introdúcelo **solo en el
archivo local `.env`**:

```dotenv
MOODLE_URL=https://sifo.comillas.edu
MOODLE_TOKEN=tu_clave_de_servicios_web
```

Ya existe `.env` con la URL y el token vacío. No lo pegues en el chat ni en el
archivo de configuración del cliente de IA. `.env` está excluido de Git; esta carpeta está
dentro de OneDrive, por lo que ese archivo podría sincronizarse según tus ajustes.
Si prefieres no guardarlo aquí, define `MOODLE_TOKEN` en el entorno del proceso
que ejecuta el servidor y deja el valor de `.env` vacío. Las variables de entorno
tienen prioridad. Codex admite `env_vars = ["MOODLE_TOKEN"]` para reenviarlo.

Si no ves esa opción o no hay servicio disponible, la conexión directa por API
necesita ayuda de soporte de Comillas. Puedes enviarles este texto:

> Quiero consultar mis asignaturas de SIFO desde un cliente MCP local mediante
> los servicios web de Moodle. ¿Están habilitados REST y la obtención de un token
> para mi cuenta? Necesito estas funciones de lectura: core_webservice_get_site_info,
> core_enrol_get_users_courses, core_course_get_contents, mod_assign_get_assignments
> y core_calendar_get_calendar_upcoming_view, además de descarga de archivos del
> servicio. El acceso debe conservar los permisos de mi cuenta.

El programa no necesita tu contraseña ni permisos de administrador. El centro
puede tener que habilitar o autorizar el servicio. La configuración administrativa
se describe en [Using web services](https://docs.moodle.org/502/en/Using_web_services).

## 3. Comprobar la conexión

```powershell
.\.venv\Scripts\python.exe moodle_mcp.py --check
```

Sin token informa de que falta `MOODLE_TOKEN`. Con un token válido muestra el
usuario y las funciones disponibles. Una función ausente tendrá que autorizarse
en el servicio; no se interpreta su ausencia como una asignatura sin materiales.

## 4. Conectar el MCP a tu aplicación

### Codex

En la configuración de MCP de Codex, añade el bloque de
[`codex.config.example.toml`](codex.config.example.toml). Sustituye las rutas
de ejemplo por las de tu equipo. El servidor carga
`.env` desde su propia carpeta, aunque Codex arranque en otra ubicación.

Si utilizas la CLI, también puedes registrarlo desde esta carpeta:

```powershell
codex mcp add moodle -- "$PWD\.venv\Scripts\python.exe" "$PWD\moodle_mcp.py"
codex mcp list
```

Abre una nueva sesión de Codex para cargar la conexión. La
[documentación oficial de OpenAI](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)
describe la configuración de servidores locales por `command` y `args`.
En este equipo se utiliza el MCP integrado en el plugin `moodle@moodle-local`.
Los pasos anteriores son la alternativa para registrar el servidor por separado;
no son necesarios si usas el plugin. Abre una nueva sesión para cargarlo.

Después puedes pedir:

- «Consulta Moodle y dime qué asignaturas tengo».
- «Carga el contexto de Cálculo, lee los apuntes del tema 2 y explícamelos».
- «Consulta las tareas y el calendario de Moodle y ayúdame a planificar la semana».

Para que se consulte al empezar una conversación de estudio, puedes añadir a
las instrucciones de ese proyecto de Codex: «Cuando pregunte por mis asignaturas,
consulta el MCP moodle, carga el contexto del curso y lee los materiales relevantes
antes de responder. Cita los enlaces y avisa si no puedes acceder».

### Claude Desktop y Claude Code

El mismo servidor local sirve para Claude. El bloque de
[`claude.desktop.config.example.json`](claude.desktop.config.example.json)
define su entrada en `mcpServers` para Claude Desktop. Incorpora esa entrada a la
configuración existente conservando los otros servidores; no sustituyas el archivo
completo. Adapta las rutas de ejemplo a tu equipo. El token sigue en `.env`, fuera de
la configuración de Claude.

En Claude Code, puedes registrarlo desde esta carpeta:

```powershell
claude mcp add --transport stdio --scope user moodle -- "$PWD\.venv\Scripts\python.exe" "$PWD\moodle_mcp.py"
```

La conexión local de Claude Desktop es distinta de los conectores remotos que se
añaden a la cuenta de Claude. Consulta la
[documentación de Anthropic](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp).
No se ha modificado ni probado la configuración de Claude en este equipo.

### ChatGPT y Claude en la web

`moodle_remote.py` ofrece las mismas herramientas por Streamable HTTP y exige
OAuth con PKCE. Cada conexión autoriza su propia cuenta Moodle mediante un
formulario HTTPS; las credenciales se cifran y quedan fuera del repositorio.
Consulta [`DEPLOYMENT.md`](DEPLOYMENT.md) para instalarlo en un VPS con Caddy.

El MCP local y el ZIP actual siguen usando `stdio`. La instalación local no
despliega el servidor remoto. El alta de la conexión depende del cliente y de
los permisos de la cuenta: [ChatGPT](https://developers.openai.com/api/docs/guides/custom-mcp-server)
y [Claude](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp).

## 5. Skill reutilizable

La skill está en [`skills/moodle-study/SKILL.md`](skills/moodle-study/SKILL.md).
Contiene el flujo para elegir una asignatura, consultar secciones y archivos,
estudiar con fuentes y planificar tareas respetando los avisos de falta de acceso.
El nombre de invocación es `moodle-study`.

El archivo `dist/moodle-study.zip` contiene únicamente la carpeta de la skill.
No incluye `.env`, credenciales, materiales privados, rutas personales ni el
servidor MCP. Conecta el MCP en la aplicación donde uses la skill; subir el ZIP
no establece por sí mismo una conexión con SIFO.

- En este equipo, `moodle-study` está integrada en el plugin instalado. Abre una
  nueva conversación, selecciona la skill del plugin y pide que consulte Moodle.
  Para instalar únicamente las instrucciones en otro equipo, copia la carpeta
  al directorio de skills de ese cliente y configura primero el MCP. Consulta la
  [documentación oficial de OpenAI](https://learn.chatgpt.com/docs/build-skills).
- En Claude, puedes subir `dist/moodle-study.zip` mediante
  **Customize → Skills → Upload a skill**, si la función está habilitada en tu
  cuenta. La [guía de Anthropic](https://support.claude.com/en/articles/12512180-use-skills-in-claude)
  explica su instalación. La estructura del ZIP sigue su
  [guía de empaquetado](https://support.claude.com/en/articles/12512198-how-to-create-custom-skills).
- El plugin `moodle` reúne las skills y el MCP local. La versión remota se
  instala con la guía de despliegue; la publicación en directorios sigue pendiente.

Ejemplo: «Usa moodle-study para explicarme el tema 2 de Control Avanzado, leyendo
primero los materiales de Moodle y citando las fuentes».

## Comprobaciones y límites

```powershell
.\.venv\Scripts\python.exe -m unittest discover -s tests -v
```

Las pruebas automáticas usan respuestas simuladas de Moodle y un cliente MCP real,
incluyendo el arranque del servidor como proceso por stdio. No necesitan credenciales.
Las 27 pruebas locales incluyen los paquetes sin secretos, la configuración y el
cierre del lanzador. Para ejecutar también las 10 pruebas de autenticación remota,
instala primero `requirements-server.txt`. Los manifiestos locales se han
validado con los esquemas oficiales de Agent Plugins 1.0.0.

El 29 de octubre de 2026 también se verificó la conexión real a SIFO: autenticación,
listado de 35 cursos, consulta de secciones de una asignatura, lectura de HTML y de
un PDF con texto, y calendario. El servicio autoriza las cinco funciones necesarias
y la descarga de archivos. La consulta de tareas de la asignatura probada devolvió
un aviso de falta de acceso a una actividad: ese resultado es parcial y no implica
que no haya tareas.

- Solo hay herramientas de consulta; no hay entregas, mensajes, edición ni
  herramientas para listar alumnos o sus notas. Moodle puede registrar el acceso.
- Se consulta en cada petición; no hay índice local ni memoria persistente.
- Los enunciados y los plazos no indican si has entregado. Las prórrogas personales
  deben verificarse en SIFO. Las fechas se devuelven en UTC para poder convertirlas.
- El calendario devuelve el horizonte configurado por Moodle, no todos los eventos.
- Las descripciones largas se recortan; los archivos se leen en fragmentos de
  12.000 caracteres mediante `offset`. Cada respuesta descargada admite hasta 10 MiB.
- PDF con texto, TXT, MD, CSV y HTML. Sin OCR, DOCX, PPTX, vídeo ni enlaces externos.
  Los adjuntos de enunciados que no aparezcan en `core_course_get_contents` no se
  descargan en esta versión.
- El token se envía por POST y se oculta en la salida. No se siguen redirecciones
  con credenciales; solo se descargan archivos del endpoint de Moodle en el mismo
  servidor. Un despliegue en una red corporativa puede necesitar adaptar el proxy.
- El MCP local funciona por stdio y el servidor remoto por HTTP autenticado.
  Las pruebas desde las cuentas reales de ChatGPT/Claude y la publicación en sus
  directorios requieren completar la conexión en cada cliente.

Implementado con el [SDK oficial de MCP para Python](https://py.sdk.modelcontextprotocol.io/)
y la [API de servicios externos de Moodle](https://moodledev.io/docs/5.0/apis/subsystems/external).
La conexión autenticada a SIFO ya se ha verificado con la clave local.
