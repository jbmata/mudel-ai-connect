# Cambios

## v0.1.0 — beta

Primera versión de prueba. Pensada para un grupo reducido que pueda informar de fallos.

### Incluye

- Instalador para Windows, `Mudel-AI-Connect-Setup.exe`, que no necesita Python. Conecta
  Claude Desktop, Claude Code y ChatGPT Work local / Codex con un servidor MCP local de
  solo lectura.
- Plugin para Codex y Claude Code, con las skills de estudio y de configuración
  (requiere Python 3.11 o superior).
- Token de Moodle cifrado con tu usuario de Windows (DPAPI) y enviado solo a tu Moodle.
- Botón **Permisos** para elegir qué puede consultar la IA.
- Estado de cada aplicación en vivo, reparación de instalaciones incompletas y
  desinstalación que avisa si algo queda.
- **Copiar diagnóstico** para pedir ayuda (no incluye el token) y aviso de versiones nuevas.
- Migración automática desde «Moodle AI Connect»: reutiliza el token guardado y retira
  las entradas con el nombre anterior.
- Compatible con la nueva carpeta de datos de Claude Desktop (`%LOCALAPPDATA%\Claude-Data`),
  que usa desde la versión 4.16336.

### Límites conocidos

- Solo Windows. El instalador **no está firmado**: SmartScreen y algunos antivirus pueden
  avisar. Comprueba la huella con `SHA256SUMS.txt`.
- Probado con SIFO (Universidad Pontificia Comillas). Otros campus deben permitir a los
  alumnos crear un token del servicio móvil de Moodle.
- ChatGPT: solo en **Work local**. El registro está comprobado, pero falta verificar una
  consulta real; el modo Chat no recibe el MCP.
- Depende de dónde guardan su configuración Claude y Codex. Si una actualización de esas
  aplicaciones lo cambia, hará falta una versión nueva del instalador.
- No aparece en «Aplicaciones instaladas» de Windows: se desinstala desde el propio programa.
- El PC debe estar encendido mientras usas la conexión.
