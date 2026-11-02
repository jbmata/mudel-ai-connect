# Moodle AI Connect — app local

Todo se ejecuta en tu PC: no hay servidor ni cuenta intermedia. La clave de Moodle
se guarda en tu perfil de usuario y solo se envía a tu Moodle. Lo que la IA lea de
tus apuntes sí pasa por su proveedor, como en cualquier conversación.

Funciona con Claude Desktop, Claude Code, ChatGPT en modo Work local y Codex.
El modo Chat de ChatGPT, también en escritorio, requiere un plugin conectado: el registro
local no lo instala. Para ese modo y Claude web o móvil está la [app online](../server/README.md).

## Instalador para Windows

`dist/Moodle-AI-Connect-Setup.exe` es un único archivo que no necesita Python ni terminal.
Pide la dirección de tu Moodle y tu clave de servicios web, prueba la conexión y
registra el servidor en los clientes que encuentre instalados: Claude Desktop
(también la versión de la Microsoft Store), Claude Code y «ChatGPT Work local / Codex».
ChatGPT Work local y Codex comparten `~/.codex/config.toml`: una sola casilla configura
ambos sin duplicar el servidor. Conserva el resto
de tu configuración y guarda una copia `.moodle-ai-connect-backup` de cada archivo que modifica.
«Desinstalar» quita el servidor, las entradas y la clave, y conserva esas copias.

La ventana muestra el estado real de cada aplicación y lo actualiza sola. Si hay que
cerrar Claude Desktop o alguna aplicación está usando el servidor, lo pregunta antes.
La clave se prueba antes de cambiar nada. Si un error inesperado impide terminar, los
detalles quedan en `%LOCALAPPDATA%\moodle-ai-connect\setup.log`.

Debajo de **Token de Moodle**, el desplegable **Cómo obtener el token** explica los pasos
desde el menú de Moodle, sin abrir el navegador. La disponibilidad de las claves depende
de los permisos que conceda tu centro; consulta la [guía de Moodle](https://docs.moodle.org/500/en/Security_keys).

La clave se cifra con tu usuario de Windows (DPAPI). El `.exe` no está firmado, así
que Windows SmartScreen puede pedir confirmación la primera vez. Si ya tienes el
plugin de Moodle AI Connect instalado en Codex, deja desmarcada la opción compartida para
no duplicar la conexión.

## ChatGPT en modo Work local

Marca «ChatGPT Work local / Codex», instala y reinicia ChatGPT. Selecciona **Work** y
ejecución **Local**, crea una tarea nueva y escribe `/mcp` para comprobar la conexión.
Si no aparece la casilla, abre la app una vez.
Requiere una versión que admita MCP local; no necesita túnel ni clave de API de OpenAI.
La [documentación oficial](https://learn.chatgpt.com/docs/extend/mcp) describe la configuración
compartida. El registro se prueba de forma aislada; falta comprobar una consulta real desde ChatGPT.

El modo Chat requiere una conexión independiente. Sus pasos y límites están en
[`CHATGPT.md`](CHATGPT.md); el instalador no configura ese modo.

## Plugin por `stdio`

Para Codex y Claude Code con Python 3.11+, sigue [`PLUGIN.md`](PLUGIN.md). El ZIP
reúne el MCP y las skills `moodle-ai-connect-study` y `setup`; la clave queda fuera del paquete.

## Contenido

```text
local/
├── moodle_ai_connect_setup.py        ventana del instalador (tkinter)
├── moodle_ai_connect_installer.py    registro en los clientes y prueba de conexión
├── plugin_runtime.py      lanzador del plugin y almacén de la clave (DPAPI)
├── plugin.json, mcp.json, .mcp.json, .claude-plugin/   manifiestos del plugin
├── skills/                moodle-ai-connect-study y setup
├── assets/                imagen del instalador e icono (se generan desde el logo)
├── scripts/               build_plugin.py, build_exe.py, make_assets.py
├── tests/
└── requirements-build.txt PyInstaller y Pillow, solo para compilar
```

El servidor MCP en sí está en [`shared/moodle_mcp.py`](../shared/moodle_mcp.py).

## Generar los paquetes

Desde la raíz del proyecto:

```powershell
.\.venv\Scripts\python.exe -m pip install -r local\requirements-build.txt
.\.venv\Scripts\python.exe local\scripts\make_assets.py
.\.venv\Scripts\python.exe local\scripts\build_exe.py
.\.venv\Scripts\python.exe local\scripts\build_plugin.py
.\.venv\Scripts\python.exe local\scripts\build_plugin.py --remote-url https://mcp.example.com/mcp
```

Sustituye la URL de ejemplo por la del servidor desplegado. Se generan en `dist/`:

- `Moodle-AI-Connect-Setup.exe`: el instalador, con el servidor y Python dentro.
- `moodle-ai-connect-plugin-0.1.0.zip` y el marketplace `moodle-ai-connect-marketplace`, para instalar el
  plugin local siguiendo `PLUGIN.md`.
- `moodle-ai-connect-remote-plugin-0.1.0.zip`, con el conector remoto, `moodle-ai-connect-study` y el logo.
  No requiere ejecutar Python en el ordenador del usuario.

Los ZIP se construyen con una lista explícita de archivos públicos. `.env`,
credenciales, datos del servidor, pruebas y temporales quedan fuera.

## Desarrollo

`local/.env` (ignorado por git) permite probar el servidor sin instalar nada:
`MOODLE_URL` y `MOODLE_TOKEN`. Las pruebas de esta app están en `local/tests`.
