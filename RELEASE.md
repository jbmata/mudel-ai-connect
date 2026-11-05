# Publicar una versión

El repositorio contiene el código fuente. Los binarios se distribuyen como adjuntos
de [GitHub Releases](https://github.com/jbmata/mudel-ai-connect/releases). El instalador avisa
de versiones nuevas leyendo esas releases, así que cada versión tiene que publicarse ahí.

## Preparar la versión

1. Sube la versión en los tres sitios: `VERSION` en `local/mudel_ai_connect_installer.py`,
   `local/plugin.json` y `local/.claude-plugin/plugin.json`. Un test falla si no coinciden.
2. Añade la entrada en [CHANGELOG.md](CHANGELOG.md): qué incluye y los límites conocidos.
3. Prepara el entorno y ejecuta `python run_tests.py -v` como explica
   [README.md](README.md#desarrollo-y-pruebas).

## Generar los paquetes

En Windows, desde la raíz y con el código ya en un commit:

```powershell
.\.venv\Scripts\python.exe local\scripts\build_exe.py
.\.venv\Scripts\python.exe local\scripts\build_plugin.py
.\.venv\Scripts\python.exe local\scripts\checksums.py
```

Se generan `dist/Mudel-AI-Connect-Setup.exe`, `dist/mudel-ai-connect-plugin-X.Y.Z.zip` y
`dist/SHA256SUMS.txt`. Genera el `.exe` desde el commit que vayas a etiquetar: así el
binario publicado corresponde a un código concreto.

## Probar antes de publicar

Las pruebas automáticas usan respuestas simuladas; no verifican un campus real. En un
perfil de prueba o un equipo de prueba:

1. Instala con el `.exe` nuevo, con Claude Desktop abierto, y comprueba que pide cerrarlo.
2. Pregunta en cada cliente que vayas a anunciar: «Consulta Moodle y dime qué asignaturas
   tengo».
3. Prueba **Copiar diagnóstico**, **Permisos** y **Desinstalar**.
4. Si cambiaste de nombre o de carpetas, instala encima de la versión anterior y comprueba
   que no quedan entradas duplicadas.

## Publicar en GitHub

1. Revisa los cambios con `git diff` y `git diff --cached`. Comprueba que `.env`,
   credenciales, copias de configuración y temporales están excluidos.
2. Crea el commit, súbelo y espera a que termine el workflow **Tests**.
3. Crea la release con la etiqueta `vX.Y.Z`. Mientras sea una beta, márcala como
   **pre-release**: el aviso de versiones nuevas también las tiene en cuenta.
4. Adjunta los tres archivos de `dist/`: el instalador, el ZIP del plugin y `SHA256SUMS.txt`.
5. En la descripción copia la entrada de `CHANGELOG.md` y añade cómo pasar el aviso de
   SmartScreen («Más información → Ejecutar de todas formas») y cómo comprobar la huella:

   ```powershell
   Get-FileHash .\Mudel-AI-Connect-Setup.exe -Algorithm SHA256
   ```

El plugin incluye `LICENSE`; el instalador la lleva en `_internal/licenses/LICENSE` dentro
del servidor local empaquetado. Las dependencias conservan sus propias licencias.

`dist/` se conserva fuera de Git. Comparte los paquetes generados, que usan una
lista explícita de archivos públicos, y conserva las credenciales en tu perfil local.

## Antes de un lanzamiento abierto

- **Firma el instalador** (por ejemplo con Azure Trusted Signing) para evitar los avisos
  de SmartScreen y los falsos positivos de antivirus.
- Prueba con al menos otro campus además de SIFO.
- Revisa con alguien con criterio legal el uso de «Moodle» en textos y materiales.
