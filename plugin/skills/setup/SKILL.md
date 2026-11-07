---
name: setup
description: Configura o repara la conexión nativa de Mudel AI Connect con el campus del usuario.
---

# Configurar Mudel AI Connect

Este paquete incluye un servidor nativo para la plataforma indicada en su README.
No hay que descargar dependencias.

Primero prueba `moodle_status`. Si ya funciona, conserva la cuenta y continúa con
la petición del usuario. Un recurso no accesible no implica un fallo de toda la conexión.

La raíz del plugin está dos niveles por encima de esta skill. Consulta su README
para localizar `bin/mudel-ai-connect-server.exe` en Windows o
`bin/mudel-ai-connect-server` en macOS. El usuario configura su cuenta ejecutando
ese binario con `--setup --plugin` en un terminal local: pide la URL y oculta el token.
Nunca solicites ni recibas el token en el chat. No aceptes una contraseña de Moodle.
No incluyas secretos en argumentos, historial de comandos ni archivos del plugin.

Comprueba después con `moodle_status` o con el binario y `--check --plugin`.
Explica las funciones ausentes sin mostrar credenciales. La cuenta del plugin se
guarda aparte de la del instalador; comparte los permisos de consulta y descargas.
El entorno `MOODLE_URL`/`MOODLE_TOKEN`, si está definido, tiene prioridad y debe
contener ambas variables. No importes archivos ni claves que el usuario no identifique.

Tras configurar la cuenta, reinicia la conexión MCP del cliente si conserva un
estado anterior. Para consultar o estudiar, usa `mudel-ai-connect-study`.
Si hay consultas desactivadas, explica el límite; no cambies permisos para eludirlo.
