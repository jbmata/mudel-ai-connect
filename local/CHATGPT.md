# ChatGPT Work local con Mudel AI Connect

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
   Mudel AI Connect. También puedes verlo en **Settings → MCP servers**.
4. Pregunta: «Consulta Moodle y dime qué asignaturas tengo».

El instalador comprueba el registro y conserva los demás ajustes. Falta verificar una
consulta real desde ChatGPT; un registro correcto no garantiza que tu versión admita MCP local.
El modo Chat no recibe automáticamente este MCP por estar registrado en el equipo.
