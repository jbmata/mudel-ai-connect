# Cambios

## Sin publicar

- **Avisos compactos:** `list_assignments` agrupa avisos iguales que solo difieren
  en `itemid` y conserva sus identificadores en `itemids` con el número en `count`.
  Un aviso individual conserva el formato anterior; los consumidores deben admitir ambos.
- **Listados compactos:** `list_courses` omite las descripciones de cursos y
  `list_assignments` los enunciados por defecto. Usa `include_descriptions=true`
  para recuperarlos con sus límites anteriores. Se conservan enlaces, fechas y avisos.
  Las instrucciones evitan cargar el temario al consultar fechas o estados y repetir
  el listado de cursos si ya se conoce el ID.

## 0.3.0 — búsqueda en materiales, figuras y actualizaciones

Pre-release para un grupo reducido de pruebas.

### Novedades

- **Buscar dentro de los materiales:** `search_course_files` devuelve hasta 5 fragmentos de
  800 caracteres con sus fuentes y la posición para ampliar la lectura. Busca en una
  asignatura, pagina con `next_file_start` y reutiliza texto en memoria durante la conexión.
  Permite responder dudas concretas sin enviar documentos completos a la IA.
- **Ver figuras:** `view_course_file` muestra páginas de PDF e imágenes incrustadas de
  PowerPoint, Word y Excel. `save_course_figures` guarda esas figuras en Descargas para que
  el usuario pueda verlas aunque su cliente no muestre las imágenes del chat.
- **Figuras al leer:** `read_course_file` adjunta hasta 3 figuras de las páginas o diapositivas
  leídas; `with_figures=false` permite pedir solo texto. Señala las páginas o diapositivas
  con figuras y sugiere ver los PDF escaneados como imágenes. El texto de PDF incluye marcas de página.
- **Preguntas guiadas:** `preparar_examen`, `plan_de_estudio` y `resumir_tema` orientan el uso
  de las herramientas y las citas de los materiales del curso.
- **Actualización automática de la conexión MCP:** al iniciarse comprueba GitHub Releases,
  verifica plataforma, tamaño, SHA-256 y arranque del servidor nuevo, y lo prepara para la
  siguiente sesión. Conserva cuenta y permisos, restaura el anterior si falla y evita rebajar versiones.

### Actualizar desde 0.2.0

Descarga y ejecuta el instalador nuevo una vez: la 0.2.0 todavía no incluye el actualizador.
Después, la conexión MCP se actualizará al iniciarse. La ventana de instalación y el plugin
se renuevan descargando sus paquetes; el número de la ventana corresponde al instalador.

### Límites conocidos

- La búsqueda usa palabras clave en el mismo fragmento, sin OCR ni búsqueda semántica.
  Revisa hasta 8 archivos por llamada y avisa de errores y archivos omitidos. Su índice
  se descarta al cerrar el MCP; la primera búsqueda puede tardar y el ahorro real de tokens no se ha medido.
- Las formas y gráficos de PowerPoint y Office y los formatos EMF, WMF y SVG no se renderizan;
  para ver diapositivas completas, aporta una exportación a PDF.
- Los paquetes siguen sin firma de desarrollador ni notarización: ESET, SmartScreen y otros
  controles pueden avisar o bloquearlos. Comprueba las huellas de los paquetes.
- La búsqueda nueva y la actualización automática están probadas con respuestas simuladas.
  Falta validar el flujo completo con un campus real y las aplicaciones de IA en Windows y Mac.

## 0.2.0 — primera versión

Pensada para un grupo reducido de pruebas que pueda informar de fallos.

### Incluye

- Instalador con ventana para **Windows** (x64) y **macOS** (Apple Silicon). Conecta Claude
  Desktop, Claude Code y ChatGPT Work local / Codex con un servidor MCP local de solo lectura.
- Plugin para Codex y Claude Code, con las skills de estudio y de configuración.
- El token se guarda en el Llavero de macOS o cifrado con DPAPI en Windows, con cuentas
  separadas para el instalador y para el plugin, y solo se envía a tu Moodle.
- Consultas: asignaturas, secciones y materiales (`get_course_context`), tareas y plazos,
  estado de entrega y notas (`get_assignment_status`, `get_grades`), calendario, foros
  (`list_forums`, `get_forum_discussions`, `get_discussion_posts`) y búsqueda en un curso
  (`search_course`). `moodle_status` indica qué funciones opcionales autoriza el campus.
- Lectura de archivos: PDF con texto, Word (DOCX), PowerPoint (PPTX, con las notas del ponente),
  Excel (XLSX), HTML, texto, páginas y libros de Moodle (`read_course_file`, y
  `read_course_files` para hasta 6 archivos a la vez). Los formatos antiguos (`.doc`, `.ppt`,
  `.xls`) se rechazan con un aviso claro.
- `download_course_file`: guarda un archivo del curso, en cualquier formato, en
  `Descargas/Mudel AI Connect/<asignatura>` (o en la carpeta que elijas en **Permisos**). No
  sobrescribe nada, reutiliza el archivo si ya está igual y lo marca como descargado de
  internet. No descarga programas ni scripts y admite hasta 200 MiB.
- Botón **Permisos** para elegir qué puede consultar y descargar la IA.
- El instalador no exige cerrar Claude Desktop ni las demás apps: sustituye el servidor
  aunque esté en uso y avisa al terminar de que Claude Desktop debe reiniciarse.
- Estado de cada aplicación en vivo, reparación de instalaciones incompletas y desinstalación.
- **Copiar diagnóstico** para pedir ayuda (no incluye el token) y aviso de versiones nuevas
  leyendo las releases públicas de GitHub.
- Migración automática desde «Moodle AI Connect»: reutiliza el token guardado y retira las
  entradas con el nombre anterior.
- Compatible con la carpeta de datos de Claude Desktop de las versiones recientes
  (`%LOCALAPPDATA%\Claude-Data`).

### Límites conocidos

- Los paquetes aún no están firmados ni notarizados: SmartScreen y algunos antivirus pueden
  avisar o marcarlos como sospechosos. Comprueba la huella con `SHA256SUMS.txt`.
- Probado con SIFO (Universidad Pontificia Comillas) en Windows. Otros campus deben permitir a
  los alumnos crear un token del servicio móvil de Moodle.
- Falta probar el instalador completo, la descarga de archivos y cada cliente de IA en Windows
  y en Mac.
- ChatGPT: solo en **Work local**. El modo Chat no recibe el MCP.
- Depende de dónde guardan su configuración Claude y Codex. Si una actualización de esas
  aplicaciones lo cambia, hará falta una versión nueva.
- El PC debe estar encendido mientras usas la conexión.
