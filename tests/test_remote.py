import base64
import hashlib
import importlib.util
import re
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch
from urllib.parse import parse_qs, urlsplit

if importlib.util.find_spec("cryptography") is None:
    raise unittest.SkipTest("Instala requirements-server.txt para probar la versión remota.")

from starlette.testclient import TestClient

import moodle_mcp as moodle
from moodle_remote import SCOPE, Store, create_app


ORIGIN = "https://moodle.example"
CALLBACK = "https://chatgpt.com/connector/oauth/test-connection"
VERIFIER = "v" * 64
CHALLENGE = base64.urlsafe_b64encode(hashlib.sha256(VERIFIER.encode()).digest()).decode().rstrip("=")


class RemoteTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.directory = Path(self.temporary.name)
        self.app = create_app(ORIGIN, self.directory)
        self.client = TestClient(self.app, base_url=ORIGIN)
        self.client.__enter__()
        self.addCleanup(self.temporary.cleanup)
        self.addCleanup(self.client.__exit__, None, None, None)

    def register(self, callback=CALLBACK):
        response = self.client.post("/register", json={
            "redirect_uris": [callback], "client_name": "Test client",
            "token_endpoint_auth_method": "none", "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"], "scope": SCOPE,
        })
        self.assertEqual(response.status_code, 201, response.text)
        return response.json()["client_id"]

    def pending(self, client_id, **overrides):
        params = {"client_id": client_id, "response_type": "code", "redirect_uri": CALLBACK,
                  "code_challenge": CHALLENGE, "code_challenge_method": "S256",
                  "scope": SCOPE, "resource": ORIGIN + "/mcp", "state": "browser-state"}
        params.update(overrides)
        response = self.client.get("/authorize", params=params, follow_redirects=False)
        self.assertEqual(response.status_code, 302, response.text)
        location = response.headers["location"]
        form = self.client.get(location)
        self.assertEqual(form.status_code, 200, form.text)
        self.assertEqual(form.headers["referrer-policy"], "same-origin")
        csrf = re.search(r'name="csrf" value="([^"]+)"', form.text).group(1)
        return location, csrf

    def authorize_code(self, client_id, token="TestMoodleToken00000001", user_id=42):
        location, csrf = self.pending(client_id)
        with patch.object(moodle.MoodleClient, "call", return_value={"userid": user_id}):
            response = self.client.post(location, data={"csrf": csrf, "moodle_token": token},
                                        headers={"Origin": ORIGIN}, follow_redirects=False)
        self.assertEqual(response.status_code, 303, response.text)
        query = parse_qs(urlsplit(response.headers["location"]).query)
        self.assertEqual(query["state"], ["browser-state"])
        self.assertNotIn(token, response.headers["location"])
        return query["code"][0]

    def exchange(self, client_id, code, **overrides):
        data = {"client_id": client_id, "grant_type": "authorization_code", "code": code,
                "code_verifier": VERIFIER, "redirect_uri": CALLBACK, "resource": ORIGIN + "/mcp"}
        data.update(overrides)
        return self.client.post("/token", data=data)

    def linked(self, token="TestMoodleToken00000001", user_id=42):
        client_id = self.register()
        code = self.authorize_code(client_id, token, user_id)
        response = self.exchange(client_id, code)
        self.assertEqual(response.status_code, 200, response.text)
        return client_id, response.json()

    def test_discovery_protects_mcp_and_serves_connect_page_without_secrets(self):
        self.assertEqual(self.client.get("/health").json(), {"status": "ok"})
        discovery = self.client.get("/.well-known/oauth-authorization-server").json()
        self.assertEqual(discovery["issuer"], ORIGIN)
        self.assertEqual(discovery["code_challenge_methods_supported"], ["S256"])
        resource = self.client.get("/.well-known/oauth-protected-resource/mcp").json()
        self.assertEqual(resource["resource"], ORIGIN + "/mcp")
        response = self.client.post("/mcp", json={"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
        self.assertEqual(response.status_code, 401)
        self.assertIn("resource_metadata=", response.headers["www-authenticate"])
        self.assertEqual(self.client.get("/connect").status_code, 400)

    def test_redirects_resources_and_scopes_are_restricted(self):
        for uri in ("https://evil.example/callback", "https://chatgpt.com.evil.example/connector/oauth/id",
                    "https://chatgpt.com@evil.example/connector/oauth/id", "http://chatgpt.com/connector/oauth/id"):
            response = self.client.post("/register", json={"redirect_uris": [uri]})
            self.assertEqual(response.status_code, 400, response.text)
        client_id = self.register()
        for override in ({"resource": "https://evil.example/mcp"}, {"scope": "moodle:write"}):
            response = self.client.get("/authorize", params={
                "client_id": client_id, "response_type": "code", "redirect_uri": CALLBACK,
                "code_challenge": CHALLENGE, "code_challenge_method": "S256", **override,
            }, follow_redirects=False)
            self.assertEqual(response.status_code, 302, response.text)
            self.assertIn("error=", response.headers["location"])

    def test_login_requires_same_origin_csrf_and_does_not_echo_token(self):
        location, csrf = self.pending(self.register())
        for headers, field in (({}, csrf), ({"Origin": "null"}, csrf),
                               ({"Origin": "https://evil.example"}, csrf),
                               ({"Origin": ORIGIN}, "wrong")):
            response = self.client.post(location, data={"csrf": field, "moodle_token": "PrivateToken000000001"},
                                        headers=headers)
            self.assertEqual(response.status_code, 403)
            self.assertNotIn("PrivateToken000000001", response.text)
        with patch.object(moodle.MoodleClient, "call", side_effect=moodle.ToolError("Denied")):
            response = self.client.post(location, data={"csrf": csrf, "moodle_token": "PrivateToken000000001"},
                                        headers={"Origin": ORIGIN})
        self.assertEqual(response.status_code, 400)
        self.assertNotIn("PrivateToken000000001", response.text)
        self.assertIn("no-store", response.headers["cache-control"])

    def test_pkce_client_binding_resource_and_code_single_use(self):
        client_id = self.register()
        code = self.authorize_code(client_id)
        self.assertEqual(self.exchange(client_id, code, code_verifier="wrong").status_code, 400)
        self.assertEqual(self.exchange(self.register(), code).status_code, 400)
        self.assertEqual(self.exchange(client_id, code, resource="https://other.example/mcp").status_code, 400)
        self.assertEqual(self.exchange(client_id, code).status_code, 200)
        self.assertEqual(self.exchange(client_id, code).status_code, 400)

    def test_credentials_are_encrypted_and_connections_survive_restart(self):
        import asyncio
        from moodle_remote import MoodleOAuth
        _, tokens = self.linked()
        for file in self.directory.glob("*.sqlite3*"):
            self.assertNotIn(b"TestMoodleToken00000001", file.read_bytes())
            self.assertNotIn(tokens["access_token"].encode(), file.read_bytes())
        other = MoodleOAuth(ORIGIN, self.directory, "https://sifo.comillas.edu")
        access = asyncio.run(other.load_access_token(tokens["access_token"]))
        self.assertEqual(access.moodle_token, "TestMoodleToken00000001")
        self.assertNotIn("moodle_token", access.model_dump())
        self.assertNotIn("TestMoodleToken00000001", repr(access))

    def test_refresh_rotates_access_and_replay_revokes_grant(self):
        import asyncio
        client_id, original = self.linked()
        response = self.client.post("/token", data={"grant_type": "refresh_token", "client_id": client_id,
                                    "refresh_token": original["refresh_token"], "resource": ORIGIN + "/mcp"})
        self.assertEqual(response.status_code, 200, response.text)
        new = response.json()
        provider = self.app.state.oauth
        self.assertIsNone(asyncio.run(provider.load_access_token(original["access_token"])))
        self.assertIsNotNone(asyncio.run(provider.load_access_token(new["access_token"])))
        replay = self.client.post("/token", data={"grant_type": "refresh_token", "client_id": client_id,
                                  "refresh_token": original["refresh_token"]})
        self.assertEqual(replay.status_code, 400)
        self.assertIsNone(asyncio.run(provider.load_access_token(new["access_token"])))

    def test_revocation_and_independent_user_context_in_mcp(self):
        first_id, first = self.linked("FirstUserMoodleToken0001", 11)
        _, second = self.linked("SecondUserMoodleToken002", 22)
        def result(instance, function, **params):
            return {"userid": 11 if instance.token.startswith("First") else 22,
                    "fullname": "First" if instance.token.startswith("First") else "Second"}
        for tokens, expected in ((first, "First"), (second, "Second")):
            headers = {"Authorization": "Bearer " + tokens["access_token"],
                       "Accept": "application/json, text/event-stream"}
            with patch.object(moodle.MoodleClient, "call", autospec=True, side_effect=result):
                response = self.client.post("/mcp", headers=headers, json={
                    "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": {"name": "moodle_status", "arguments": {}},
                })
            self.assertEqual(response.status_code, 200, response.text)
            self.assertEqual(response.json()["result"]["structuredContent"]["user"], expected)
            self.assertNotIn("MoodleToken", response.text)
        response = self.client.post("/revoke", data={"client_id": first_id,
                                    "token": first["refresh_token"]})
        self.assertEqual(response.status_code, 200, response.text)
        response = self.client.get("/mcp", headers={"Authorization": "Bearer " + first["access_token"]})
        self.assertEqual(response.status_code, 401)
        import asyncio
        self.assertIsNotNone(asyncio.run(self.app.state.oauth.load_access_token(second["access_token"])))

    def test_body_size_and_rate_limit(self):
        self.assertEqual(self.client.post("/connect", content=b"x" * 16385).status_code, 413)
        statuses = [self.client.get("/connect").status_code for _ in range(32)]
        self.assertIn(429, statuses)

    def test_store_consumes_codes_atomically_and_expires_records(self):
        import time
        store = Store(self.directory)
        store.put("test", "single-use", {"value": 1}, time.time() + 60)
        with ThreadPoolExecutor(max_workers=4) as workers:
            values = list(workers.map(lambda _: store.get("test", "single-use", consume=True), range(4)))
        self.assertEqual(sum(item is not None for item in values), 1)
        store.put("test", "expired", {"value": 1}, time.time() - 1)
        self.assertIsNone(store.get("test", "expired"))


if __name__ == "__main__":
    unittest.main()
