# Mudel AI Connect 0.1.0

Mudel AI Connect reúne el conector MCP de Moodle y las skills `mudel-ai-connect-study`
(consulta, estudio y organización con fuentes) y `setup` (configuración de la cuenta).

Permite consultar asignaturas, secciones, archivos, tareas y calendario con los
permisos de tu cuenta. No modifica cursos ni entrega trabajos. Necesita Python
3.11 o superior disponible como `python` y acceso a los servicios web de tu Moodle.

## Preparar la cuenta y el entorno

Extrae el ZIP en una carpeta local. Abre un terminal en esa carpeta y ejecuta:

```text
python plugin_runtime.py setup
```

El instalador prepara las dependencias en tu perfil local y pide la URL de Moodle
y su clave de servicios web. La clave se escribe de forma oculta en el terminal;
no la pegues en el chat. En Windows se guarda cifrada con tu usuario (DPAPI); en Linux
y macOS, en texto con permisos solo para tu usuario. El archivo está en tu perfil:

- Windows: `%LOCALAPPDATA%\mudel-ai-connect\credentials.json`.
- Linux/macOS: `$XDG_DATA_HOME/mudel-ai-connect/credentials.json`, o
  `~/.local/share/mudel-ai-connect/credentials.json` si esa variable no está definida.

El ZIP no contiene credenciales. Comparte únicamente el ZIP generado; evita
compartir el archivo de configuración del usuario. Para importar una configuración
que ya tengas en tu equipo:

```text
python plugin_runtime.py setup --from-env /ruta/a/tu/.env
```

Cada persona que instale el plugin debe configurar su propia cuenta en su equipo.
El plugin usa su archivo `credentials.json`, aunque exista una clave cifrada del
instalador de Windows. Las cuentas de las dos instalaciones se configuran por separado.
El plugin actualizado respeta las consultas permitidas que hayas guardado desde
el botón **Permisos** del instalador de Windows. La selección se comparte entre
los clientes del mismo usuario; el estado de conexión sigue disponible.

Para comprobar el acceso:

```text
python plugin_runtime.py run --check
```

El entorno de Python está fuera de la carpeta del plugin y se reutiliza aunque
el cliente copie o actualice el paquete. El arranque normal no instala dependencias.

## Claude Code

El paquete incluye `.claude-plugin/plugin.json`, las skills y el MCP en `.mcp.json`.
Después de ejecutar `setup`, puedes probarlo desde un terminal:

```text
claude --plugin-dir /ruta/a/la/carpeta/del/plugin
```

Prueba una petición como «Usa mudel-ai-connect-study para listar mis asignaturas y cargar el
temario de una de ellas». La forma de invocación concreta puede llevar el prefijo
del plugin, por ejemplo `moodle:mudel-ai-connect-study`.

## Codex y clientes OpenAI locales

La raíz contiene `plugin.json` y `mcp.json` en formato Agent Plugins. Añade el
directorio del plugin a un marketplace local y después instálalo desde ese
marketplace. Un catálogo mínimo apunta al directorio del plugin:

```json
{
  "name": "mudel-ai-connect-local",
  "interface": {"displayName": "Mudel AI Connect"},
  "plugins": [{
    "name": "mudel-ai-connect",
    "source": {"source": "local", "path": "./plugins/mudel-ai-connect"},
    "policy": {"installation": "AVAILABLE", "authentication": "ON_INSTALL"},
    "category": "Productivity"
  }]
}
```

Guarda el catálogo en `.agents/plugins/marketplace.json` dentro de una carpeta de
marketplace y el plugin en `plugins/mudel-ai-connect` relativo a esa carpeta. Regístralo con:

```text
codex plugin marketplace add /ruta/al/marketplace
codex plugin add mudel-ai-connect@mudel-ai-connect-local
```

Abre una conversación nueva y prueba las herramientas del plugin. Si ya habías
añadido el MCP y la skill por separado, puedes ver dos conexiones o skills hasta
desactivar las entradas anteriores. El paquete no elimina configuraciones previas.

## Clientes compatibles

El plugin ejecuta el MCP localmente por `stdio`. Requiere un cliente que permita
ejecutar programas en tu equipo.

La publicación en los directorios de cada proveedor es un paso distinto de la
creación del paquete. La instalación y el uso de componentes locales dependen
del cliente y de las políticas de la cuenta. Documentación de referencia:

- [Paquetes de plugins de OpenAI](https://developers.openai.com/plugins/build/plugins).
- [Plugins locales de OpenAI](https://developers.openai.com/plugins/deploy/desktop-only).
- [Manifiesto de plugins de Claude](https://code.claude.com/docs/en/plugins-reference).

## Límites de lectura

La IA consulta los datos cuando los necesita y cita los materiales utilizados.
Las secciones y los archivos pueden requerir varias llamadas para leerlos enteros.
Se admiten PDF con texto, HTML, TXT, MD y CSV de hasta 10 MiB por respuesta. No hay
OCR ni lectura de DOCX, PPTX, vídeo o enlaces alojados fuera del campus.

Los avisos de falta de permiso indican que el resultado puede estar incompleto.
Los plazos de tareas no indican si ya has entregado, y el calendario puede mostrar
solo un horizonte limitado. Las fechas se devuelven en UTC.

## Licencia

Mudel AI Connect se distribuye con licencia MIT. El paquete incluye el texto en
`LICENSE`. Las dependencias conservan sus propias licencias.
