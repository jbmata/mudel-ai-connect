# Mejoras pendientes

Lo que falta por añadir a Mudel AI Connect y las notas de diseño que ya se han comprobado. Todo
es de **solo lectura**: no se añade ninguna herramienta que cambie algo en Moodle, salvo guardar
archivos en la carpeta de descargas que elige el usuario.

Última revisión: 2026-11-08.

## Propuestas pendientes

Ordenadas por lo que aportan frente al esfuerzo. Ninguna está empezada.

| # | Propuesta | Esfuerzo | Notas |
| --- | --- | --- | --- |
| P1 | **Firmar los ejecutables** | Medio (trámites) | Es lo más urgente: ESET (detección heurística `ML/Augur`) borra el instalador sin firmar y SmartScreen avisa. SignPath Foundation firma gratis proyectos de código abierto; hay que pedir el alta y no se sabe cuánto tardan. En Mac hay que notarizar con una cuenta de desarrollador de Apple (unos 99 $ al año). |
| P2 | **PDF escaneados** | Mínimo | `read_course_file` dice «puede requerir OCR». Con `view_course_file` el modelo ya puede ver esas páginas como imagen, así que basta cambiar el mensaje para que lo sugiera. Sin OCR en el instalador. |
| P3 | **Avisar de que un material tiene figuras** | Bajo | Que `read_course_file` indique, por ejemplo, «la página 2 tiene 3 imágenes» o «la diapositiva 4 tiene dibujos», para que el modelo sepa cuándo llamar a `view_course_file` sin depender de que se le ocurra. |
| P4 | **Preguntas guiadas (prompts MCP)** | Bajo | Plantillas como «hazme un test con las pruebas anteriores de esta asignatura», «plan de estudio hasta la próxima entrega» o «resume el tema 3 con sus fuentes». Ya existe `estudiar_curso`; estas se añaden igual. |
| P5 | **Diapositivas completas de PowerPoint** | Medio | Hoy solo salen las imágenes incrustadas. Exportar la presentación a PDF con PowerPoint (solo Windows con Office) o con LibreOffice si está instalado, y dibujar esas páginas con el mismo motor. Tarda unos segundos por archivo y depende de software externo; no se ha probado. |
| P6 | **Buscar dentro de los archivos** | Alto | `search_course` solo busca en nombres de archivo y secciones. Buscar «termopar» y encontrar la prueba exige un índice local de texto, con su coste de disco y de actualización. |

## Notas para añadir herramientas

- **Permisos**: no se añaden categorías nuevas. El archivo de permisos se valida con
  exactamente las 5 categorías actuales, y añadir una bloquearía a quien ya lo tenga guardado.
- **Funciones de Moodle**: hay que ampliar la lista de funciones autorizadas del cliente y que
  `moodle_status` informe de cuáles de las nuevas permite el campus del usuario.
- **Documentación a actualizar** al terminar: `src/instructions.txt`, la skill de estudio,
  la tabla de herramientas del README, `docs/DESARROLLO.md` y el CHANGELOG.

## No elegidas por ahora

- Botón «Conectar con tu universidad» (iniciar sesión en el campus para obtener el token). Se
  probó con la redirección a `mudelai://` de `launch.php`: Chrome la deja en blanco sin avisar, y
  Moodle no admite una dirección local en `urlscheme` (solo un esquema de protocolo). Se descartó;
  el token se introduce a mano.
- OCR dentro del instalador (alto esfuerzo y añade peso); ver P2.
- Cuestionarios: fechas, intentos y notas.
- Mensajes de Moodle.
- Caché local de archivos ya leídos.
- ODT, ODP y ODS.
- Botón «Reiniciar Claude Desktop» tras instalar.
- Probar la instalación real, `download_course_file` y la versión de Mac.
