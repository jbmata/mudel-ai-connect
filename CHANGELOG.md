# Cambios

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
- `view_course_file`: muestra figuras como imágenes para que la IA pueda leer circuitos, gráficas y
  esquemas. Dibuja páginas de PDF (hasta 4 por consulta) y devuelve las imágenes incrustadas de
  PPTX, DOCX y XLSX o una imagen suelta (PNG, JPG, GIF, WEBP). Los dibujos hechos con formas de
  PowerPoint, los gráficos de Office y los formatos EMF, WMF y SVG no se pueden mostrar, y avisa
  de ello.
- Aviso de figuras: al leer un PDF, `read_course_file` indica qué páginas tienen imágenes o dibujos (se
  detectan interpretando la página sin dibujarla); en PPTX, DOCX y XLSX lo marca en el propio texto.
  Un PDF sin texto (escaneado) sugiere verlo con `view_course_file` en lugar de pedir OCR.
- Preguntas guiadas (prompts MCP): `preparar_examen`, `plan_de_estudio` y `resumir_tema`.
- `save_course_figures`: guarda esas mismas figuras en `Descargas/Mudel AI Connect/<asignatura>` y
  devuelve la ruta, para que el usuario las abra aunque su cliente no muestre las imágenes del chat.
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
