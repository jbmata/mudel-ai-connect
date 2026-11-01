# Moodle remoto

La versión remota ejecuta las mismas seis herramientas de consulta que el plugin
local. Cada conexión de ChatGPT o Claude autoriza su propia cuenta mediante OAuth
con PKCE S256. El inicio de sesión solicita una clave de servicios web de Moodle
en un formulario HTTPS; la contraseña de la universidad no se utiliza.

El campus lo configura el administrador. Esta beta está preparada para SIFO;
el formulario no acepta URLs arbitrarias. Las herramientas siguen siendo de lectura.

## Instalación en Ubuntu

El servidor necesita Python 3.11+, un nombre DNS que apunte a él y acceso entrante
por los puertos 80 y 443. Puede utilizarse inicialmente el nombre que asigna el
proveedor al VPS. La publicación en un directorio puede exigir un dominio propio
verificable, además de sus requisitos de revisión.

Sube únicamente el código revisado a `/opt/moodle-mcp/app`, con propietario root.
Excluye `.env`, credenciales, entornos virtuales y el contenido de `tmp` y `dist`.
En un servidor nuevo y dedicado al proyecto, ejecuta:

```bash
sudo bash /opt/moodle-mcp/app/deploy/provision.sh mcp.example.com
```

Sustituye `mcp.example.com` por el nombre real. El script instala las dependencias,
Caddy y un servicio `moodle-mcp` que se inicia automáticamente. La aplicación
escucha solo en `127.0.0.1:8000`; Caddy publica HTTPS y gestiona su certificado.
El instalador se detiene si encuentra una configuración Caddy previa ajena al proyecto.

La configuración está en `/etc/moodle-mcp.env`:

```dotenv
PUBLIC_URL=https://mcp.example.com
MOODLE_DATA_DIR=/var/lib/moodle-mcp
MOODLE_CAMPUS_URL=https://sifo.comillas.edu
```

No configures una clave Moodle global. Cada usuario introduce la suya durante
el proceso de conexión de su cliente de IA.

## Conectar una cuenta

Añade `https://mcp.example.com/mcp` como servidor MCP remoto en el cliente y elige
OAuth con registro dinámico de cliente. El cliente descubre `/authorize`, `/token`
y `/register` desde `/.well-known/oauth-authorization-server`.

El flujo abre el formulario de conexión de Moodle. Autoriza las consultas con
tu clave del servicio **Moodle mobile web service**. Introduce la clave solo en
ese formulario HTTPS, nunca en el chat ni en el repositorio.

Esta beta acepta los callbacks oficiales de ChatGPT y Claude web. Si el cliente
utiliza una URL diferente, revisa la documentación de ese cliente antes de cambiar
la lista de callbacks permitidos. La disponibilidad en móvil y en cada cuenta
depende del cliente; el despliegue no publica automáticamente el plugin en sus directorios.

## Datos y mantenimiento

- Las credenciales, los clientes OAuth y los códigos se cifran con Fernet en
  `accounts.sqlite3`. Los identificadores de tokens se guardan como hashes.
- La clave de cifrado se genera al arrancar y se guarda en `encryption.key`,
  fuera del código, con permisos del usuario del servicio. Conserva copias seguras
  tanto de la base de datos como de esa clave para poder restaurar las conexiones.
- Los accesos caducan en una hora. La conexión dura hasta 30 días y requiere
  volver a autorizarse después. Los refresh tokens rotan; su reutilización revoca
  la conexión. El endpoint OAuth `/revoke` también invalida toda la conexión.
- No hay logs de acceso HTTP que guarden códigos OAuth. Los errores de Moodle
  y los resultados de herramientas ocultan la clave.
- Actualizar el código conserva los datos en `/var/lib/moodle-mcp`. Tras una
  actualización, reinicia el servicio con `sudo systemctl restart moodle-mcp`.
- El alojamiento y el dominio siguen necesitando mantenimiento. Esta beta
  cuenta con pruebas automáticas; no ha pasado una auditoría externa.

Para comprobar el servicio sin consultar datos personales:

```bash
curl https://mcp.example.com/health
sudo systemctl status moodle-mcp
```

El endpoint `/health` devuelve `{"status":"ok"}`. Una llamada a `/mcp` sin
autenticación debe recibir HTTP 401 y la información de descubrimiento OAuth.

## Pruebas

```powershell
.\.venv\Scripts\python.exe -m pip install -r requirements-server.txt
.\.venv\Scripts\python.exe -m unittest discover -s tests -v
```

Las pruebas usan cuentas ficticias y respuestas simuladas de Moodle. Comprueban
el flujo OAuth, PKCE, CSRF, callbacks, audiencia, cifrado, revocación, rotación,
aislamiento de cuentas y consumo único de códigos.

## Empaquetar el plugin remoto

```powershell
.\.venv\Scripts\python.exe scripts/build_plugin.py --remote-url https://mcp.example.com/mcp
```

Genera `dist/moodle-remote-plugin-0.1.0.zip`, con el MCP remoto, la skill de estudio
y el logo. La autenticación se realiza desde el cliente; el paquete no lleva
credenciales ni el instalador local. El ZIP local se sigue generando sin esa opción.
