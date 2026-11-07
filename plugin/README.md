# Plugin nativo de Mudel AI Connect

Este paquete sirve para la plataforma y arquitectura de su nombre. No necesita
Python. Conserva el directorio completo del plugin; el servidor está en `bin/`.

Instala esta carpeta como plugin local en tu cliente compatible. Incluye los manifiestos de Codex/ChatGPT Work local y Claude Code,
el MCP por stdio y las skills de configuración y estudio. El manifiesto arranca
`bin/__SERVER__ --plugin` y no instala dependencias.

Configura la cuenta desde tu terminal, en la raíz del plugin:

```text
./bin/__SERVER__ --setup --plugin
./bin/__SERVER__ --check --plugin
```

Estos comandos también funcionan en PowerShell en Windows.
El primer comando pide la URL y el token con entrada oculta; no lo escribas en el
historial del shell ni en un chat. No se lee `.env`. Se valida el formato al guardar
y se comprueba la conexión con `--check`.

La cuenta queda en el perfil del usuario, fuera del paquete: DPAPI en Windows y
Llavero en macOS. Es independiente de la cuenta del instalador. Si una versión Python
anterior del plugin ya guardó una cuenta, el lanzador nativo puede leerla. Las preferencias de consultas
y carpeta de descargas se comparten con el programa y se editan en su ventana.

Reinicia la conexión MCP después de cambiar la cuenta. El servidor no modifica
Moodle y no envía el token a la IA; los materiales que consulte la IA sí pasan por
el proveedor de ese cliente. Proyecto independiente, sin afiliación a Moodle ni
a tu universidad. Los paquetes experimentales no tienen firma de desarrollador
ni notarización de Apple; comprueba su procedencia y SHA256SUMS.txt.
