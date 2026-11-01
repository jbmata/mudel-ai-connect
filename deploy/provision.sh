#!/usr/bin/env bash
set -euo pipefail

public_host=${1:?Indica el nombre DNS público del servidor}
if [[ ! "$public_host" =~ ^[a-zA-Z0-9][a-zA-Z0-9.-]+$ ]]; then
    printf '%s\n' 'El nombre DNS no es válido.' >&2
    exit 1
fi
if [[ $(id -u) != 0 ]]; then
    printf '%s\n' 'Ejecuta este instalador con sudo.' >&2
    exit 1
fi
app=$(cd -- "$(dirname -- "$0")/.." && pwd)
if [[ "$app" != /opt/moodle-mcp/app ]]; then
    printf '%s\n' 'Coloca el código en /opt/moodle-mcp/app antes de instalar.' >&2
    exit 1
fi
if [[ -f /etc/caddy/Caddyfile && ! -f /etc/moodle-mcp.env ]]; then
    printf '%s\n' 'Ya existe una configuración Caddy ajena a este proyecto. Revísala antes de continuar.' >&2
    exit 1
fi
if [[ -f /etc/moodle-mcp.env ]] && ! grep -Fxq "PUBLIC_URL=https://$public_host" /etc/moodle-mcp.env; then
    printf '%s\n' 'El dominio no coincide con la instalación existente. Revisa su configuración.' >&2
    exit 1
fi
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq python3-venv curl gnupg debian-keyring debian-archive-keyring
if [[ ! -f /usr/share/keyrings/caddy-stable-archive-keyring.gpg ]]; then
    curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/gpg.key |
        gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
fi
curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt > /etc/apt/sources.list.d/caddy-stable.list
chmod 644 /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list
apt-get update -qq
apt-get install -y -qq caddy
id moodle-mcp >/dev/null 2>&1 || useradd --system --home-dir /var/lib/moodle-mcp --shell /usr/sbin/nologin moodle-mcp
python3 -m venv /opt/moodle-mcp/venv
/opt/moodle-mcp/venv/bin/python -m pip install --disable-pip-version-check -r "$app/requirements-server.txt"
install -d -m 700 -o moodle-mcp -g moodle-mcp /var/lib/moodle-mcp
if [[ ! -f /etc/moodle-mcp.env ]]; then
    umask 027
    printf 'PUBLIC_URL=https://%s\nMOODLE_DATA_DIR=/var/lib/moodle-mcp\nMOODLE_CAMPUS_URL=https://sifo.comillas.edu\n' "$public_host" > /etc/moodle-mcp.env
    chown root:moodle-mcp /etc/moodle-mcp.env
    chmod 640 /etc/moodle-mcp.env
fi
install -m 644 "$app/deploy/moodle-mcp.service" /etc/systemd/system/moodle-mcp.service
cat > /etc/caddy/Caddyfile <<CADDY
$public_host {
    request_body {
        max_size 64KB
    }
    reverse_proxy 127.0.0.1:8000
}
CADDY
caddy validate --config /etc/caddy/Caddyfile
systemctl daemon-reload
systemctl enable --now moodle-mcp
systemctl restart moodle-mcp
systemctl reload caddy
printf 'Conector: https://%s/mcp\n' "$public_host"
