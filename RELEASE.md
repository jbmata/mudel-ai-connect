# Publicar una versión

La aplicación es nativa (Rust). Los binarios se distribuyen como adjuntos de
[GitHub Releases](https://github.com/jbmata/mudel-ai-connect/releases); el repositorio solo
contiene el código fuente. La ventana avisa de versiones nuevas leyendo esas releases, así
que cada versión tiene que publicarse ahí.

## Preparar la versión

1. Sube la versión en `Cargo.toml` (`version`) y en `plugin/metadata/plugin.json` y
   `plugin/metadata/claude-plugin.json`. `scripts/package.ps1` falla si no coinciden.
2. Añade la entrada en [CHANGELOG.md](CHANGELOG.md): qué incluye y los límites conocidos.
3. Ejecuta formato, Clippy y pruebas:

   ```text
   cargo fmt -- --check
   cargo clippy --locked --all-targets --features desktop -- -D warnings
   cargo test --locked --features desktop
   ```

## Generar los paquetes

Los paquetes que se publican deben salir del workflow **Tests** (artefactos
`mudel-native-*`), que compila en Windows con MSVC y en Mac con Apple Silicon desde el
commit que vayas a etiquetar. Para generarlos a mano, en cada plataforma:

```powershell
./scripts/package.ps1
./scripts/verify-package.ps1
```

Se crean en `dist/native/<sistema-arquitectura>/`: el instalador (`Mudel-AI-Connect-Setup.exe`
o el ZIP de la app de Mac), el ZIP del plugin, `LICENSE`, `README.md` y `SHA256SUMS.txt`.
Las instrucciones para compilar en este equipo Windows sin Visual C++ Build Tools están en
[docs/DESARROLLO.md](docs/DESARROLLO.md).

## Probar antes de publicar

Las pruebas automáticas usan respuestas simuladas; no verifican un campus real. En un
equipo de prueba, con cada plataforma que vayas a anunciar:

1. Instala con el paquete nuevo, con Claude Desktop abierto, y comprueba que pide cerrarlo.
2. Pregunta en cada cliente: «Consulta Moodle y dime qué asignaturas tengo».
3. Pide descargar un archivo y comprueba que queda en la carpeta de Descargas elegida.
4. Prueba **Copiar diagnóstico**, **Permisos** y **Desinstalar**.
5. Instala encima de la versión anterior y comprueba que no quedan entradas duplicadas.

## Publicar en GitHub

1. Revisa los cambios con `git diff`. Comprueba que `.env`, credenciales, copias de
   configuración y temporales están excluidos.
2. Sube el commit y espera a que termine el workflow **Tests**.
3. Crea la release con la etiqueta `vX.Y.Z`. Mientras sea una beta, márcala como
   **pre-release**: el aviso de versiones nuevas también las tiene en cuenta.
4. Adjunta los archivos de cada plataforma y su `SHA256SUMS.txt`.
5. En la descripción copia la entrada de `CHANGELOG.md` y explica cómo pasar el aviso de
   SmartScreen («Más información → Ejecutar de todas formas») y cómo comprobar la huella:

   ```powershell
   Get-FileHash .\Mudel-AI-Connect-Setup.exe -Algorithm SHA256
   ```

El instalador incluye la licencia MIT del proyecto dentro del servidor instalado. Las
dependencias conservan sus propias licencias. `dist/` queda fuera de Git.

## Antes de un lanzamiento abierto

- **Firma el instalador** (SignPath Foundation, Azure Trusted Signing o un certificado
  de firma de código) y **notariza la app de macOS**. Sin firma, SmartScreen y algunos
  antivirus (por ejemplo, ESET con su detección heurística ML/Augur) pueden marcar o
  borrar el instalador.
- Prueba con al menos otro campus además de SIFO.
- Revisa con alguien con criterio legal el uso de «Moodle» en textos y materiales.
