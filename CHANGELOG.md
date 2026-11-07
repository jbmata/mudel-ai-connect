# Cambios

## Sin publicar — aplicación nativa

- Todo el programa está reescrito en Rust y no necesita Python: servidor MCP, ventana de
  instalación y lanzador del plugin. Windows x64 y macOS con Apple Silicon.
- Las siete herramientas, el prompt `estudiar_curso` y los permisos conservan sus nombres,
  parámetros y resultados. Las cuentas y preferencias de la v0.2 se reconocen al actualizar.
- El token se guarda en el Llavero de macOS o cifrado con DPAPI en Windows, con cuentas
  separadas para el instalador y para el plugin.
- La ventana avisa de versiones nuevas leyendo las releases públicas de GitHub.
- Se retiran del repositorio el instalador, el servidor y las pruebas en Python. Las
  versiones v0.1 y v0.2 siguen disponibles en sus releases.
- El instalador ya no exige cerrar Claude Desktop ni las demás apps: sustituye el servidor
  aunque esté en uso y avisa al terminar de que Claude Desktop debe reiniciarse.
- La ventana tiene tamaño fijo, sin barra de desplazamiento, y un aspecto de instalador
  de escritorio.
- El empaquetado genera ZIP con rutas válidas también desde Windows PowerShell 5.1.

### Límites conocidos

- Los paquetes aún no están firmados ni notarizados. Algunos antivirus pueden marcarlos.
- Probado con SIFO (Comillas) en Windows. Falta probar el instalador completo, la descarga
  de archivos y cada cliente de IA en Windows y en Mac.

## v0.2.0 — beta

- Herramienta `download_course_file`: guarda un archivo del curso, en cualquier formato,
  en `Descargas\Mudel AI Connect\<asignatura>`. No sobrescribe nada, reutiliza el archivo si
  ya está igual y Windows lo marca como descargado de internet. No descarga programas ni
  scripts (`.exe`, `.bat`, `.js`…) y admite hasta 200 MiB.
- Permiso nuevo **Descargar archivos a tu carpeta Descargas**. Si ya habías guardado tus
  permisos, toma el valor que elegiste para «Materiales y archivos».
- En **Permisos → Cambiar carpeta…** eliges dónde se guardan las descargas. Si esa carpeta
  no está disponible, se guardan en Descargas y se avisa.

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
