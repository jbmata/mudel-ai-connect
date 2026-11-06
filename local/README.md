# Mudel AI Connect — app local

Esta guía describe la versión Python anterior. La aplicación y los paquetes
nativos de Windows y macOS están en la [guía Rust](../rust/README.md).

El MCP se ejecuta en tu PC y consulta Moodle directamente. La clave de Moodle
se guarda en tu perfil de usuario y solo se envía a tu Moodle. Lo que la IA lea de
tus apuntes sí pasa por su proveedor, como en cualquier conversación.

Funciona con Claude Desktop, Claude Code, ChatGPT en modo Work local y Codex.
El modo Chat de ChatGPT, también en escritorio, requiere un plugin conectado: el registro
local no lo instala. Esta versión requiere un cliente con ejecución MCP local.

## Instalador para Windows

`dist/Mudel-AI-Connect-Setup.exe` es un único archivo que no necesita Python ni terminal.
Pide la dirección de tu Moodle y tu clave de servicios web, prueba la conexión y
registra el servidor en los clientes que encuentre instalados: Claude Desktop
(también la versión de la Microsoft Store), Claude Code y «ChatGPT Work local / Codex».
ChatGPT Work local y Codex comparten `~/.codex/config.toml`: una sola casilla configura
ambos sin duplicar el servidor. Conserva el resto
de tu configuración y guarda una copia `.mudel-ai-connect-backup` de cada archivo que modifica.
«Desinstalar» quita el servidor, las entradas y la clave, y conserva esas copias.

La ventana muestra el estado real de cada aplicación y lo actualiza sola. Si hay que
cerrar Claude Desktop o alguna aplicación está usando el servidor, lo pregunta antes.
La clave se prueba antes de cambiar nada. Si un error inesperado impide terminar, los
detalles quedan en `%LOCALAPPDATA%\mudel-ai-connect\setup.log`.

Antes de instalar se comprueban las configuraciones de todos los clientes elegidos.
Si falla una escritura o la verificación final, se recuperan el servidor, la cuenta
y las configuraciones anteriores. Al abrir un instalador más reciente, la ventana
indica **Actualizar**. Si quedan archivos bloqueados al desinstalar, muestra el
error y permite volver a intentarlo; también elimina los archivos de solo lectura.

