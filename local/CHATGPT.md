# ChatGPT con Moodle AI Connect

## ChatGPT Work local: conexión local

La [documentación oficial de OpenAI](https://learn.chatgpt.com/docs/extend/mcp) indica que
ChatGPT de escritorio y Codex comparten `~/.codex/config.toml` para el mismo equipo.
Esta opción usa la ejecución local de **Work**, no el modo Chat. El instalador los muestra
juntos como **«ChatGPT Work local / Codex»** y registra el
servidor una sola vez. No necesitas túnel ni clave de API de OpenAI para esta opción.

1. Abre ChatGPT de escritorio una vez. Requiere una versión que admita **Work local** y MCP.
2. En el instalador, marca **«ChatGPT Work local / Codex»**, introduce tu campus y tu
   token de Moodle y pulsa **Instalar**.
3. Reinicia ChatGPT, selecciona **Work** y ejecución **Local**, y crea una tarea nueva.
   Escribe `/mcp` para comprobar que aparece
   Moodle AI Connect. También puedes verlo en **Settings → MCP servers**.
4. Pregunta: «Consulta Moodle y dime qué asignaturas tengo».

El instalador comprueba el registro y conserva los demás ajustes. Falta verificar una
consulta real desde ChatGPT; un registro correcto no garantiza que tu versión admita MCP local.
El modo Chat no recibe automáticamente este MCP por estar registrado en el equipo.

## ChatGPT en modo Chat: conectar un plugin

Esto se aplica al modo Chat de la app de escritorio y a ChatGPT web. El registro local
del instalador no instala un plugin en ese modo. Según la
[documentación de OpenAI](https://learn.chatgpt.com/docs/extend/mcp), las herramientas
de los chats alojados se conectan mediante plugins.

Si tienes desplegada la [app online](../server/README.md), abre **ChatGPT Plugins → + →
Add custom MCP server**, introduce su URL HTTPS con `/mcp`, configura la autenticación
e instala el plugin. Completa la conexión con tu Moodle y selecciónalo con `@` en una
conversación nueva. Comprueba `moodle_status` y `list_courses` antes de dar el acceso por
verificado. El instalador no realiza ese alta por ti.

## ChatGPT web con el Secure MCP Tunnel (experimental)

**Estado: experimental.** Está montado según la [documentación de OpenAI](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels),
pero no se ha probado con una cuenta real. Si algo no coincide, manda lo que diga
`tunnel-client doctor --explain`, sin tu clave.

### Por qué hace falta en la web

ChatGPT web no puede lanzar programas de tu PC. Solo se conecta a un servidor al que le llegue
una petición por internet. El túnel de OpenAI resuelve eso sin VPS ni puertos abiertos:
`tunnel-client` corre en tu PC, abre una conexión **de salida** hacia OpenAI, recibe las
peticiones de ChatGPT y las pasa a tu servidor por `stdio`, el mismo que usan Claude y Codex.

### Límites del túnel

- **El PC tiene que estar encendido** y `tunnel-client` en marcha mientras uses ChatGPT.
- **Solo para ti.** OpenAI avisa de que no sirve para publicar ni repartir el plugin.
- **Sin móvil con el PC apagado.** Para eso, la [app online](../server/README.md).
- Límites del túnel: 120 s por petición, 10 MiB por mensaje y 600 peticiones por minuto.
- Lo que ChatGPT lea de tus apuntes pasa por OpenAI, como en cualquier conversación. La
  clave de Moodle sigue cifrada en tu PC.

### Lo que OpenAI no aclara

Estas tres cosas hay que comprobarlas en tu caso:

1. **Si `tunnel-client` tiene versión para Windows.** La documentación no menciona ningún
   sistema operativo. Mira los archivos de la [última versión](https://github.com/openai/tunnel-client/releases/latest).
   Si no hay para Windows, este camino no es posible: usa la app online.
2. **Qué plan de ChatGPT lo admite.** La documentación habla de permisos del espacio de
   trabajo y no nombra ningún plan.
3. **Cómo se entrecomilla `--mcp-command` si la ruta tiene espacios.** La ruta por defecto
   (`C:/Users/<tú>/AppData/Local/moodle-ai-connect/...`) no los tiene salvo que tu usuario
   de Windows los lleve.

### Pasos para ChatGPT web

1. **Instala Moodle AI Connect** con el instalador. Los comandos usan el servidor que instala.
2. **Descarga `tunnel-client`** y ponlo en una carpeta de tu PATH, o llámalo con su ruta.
3. **Crea el túnel** en [OpenAI Platform → Túneles](https://platform.openai.com/settings/organization/tunnels)
   y copia su `tunnel_id`. Tu usuario necesita los permisos de túneles **Read**, **Manage**
   y **Use**; un administrador de la organización los concede y pueden tardar hasta 30
   minutos. Necesitas además una clave de API de OpenAI Platform.
4. **En PowerShell**, usa estos comandos con tus datos. Sustituye `TU_USUARIO` por tu usuario
   de Windows para indicar la ruta de instalación:

   ```powershell
   $env:CONTROL_PLANE_API_KEY = "sk-..."
   tunnel-client init --sample sample_mcp_stdio_local --profile moodle-ai-connect --tunnel-id TU_TUNNEL_ID --mcp-command "C:/Users/TU_USUARIO/AppData/Local/moodle-ai-connect/bin/moodle-ai-connect-server/moodle-ai-connect-server.exe"
   tunnel-client doctor --profile moodle-ai-connect --explain
   tunnel-client run --profile moodle-ai-connect
   ```

   `doctor` comprueba la configuración y la conexión. Si falla, no sigas: arréglalo antes.
5. **En ChatGPT**, abre [Plugins](https://chatgpt.com/plugins) → botón **+** → **Add custom
   MCP server** → **Connection: Tunnel**. Elige tu túnel, acepta el aviso de riesgo y pulsa
   **Create as a plugin**. Deja `tunnel-client run` abierto mientras lo uses.
6. **Pruébalo** en una conversación nueva: «Consulta Moodle y dime qué asignaturas tengo».

### Seguridad del túnel

- La clave de API de OpenAI es **distinta** de la de Moodle. Escríbela solo en tu PowerShell:
  nunca en el chat, en el instalador ni en un archivo del proyecto.
- **Quien use tu ChatGPT podrá consultar tu Moodle** con tus permisos mientras el túnel
  esté activo. No lo montes en un PC compartido.
- Para cortar el acceso, detén `tunnel-client`. El túnel y sus permisos se gestionan en
  OpenAI Platform.
