"""MCP HTTPS con autorización OAuth y credenciales Moodle por conexión."""

from __future__ import annotations

import asyncio
import hashlib
import html
import json
import logging
import os
import re
import secrets
import sqlite3
import time
from collections import OrderedDict
from contextlib import closing
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from cryptography.fernet import Fernet
from mcp.server import MCPServer
from mcp.server.auth.provider import (
    AuthorizationCode, AuthorizationParams, AuthorizeError,
    OAuthAuthorizationServerProvider, RegistrationError, RefreshToken, TokenError,
    construct_redirect_uri,
)
from mcp.server.auth.settings import AuthSettings, ClientRegistrationOptions, RevocationOptions
from mcp.server.transport_security import TransportSecuritySettings
from mcp.shared.auth import OAuthClientInformationFull, OAuthToken
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.requests import Request
from starlette.responses import FileResponse, HTMLResponse, JSONResponse, RedirectResponse, Response

import moodle_mcp as moodle

SCOPE = "moodle:read"
COOKIE = "__Host-moodle-csrf"
DAY = 86400
ROOT = Path(__file__).resolve().parent


def digest(value: str) -> str:
    return hashlib.sha256(value.encode()).hexdigest()


class Store:
    """Registros cifrados; los identificadores de tokens se guardan como hashes."""

    def __init__(self, directory: Path):
        directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.path = directory / "accounts.sqlite3"
        key_path = directory / "encryption.key"
        if not key_path.exists():
            if self.path.exists():
                raise ValueError("Falta la clave de cifrado de la base de datos existente.")
            descriptor = os.open(key_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
            with os.fdopen(descriptor, "wb") as stream:
                stream.write(Fernet.generate_key())
        self.cipher = Fernet(key_path.read_bytes())
        with closing(sqlite3.connect(self.path)) as db, db:
            db.execute("CREATE TABLE IF NOT EXISTS records "
                       "(kind TEXT, key TEXT, expires REAL, value BLOB, PRIMARY KEY(kind,key))")
        self.path.chmod(0o600)

    def put(self, kind: str, key: str, value: dict, expires: float | None) -> None:
        encrypted = self.cipher.encrypt(json.dumps(value).encode())
        with closing(sqlite3.connect(self.path)) as db, db:
            db.execute("DELETE FROM records WHERE expires <= ?", (time.time(),))
            db.execute("INSERT OR REPLACE INTO records VALUES (?,?,?,?)",
                       (kind, digest(key), expires, encrypted))

    def get(self, kind: str, key: str, *, consume: bool = False) -> dict | None:
        with closing(sqlite3.connect(self.path)) as db, db:
            if consume:
                row = db.execute("DELETE FROM records WHERE kind=? AND key=? "
                                 "AND (expires IS NULL OR expires>?) RETURNING value",
                                 (kind, digest(key), time.time())).fetchone()
            else:
                row = db.execute("SELECT value FROM records WHERE kind=? AND key=? "
                                 "AND (expires IS NULL OR expires>?)",
                                 (kind, digest(key), time.time())).fetchone()
        return json.loads(self.cipher.decrypt(row[0])) if row else None

    def delete(self, kind: str, key: str) -> None:
        with closing(sqlite3.connect(self.path)) as db, db:
            db.execute("DELETE FROM records WHERE kind=? AND key=?", (kind, digest(key)))

    def client_count(self) -> int:
        with closing(sqlite3.connect(self.path)) as db, db:
            return db.execute("SELECT count(*) FROM records WHERE kind='client'").fetchone()[0]


class MoodleOAuth(OAuthAuthorizationServerProvider):
    def __init__(self, public_url: str, directory: Path, campus_url: str):
        parsed = urlsplit(public_url)
        if (parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password
                or parsed.path not in {"", "/"} or parsed.query or parsed.fragment):
            raise ValueError("PUBLIC_URL debe ser un origen HTTPS sin ruta ni credenciales.")
        self.public_url = public_url.rstrip("/")
        self.resource = self.public_url + "/mcp"
        # El campus lo fija el administrador; el formulario no acepta destinos arbitrarios.
        self.campus_url = moodle.MoodleClient(campus_url, "ValidationOnly").base_url
        self.store = Store(directory)

    @staticmethod
    def valid_redirect(uri: str) -> bool:
        parsed = urlsplit(uri)
        if (parsed.scheme != "https" or parsed.username is not None or parsed.password is not None
                or parsed.fragment or parsed.query):
            return False
        if parsed.netloc == "chatgpt.com":
            return (parsed.path == "/connector_platform_oauth_redirect"
                    or re.fullmatch(r"/connector/oauth/[A-Za-z0-9_-]+", parsed.path) is not None)
        return parsed.netloc == "claude.ai" and parsed.path == "/api/mcp/auth_callback"

    async def get_client(self, client_id: str):
        value = self.store.get("client", client_id)
        return OAuthClientInformationFull.model_validate(value) if value else None

    async def register_client(self, client_info: OAuthClientInformationFull) -> None:
        if (not client_info.redirect_uris or len(client_info.redirect_uris) > 10
                or any(not self.valid_redirect(str(uri)) for uri in client_info.redirect_uris)):
            raise RegistrationError("invalid_redirect_uri", "Usa el callback oficial de ChatGPT o Claude.")
        if self.store.client_count() >= 1000:
            raise RegistrationError("invalid_client_metadata", "Se ha alcanzado el límite de conexiones.")
        self.store.put("client", client_info.client_id, client_info.model_dump(mode="json"), None)

    async def authorize(self, client, params: AuthorizationParams) -> str:
        if not self.valid_redirect(str(params.redirect_uri)):
            raise AuthorizeError("invalid_request", "Callback no permitido.")
        if params.resource not in {None, self.resource}:
            raise AuthorizeError("invalid_target", "Recurso no permitido.")
        if set(params.scopes or [SCOPE]) != {SCOPE}:
            raise AuthorizeError("invalid_scope", "Solo se permite consultar Moodle.")
        if not re.fullmatch(r"[A-Za-z0-9_-]{43}", params.code_challenge):
            raise AuthorizeError("invalid_request", "Se necesita PKCE S256.")
        request_id = secrets.token_urlsafe(32)
        expires = time.time() + 600
        self.store.put("pending", request_id, {
            "client_id": client.client_id, "params": params.model_dump(mode="json"),
            "expires": expires,
        }, expires)
        return self.public_url + "/connect?request=" + request_id

    async def load_authorization_code(self, client, authorization_code: str):
        value = self.store.get("code", authorization_code)
        if not value or value["client_id"] != client.client_id:
            return None
        return AuthorizationCode.model_validate(value)

    def issue(self, grant_id: str, grant: dict) -> OAuthToken:
        access, refresh = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
        now = int(time.time())
        expires = min(now + 3600, int(grant["expires"]))
        common = {"client_id": grant["client_id"], "scopes": [SCOPE],
                  "resource": self.resource, "subject": grant["subject"],
                  "grant_id": grant_id, "version": grant["version"]}
        self.store.put("access", access, {**common, "expires_at": expires}, expires)
        self.store.put("refresh", refresh, {**common, "expires_at": grant["expires"]}, grant["expires"])
        return OAuthToken(access_token=access, refresh_token=refresh, token_type="Bearer",
                          expires_in=expires - now, scope=SCOPE)

    async def exchange_authorization_code(self, client, authorization_code):
        if authorization_code.client_id != client.client_id:
            raise TokenError("invalid_grant", "Código no válido.")
        value = self.store.get("code", authorization_code.code, consume=True)
        if not value or value["client_id"] != client.client_id:
            raise TokenError("invalid_grant", "Código caducado o utilizado.")
        grant_id = secrets.token_urlsafe(32)
        grant = {"client_id": client.client_id, "subject": value["subject"],
                 "moodle_token": value["moodle_token"], "moodle_url": self.campus_url,
                 "version": secrets.token_hex(16),
                 "expires": int(time.time()) + 30 * DAY}
        self.store.put("grant", grant_id, grant, grant["expires"])
        return self.issue(grant_id, grant)

    def valid_grant(self, value: dict | None) -> dict | None:
        if not value:
            return None
        grant = self.store.get("grant", value["grant_id"])
        if (not grant or grant["moodle_url"] != self.campus_url or grant["client_id"] != value["client_id"]
                or grant["version"] != value["version"]):
            return None
        return grant

    async def load_refresh_token(self, client, refresh_token: str):
        spent = self.store.get("spent-refresh", refresh_token)
        if spent and spent["client_id"] == client.client_id:
            self.store.delete("grant", spent["grant_id"])
            return None
        value = self.store.get("refresh", refresh_token)
        if not self.valid_grant(value) or value["client_id"] != client.client_id:
            return None
        return RefreshToken.model_validate({**value, "token": refresh_token})

    async def exchange_refresh_token(self, client, refresh_token, scopes):
        if set(scopes) != {SCOPE} or refresh_token.client_id != client.client_id:
            raise TokenError("invalid_scope", "Permisos no válidos.")
        value = self.store.get("refresh", refresh_token.token, consume=True)
        grant = self.valid_grant(value)
        if not grant or value["client_id"] != client.client_id:
            raise TokenError("invalid_grant", "Conexión caducada o revocada.")
        self.store.put("spent-refresh", refresh_token.token, value, value["expires_at"])
        grant["version"] = secrets.token_hex(16)
        self.store.put("grant", value["grant_id"], grant, grant["expires"])
        return self.issue(value["grant_id"], grant)

    async def load_access_token(self, token: str):
        value = self.store.get("access", token)
        grant = self.valid_grant(value)
        if not grant:
            return None
        return moodle.MoodleAccessToken.model_validate({
            **value, "token": token, "claims": {"iss": self.public_url},
            "moodle_url": grant["moodle_url"], "moodle_token": grant["moodle_token"],
        })

    async def revoke_token(self, token) -> None:
        value = self.store.get("access", token.token) or self.store.get("refresh", token.token)
        if value:
            self.store.delete("grant", value["grant_id"])

    async def connect(self, request: Request) -> Response:
        request_id = request.query_params.get("request", "")
        pending = self.store.get("pending", request_id)
        if not pending:
            return page("Enlace caducado", "Vuelve a conectar Moodle desde tu aplicación de IA.", 400)
        if request.method == "GET":
            csrf = secrets.token_urlsafe(32)
            pending["csrf"] = digest(csrf)
            self.store.put("pending", request_id, pending, pending["expires"])
            response = self.form(request_id, pending, csrf)
            response.set_cookie(COOKIE, csrf, max_age=600, secure=True, httponly=True,
                                samesite="lax", path="/")
            return response
        if request.headers.get("origin") != self.public_url:
            return page("Petición no válida", "Vuelve a conectar desde tu aplicación de IA.", 403)
        if request.headers.get("content-type", "").split(";")[0] != "application/x-www-form-urlencoded":
            return Response(status_code=415)
        try:
            form = parse_qs((await request.body()).decode("utf-8", errors="replace"), max_num_fields=5)
        except ValueError:
            return Response(status_code=400)
        if any(len(values) != 1 for values in form.values()):
            return Response(status_code=400)
        csrf = form.get("csrf", [""])[0]
        if (not csrf or not secrets.compare_digest(csrf, request.cookies.get(COOKIE, ""))
                or not secrets.compare_digest(digest(csrf), pending.get("csrf", ""))):
            return page("Petición no válida", "Recarga el formulario e inténtalo de nuevo.", 403)
        token = form.get("moodle_token", [""])[0].strip()
        if not re.fullmatch(r"[A-Za-z0-9]{16,512}", token):
            return self.form(request_id, pending, csrf, "Comprueba la clave de servicios web de Moodle.", 400)
        try:
            client = moodle.MoodleClient(self.campus_url, token)
            info = await asyncio.to_thread(client.call, "core_webservice_get_site_info")
            if not isinstance(info.get("userid"), int) or info["userid"] <= 0:
                raise moodle.ToolError("Cuenta no válida.")
        except moodle.ToolError:
            return self.form(request_id, pending, csrf, "Moodle no ha validado la clave. Revisa la clave y sus permisos.", 400)
        if not self.store.get("pending", request_id, consume=True):
            return page("Enlace utilizado", "Vuelve a conectar desde tu aplicación de IA.", 400)
        params = AuthorizationParams.model_validate(pending["params"])
        code, expires = secrets.token_urlsafe(32), time.time() + 300
        subject = digest(self.campus_url + ":" + str(info["userid"]))
        self.store.put("code", code, {
            "code": code, "client_id": pending["client_id"], "scopes": [SCOPE],
            "expires_at": expires, "code_challenge": params.code_challenge,
            "redirect_uri": str(params.redirect_uri), "resource": self.resource,
            "redirect_uri_provided_explicitly": params.redirect_uri_provided_explicitly,
            "subject": subject, "moodle_token": token,
        }, expires)
        response = RedirectResponse(construct_redirect_uri(str(params.redirect_uri), code=code, state=params.state), 303)
        response.delete_cookie(COOKIE, path="/", secure=True, httponly=True, samesite="lax")
        return response

    def form(self, request_id: str, pending: dict, csrf: str, error: str = "", status: int = 200):
        host = urlsplit(pending["params"]["redirect_uri"]).hostname
        application = "ChatGPT" if host == "chatgpt.com" else "Claude"
        campus = html.escape(urlsplit(self.campus_url).hostname)
        message = f'<p role="alert">{html.escape(error)}</p>' if error else ""
        content = f"""<p>Conecta tu cuenta de <strong>{campus}</strong> con {application}.</p>
<p>Permitirás consultar tus asignaturas, materiales y fechas. El plugin no realiza entregas.</p>
{message}<form method="post" action="/connect?request={html.escape(request_id, quote=True)}">
<input type="hidden" name="csrf" value="{html.escape(csrf, quote=True)}">
<label for="token">Clave de servicios web de Moodle</label>
<input id="token" name="moodle_token" type="password" autocomplete="off" required maxlength="512">
<p><a href="{html.escape(self.campus_url, quote=True)}/user/managetoken.php" target="_blank" rel="noopener noreferrer">Ver mis claves en Moodle</a></p>
<p>Introduce la clave del servicio Moodle mobile web service. Tu contraseña de la universidad no se utiliza aquí.</p>
<p>La clave se guarda cifrada en este servidor para atender esta conexión. No se comparte con el modelo de IA.</p>
<button type="submit">Autorizar consultas en {application}</button></form>"""
        return page("Conectar Moodle", content, status, trusted_html=True)


def page(title: str, content: str, status: int = 200, *, trusted_html: bool = False):
    body = content if trusted_html else f"<p>{html.escape(content)}</p>"
    return HTMLResponse(f"""<!doctype html><html lang="es"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>{html.escape(title)}</title>
<style>body{{font:17px/1.5 system-ui;background:#f6f6f9;color:#18181c;margin:0;padding:24px}}
main{{max-width:540px;margin:24px auto;background:white;padding:32px;border-radius:24px}}
img{{width:100px;display:block;margin:auto}}h1{{font-size:28px}}input{{box-sizing:border-box;width:100%;padding:12px;margin:8px 0}}
button{{background:#ed0086;color:white;border:0;border-radius:10px;padding:14px 20px;font:inherit;cursor:pointer}}
a{{color:#9e005a}}code{{overflow-wrap:anywhere}}[role=alert]{{color:#a00032}}</style>
<main><img src="/logo.png" alt="Moodle"><h1>{html.escape(title)}</h1>{body}</main></html>""", status)


class RequestGuards(BaseHTTPMiddleware):
    """Límites del formulario, cabeceras y frecuencia de intentos de conexión."""

    def __init__(self, app, resource: str):
        super().__init__(app)
        self.attempts = OrderedDict()
        self.resource = resource

    async def dispatch(self, request, call_next):
        if request.url.path in {"/connect", "/register", "/token", "/revoke", "/authorize"}:
            address = request.client.host if request.client else "unknown"
            key = (address, request.url.path)
            now = time.monotonic()
            for expired in list(self.attempts):
                if now - self.attempts[expired][0] >= 60:
                    del self.attempts[expired]
            window, count = self.attempts.get(key, (now, 0))
            if count >= 30 or (key not in self.attempts and len(self.attempts) >= 4096):
                return Response("Demasiados intentos. Espera un minuto.", 429, headers={"Retry-After": "60"})
            self.attempts[key] = (window, count + 1)
        if request.method == "POST" and request.url.path != "/mcp":
            body = bytearray()
            async for chunk in request.stream():
                body.extend(chunk)
                if len(body) > 16384:
                    return Response(status_code=413)
            if request.url.path in {"/token", "/revoke"}:
                try:
                    fields = parse_qs(body.decode("utf-8", errors="replace"), keep_blank_values=True,
                                      max_num_fields=20)
                except ValueError:
                    return JSONResponse({"error": "invalid_request"}, 400)
                if request.url.path == "/token" and fields.get("resource", [self.resource]) != [self.resource]:
                    return JSONResponse({"error": "invalid_target"}, 400)
                if request.url.path == "/revoke" and "client_secret" not in fields:
                    # shortcut: SDK 2.3 exige este campo opcional; retirar al corregirse en el SDK.
                    body.extend(b"&client_secret=")
            request._body = bytes(body)
        response = await call_next(request)
        response.headers.update({
            # no-referrer convierte Origin en null al enviar formularios HTML.
            "Cache-Control": "no-store", "Referrer-Policy": "same-origin",
            "X-Content-Type-Options": "nosniff", "X-Frame-Options": "DENY",
            "Content-Security-Policy": "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; "
                                       "form-action 'self' https://chatgpt.com https://claude.ai; "
                                       "frame-ancestors 'none'; base-uri 'none'",
        })
        return response


def create_app(public_url: str, directory: Path, campus_url: str = "https://sifo.comillas.edu"):
    provider = MoodleOAuth(public_url, directory, campus_url)
    server = MCPServer("moodle", instructions=moodle.INSTRUCTIONS,
        auth_server_provider=provider, log_level="WARNING",
        auth=AuthSettings(issuer_url=provider.public_url,
            resource_server_url=provider.resource, validate_token_resource=True,
            required_scopes=[SCOPE],
            client_registration_options=ClientRegistrationOptions(
                enabled=True, valid_scopes=[SCOPE], default_scopes=[SCOPE]),
            revocation_options=RevocationOptions(enabled=True)))
    for name in ("moodle_status", "list_courses", "get_course_context", "list_assignments",
                 "get_upcoming_events", "read_course_file"):
        server.tool(annotations=moodle.READ_ONLY)(getattr(moodle, name))
    server.prompt()(moodle.estudiar_curso)
    server.custom_route("/connect", methods=["GET", "POST"])(provider.connect)

    @server.custom_route("/", methods=["GET"])
    async def home(request):
        return page("Tus asignaturas, en contexto", """<p>Consulta materiales, estudia con tus apuntes y organiza tus tareas desde tu IA.</p>
<p>Para empezar, añade el conector Moodle en ChatGPT o Claude y sigue el proceso de conexión de tu cuenta.</p>""", trusted_html=True)

    @server.custom_route("/logo.png", methods=["GET"])
    async def logo(request):
        return FileResponse(ROOT / "assets/logo.png", media_type="image/png")

    @server.custom_route("/health", methods=["GET"])
    async def health(request):
        return JSONResponse({"status": "ok"})

    app = server.streamable_http_app(json_response=True, stateless_http=True,
        max_request_body_size=65536,
        transport_security=TransportSecuritySettings(
            allowed_hosts=[urlsplit(provider.public_url).netloc],
            allowed_origins=[provider.public_url, "https://chatgpt.com", "https://claude.ai"]))
    app.add_middleware(RequestGuards, resource=provider.resource)
    app.state.oauth = provider
    return app


if __name__ == "__main__":
    import uvicorn
    app = create_app(os.environ["PUBLIC_URL"], Path(os.environ["MOODLE_DATA_DIR"]),
                     os.environ.get("MOODLE_CAMPUS_URL", "https://sifo.comillas.edu"))
    logging.getLogger().setLevel(logging.WARNING)
    uvicorn.run(app, host="127.0.0.1", port=8000, access_log=False,
                log_level="warning", proxy_headers=True, forwarded_allow_ips="127.0.0.1")