Debajo de **Token de Moodle**, el desplegable **Cómo obtener el token** explica los pasos
desde el menú de Moodle, sin abrir el navegador. La disponibilidad de las claves depende
de los permisos que conceda tu centro; consulta la [guía de Moodle](https://docs.moodle.org/500/en/Security_keys).

La clave se cifra con tu usuario de Windows (DPAPI). El `.exe` no está firmado, así
que Windows SmartScreen puede pedir confirmación la primera vez. Si ya tienes el
plugin de Mudel AI Connect instalado en Codex, deja desmarcada la opción compartida para
no duplicar la conexión.

El ejecutable usa su clave cifrada y el plugin por Python usa `credentials.json`.
Cambiar la cuenta de uno no sustituye la del otro; ambos comparten las preferencias
de consultas permitidas.

## Consultas permitidas

Abre el instalador y pulsa **Permisos** para activar o desactivar cada grupo y
pulsa **Guardar**. Puedes cambiarlo sin volver a introducir el token ni reinstalar.

| Permiso | Consultas que controla |
| --- | --- |
| Asignaturas | Listar tus cursos con `list_courses`. |
| Materiales y archivos | Consultar temario y recursos con `get_course_context` y leer archivos con `read_course_file`. |
| Tareas | Consultar enunciados y plazos con `list_assignments`. |
| Calendario | Consultar eventos con `get_upcoming_events`. |
| Descargar archivos a tu carpeta Descargas | Guardar archivos del curso con `download_course_file`. |

El estado de conexión (`moodle_status`) sigue disponible para diagnóstico. Todos
los grupos están activados inicialmente para conservar el comportamiento de
las instalaciones existentes. Puedes desactivarlos todos. Si guardaste tus permisos
antes de que existieran las descargas, estas siguen lo que elegiste para los materiales.

Las descargas van a `Descargas\Mudel AI Connect\<asignatura>`. En **Permisos →
Cambiar carpeta…** puedes elegir otra; solo la eliges tú, la IA no puede indicar dónde
se guarda. Si esa carpeta no está (por ejemplo, un USB desconectado), se guarda en
Descargas y la IA te lo dice. No se sobrescribe
ningún archivo: si ya existe uno distinto con el mismo nombre, se guarda como
`nombre (1).pdf`. No se descargan programas ni scripts y el límite es de 200 MiB.

Los permisos se guardan en `%LOCALAPPDATA%\mudel-ai-connect\permissions.json`,
se comparten entre los clientes de IA del mismo usuario y se conservan al
desinstalar. El MCP comprueba el archivo antes de cada consulta y bloquea las
desactivadas antes de contactar con Moodle. Una consulta ya iniciada puede terminar.
Si el archivo no se puede leer o tiene un formato inválido, las consultas se
bloquean hasta que vuelvas a guardar los permisos.

Actualiza el servidor con este instalador para que aplique este control; los
ejecutables y plugins anteriores no lo incluyen. Los límites afectan a este MCP
y no amplían los permisos concedidos por Moodle ni retiran información que la IA
ya haya consultado.

## ChatGPT en modo Work local

Marca «ChatGPT Work local / Codex», instala y reinicia ChatGPT. Selecciona **Work** y
ejecución **Local**, crea una tarea nueva y escribe `/mcp` para comprobar la conexión.
Si no aparece la casilla, abre la app una vez.
Requiere una versión que admita MCP local; no necesita túnel ni clave de API de OpenAI.
La [documentación oficial](https://learn.chatgpt.com/docs/extend/mcp) describe la configuración
compartida. El registro se prueba de forma aislada; falta comprobar una consulta real desde ChatGPT.

Los pasos para ChatGPT Work local están en [`CHATGPT.md`](CHATGPT.md).

## Plugin por `stdio`

Para Codex y Claude Code con Python 3.11+, sigue [`PLUGIN.md`](PLUGIN.md). El ZIP
reúne el MCP y las skills `mudel-ai-connect-study` y `setup`; la clave queda fuera del paquete.

## Contenido

```text
local/
├── mudel_ai_connect_setup.py        ventana del instalador (tkinter)
├── mudel_ai_connect_installer.py    registro en los clientes y prueba de conexión
├── plugin_runtime.py      lanzador del plugin y almacén de la clave (DPAPI)
├── plugin.json, mcp.json, .mcp.json, .claude-plugin/   manifiestos del plugin
├── skills/                mudel-ai-connect-study y setup
├── assets/                imagen del instalador e icono (se generan desde el logo)
├── scripts/               build_plugin.py, build_exe.py, make_assets.py
├── tests/
└── requirements-build.txt PyInstaller y Pillow, solo para compilar
```

El servidor MCP en sí está en [`shared/moodle_mcp.py`](../shared/moodle_mcp.py).

## Generar los paquetes

Desde la raíz del proyecto:

```powershell
.\.venv\Scripts\python.exe -m pip install -r local\requirements.txt
.\.venv\Scripts\python.exe -m pip install -r local\requirements-build.txt
.\.venv\Scripts\python.exe local\scripts\make_assets.py
.\.venv\Scripts\python.exe local\scripts\build_exe.py
.\.venv\Scripts\python.exe local\scripts\build_plugin.py
```

Se generan en `dist/`:

- `Mudel-AI-Connect-Setup.exe`: el instalador, con el servidor y Python dentro.
- `mudel-ai-connect-plugin-0.2.0.zip` y el marketplace `mudel-ai-connect-marketplace`, para instalar el
  plugin local siguiendo `PLUGIN.md`.

Los ZIP se construyen con una lista explícita de archivos públicos. `.env`,
credenciales, pruebas y temporales quedan fuera.

Los paquetes incluyen la licencia MIT del proyecto. Para distribuirlos, sigue la
[guía de publicación](../RELEASE.md); los binarios se adjuntan a GitHub Releases.

## Desarrollo

`local/.env` (ignorado por git) permite probar el servidor sin instalar nada:
`MOODLE_URL` y `MOODLE_TOKEN`. Las pruebas de esta app están en `local/tests`.
