# Mejoras elegidas

Lista de lo que se quiere añadir a Mudel AI Connect, con su estado y las notas de diseño que
ya se han comprobado. Todo es de **solo lectura**: no se añade ninguna herramienta que cambie
algo en Moodle.

Última revisión: 2026-11-07.

## Estado

| # | Mejora | Estado |
| --- | --- | --- |
| A1 | Leer DOCX, PPTX y XLSX | **Hecho.** Probado con un `.docx`, un `.pptx` y un `.xlsx` reales |
| A3 | Leer páginas y libros de Moodle | **Hecho.** Páginas probadas con datos reales; libros solo con datos de ejemplo |
| B1 | Estado de entrega, notas y feedback | **Hecho.** Probado con datos reales |
| B2 | Foros y anuncios | **Hecho.** Probado con un foro de avisos real con una discusión y su respuesta |
| B3 | Búsqueda en un curso (`search_course`) | **Hecho.** En todas las asignaturas tarda unos 8 s |
| C2 | Leer varios archivos de golpe | **Hecho.** Probado con datos reales |

Las seis están implementadas, con pruebas automáticas, y documentadas en el README, la guía de
desarrollo, la skill de estudio y el CHANGELOG. Lo que sigue en este archivo describe el diseño
y lo que se decidió dejar fuera.

## A1. Leer DOCX, PPTX y XLSX

`read_course_file` hoy solo lee PDF con texto, HTML, TXT, MD y CSV.

Por qué: en los 12 primeros cursos de la cuenta de pruebas hay 42 `.xlsx`, 25 `.docx` y 16 `.pptx`
(frente a 757 PDF), además de 3 `.ppt` antiguos y 2 `.ods`.

- DOCX: párrafos, tabulaciones, saltos de línea y tablas (`celda | celda`).
- PPTX: una sección por diapositiva, en orden numérico, con las notas del ponente.
- XLSX: una sección por hoja visible, celdas separadas por tabuladores. Las fechas salen como
  número de serie de Excel (no se aplica el formato de la celda).
- `.doc`, `.ppt` y `.xls` (formatos binarios antiguos): no se leen; el error dice que se guarde
  el archivo en el formato moderno.
- Seguridad: límites de tamaño al descomprimir (64 MiB por parte, 160 MiB en total, 20.000
  entradas) contra «bombas ZIP», y un documento malformado no puede cerrar el servidor.
- Fuera de alcance por ahora: ODT/ODP/ODS (LibreOffice) y OCR para PDF escaneados.

## A3. Páginas y libros de Moodle

Hallazgo con datos reales: una **página** de Moodle (`mod_page`) ya aparece en
`get_course_context` como un archivo `index.html`, y `read_course_file` la lee correctamente.
Lo que falta es otra cosa:

- **Libros** (`mod_book`): cada capítulo es un `index.html` y la IA no puede distinguirlos.
  Hay que leer la estructura del libro y poner el título de cada capítulo en su `file_index`.
  La cuenta de pruebas no tiene ningún libro, así que se probará con datos de ejemplo según el
  formato documentado por Moodle.
- **Etiquetas** (`label`): su contenido viaja en la descripción del módulo, que sigue
  cortándose a 4.000 caracteres. No se cambió.
- Las instrucciones del servidor y la skill indican ya que las páginas y los libros se leen
  con `read_course_file`.

## B1. Estado de entrega, notas y feedback

Hoy `list_assignments` avisa de que no sabe si el alumno ha entregado.

- `get_assignment_status`: estado de la entrega (nueva, borrador, enviada…), si está calificada,
  nota, comentarios del profesor, prórroga y si está bloqueada. Usa `mod_assign_get_submission_status`.
- `get_grades`: calificaciones del curso con su nota, rango, porcentaje y comentario. Usa
  `gradereport_user_get_grade_items`.
- Cambiar la nota de `list_assignments` para que remita a la herramienta nueva.
- Permiso: se cuelga de «Tareas».

## B2. Foros y anuncios

- `list_forums`: foros de un curso, con su tipo (avisos, general…). Usa
  `mod_forum_get_forums_by_courses`.
- `get_forum_discussions`: discusiones de un foro con el mensaje inicial.
- `get_discussion_posts`: las respuestas de una discusión.
- Permiso: se cuelga de «Materiales y archivos».
- Nota: los cursos de la cuenta de pruebas tienen 10 foros normales y 11 de «foro avanzado»
  (`hsuforum`, otro plugin con funciones propias). De momento solo los normales.
- Los mensajes se recortan a 4.000 caracteres cada uno.

## B3. `search_course`

Busca un texto en un curso sin recorrerlo sección a sección.

- Compara sin distinguir mayúsculas ni tildes con: nombre y resumen de cada sección, nombre y
  descripción de cada módulo, y nombres de archivo.
- No busca dentro del contenido de los archivos (sería lento y costoso).
- Devuelve módulo, sección, `file_index` y enlace.
- `course_id` opcional: sin él, busca en todas las asignaturas del usuario (hasta 50), de 6 en 6
  a la vez, y devuelve como máximo 40 resultados. Una asignatura sin acceso no impide las demás.
- Permiso: «Materiales y archivos».

## C2. Leer varios archivos de golpe

Una herramienta que lee hasta 6 archivos de un curso en una sola llamada (por ejemplo «resume
todo el tema 3»).

- Descarga la lista de contenidos del curso una sola vez en lugar de una por archivo.
- Devuelve el primer fragmento de cada uno, con `next_offset` para seguir leyendo con
  `read_course_file`, y el error de cada archivo que falle sin cancelar los demás.
- Permiso: «Materiales y archivos».

## Notas comunes

- **Permisos**: no se añaden categorías nuevas. El archivo de permisos se valida con
  exactamente las 5 categorías actuales, y añadir una bloquearía a quien ya lo tenga guardado.
- **Funciones de Moodle**: hay que ampliar la lista de funciones autorizadas del cliente y que
  `moodle_status` informe de cuáles de las nuevas permite el campus del usuario.
- **Documentación a actualizar** al terminar: `src/instructions.txt`, la skill de estudio,
  la tabla de herramientas del README, `docs/DESARROLLO.md` y el CHANGELOG.

## Conectar con tu universidad

Botón en la ventana para obtener el token iniciando sesión en la página oficial del campus, sin
escribir la clave a mano. Estado: **implementado en Windows; falta probarlo con un inicio de sesión
real** (ver el detalle en `docs/DESARROLLO.md`).

- Comprobado en tu campus: `launch.php` existe y redirige al login sin dar nada; ofrece acceso con
  Office 365, Google y EduGain.
- Pendiente: probarlo pulsando el botón, soporte para macOS y comprobar qué hace el campus si no
  acepta el esquema `mudelai://`.

## No elegidas por ahora

- OCR para PDF escaneados (alto esfuerzo y añade peso al instalador).
- Cuestionarios: fechas, intentos y notas.
- Caché local de archivos ya leídos.
- ODT, ODP y ODS.
- Firma de código y notarización (depende de elegir proveedor y certificado).
- Botón «Reiniciar Claude Desktop» tras instalar.
- Probar la instalación real, `download_course_file` y la versión de Mac.
