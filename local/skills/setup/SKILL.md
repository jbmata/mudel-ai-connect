---
name: setup
description: Instala y configura la conexión local de Moodle AI Connect, conecta la cuenta del usuario o diagnostica fallos del MCP. Úsala para configurar o reparar el acceso al campus.
---

# Configurar Moodle AI Connect

El plugin incluye un MCP local y la skill `moodle-ai-connect-study`. Necesita Python 3.11+
y un token de servicios web autorizado de la cuenta del usuario.

Esta skill se ocupa de la conexión. Las consultas del campus, el estudio y la
organización se atienden con `moodle-ai-connect-study` usando esa misma conexión.

## Comprobar antes de cambiar

Si las herramientas ya están disponibles, usa `moodle_status` para comprobar la
cuenta y las funciones autorizadas. Si la conexión funciona, conserva la
configuración y continúa con la petición del usuario. No reinstales dependencias
ni pidas otra clave como parte de una consulta habitual.

Un aviso de acceso a una actividad o un formato de archivo no admitido puede
afectar solo a ese recurso. Distingue ese caso de un fallo de conexión antes de
cambiar credenciales. No reinicies ni regeneres claves desde Moodle.

## Preparar la conexión local

Localiza la raíz del plugin a partir de la ubicación de esta skill. El instalador
está en `../../plugin_runtime.py` respecto al directorio de esta skill. Consulta
`../../README.md` para los pasos propios de cada cliente; en el árbol de desarrollo
la guía del plugin se llama `PLUGIN.md`.

Si hay acceso a un terminal local, prepara las dependencias ejecutando
`python "<raíz del plugin>/plugin_runtime.py" setup --no-config`. La instalación
requiere descargar las dependencias de Python. Después, el usuario puede ejecutar
`python "<raíz del plugin>/plugin_runtime.py" setup` en su propio terminal para
introducir la clave de forma oculta. No solicites ni recibas el token en el chat.

Cuando el usuario ya tiene un archivo `.env` local con su cuenta configurada y
pide reutilizarla, puedes importarlo con
`python "<raíz del plugin>/plugin_runtime.py" setup --from-env "<archivo .env>"`.
Importa solo el archivo que el usuario identifique como su configuración. La clave
queda en el perfil local del usuario, fuera de la carpeta y del ZIP del plugin.

## Verificar y explicar los límites

Comprueba la conexión con `moodle_status` o con
`python "<raíz del plugin>/plugin_runtime.py" run --check`. Describe las funciones
ausentes y los errores de acceso sin mostrar credenciales. Tras configurar la
cuenta, pide reiniciar la conexión MCP en el cliente si aún conserva un fallo de
arranque anterior y ofrece probar el listado de asignaturas.

Esta versión utiliza `stdio`. Si el cliente solo permite conectores remotos,
explica que hace falta un despliegue HTTPS autenticado antes de poder utilizar
las herramientas; instalar las instrucciones por sí solo no conecta Moodle.
