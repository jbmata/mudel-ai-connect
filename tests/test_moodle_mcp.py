import json
import sys
import unittest
from io import BytesIO
from pathlib import Path
from unittest.mock import patch
from urllib.parse import parse_qs

import httpx
from mcp import Client, StdioServerParameters
from mcp.server.mcpserver.exceptions import ToolError
from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

import moodle_mcp as server

BASE = "https://campus.example/moodle"
TOKEN = "secret123"
FILE_URL = BASE + "/webservice/pluginfile.php/42/mod_resource/content/1/apuntes.txt"


def make_pdf() -> bytes:
    writer = PdfWriter()
    page = writer.add_blank_page(width=200, height=200)
    font = DictionaryObject({NameObject("/Type"): NameObject("/Font"),
                             NameObject("/Subtype"): NameObject("/Type1"),
                             NameObject("/BaseFont"): NameObject("/Helvetica")})
    page[NameObject("/Resources")] = DictionaryObject({
        NameObject("/Font"): DictionaryObject({NameObject("/F1"): writer._add_object(font)})})
    stream = DecodedStreamObject()
    stream.set_data(b"BT /F1 12 Tf 10 100 Td (Apuntes de prueba) Tj ET")
    page[NameObject("/Contents")] = writer._add_object(stream)
    buffer = BytesIO()
    writer.write(buffer)
    return buffer.getvalue()


class MoodleTests(unittest.TestCase):
    def client(self, handler):
        return server.MoodleClient(BASE, TOKEN, transport=httpx.MockTransport(handler))

    def test_configuration_requires_https_and_token(self):
        for url, token in [(BASE, ""), ("http://campus.example", TOKEN),
                           ("https://user:pass@campus.example", TOKEN),
                           (BASE + "?token=anything", TOKEN)]:
            with self.subTest(url=url), self.assertRaises(ToolError):
                server.MoodleClient(url, token)

    def test_rest_uses_post_and_moodle_array_parameters(self):
        def handler(request):
            self.assertEqual(request.method, "POST")
            self.assertEqual(str(request.url), BASE + "/webservice/rest/server.php")
            form = parse_qs(request.content.decode())
            self.assertEqual(form["wstoken"], [TOKEN])
            self.assertEqual(form["courseids[0]"], ["7"])
            return httpx.Response(200, json={"courses": []})

        self.assertEqual(self.client(handler).call("mod_assign_get_assignments", **{"courseids[0]": 7}), {"courses": []})

    def test_write_functions_never_reach_network(self):
        def handler(request):
            self.fail("No debe haber petición para una función de escritura")

        with self.assertRaises(ToolError):
            self.client(handler).call("core_course_delete_courses")

    def test_moodle_errors_do_not_echo_secrets_or_server_messages(self):
        client = self.client(lambda request: httpx.Response(200, json={
            "exception": "moodle_exception", "errorcode": "invalidtoken", "message": TOKEN}))
        with self.assertRaises(ToolError) as caught:
            client.call("core_webservice_get_site_info")
        self.assertIn("invalidtoken", str(caught.exception))
        self.assertNotIn(TOKEN, str(caught.exception))

    def test_redirects_html_and_http_errors_are_clear(self):
        for response in [httpx.Response(302, headers={"location": "https://other.example"}),
                         httpx.Response(200, text="<html>Log in</html>"), httpx.Response(403)]:
            with self.subTest(status=response.status_code), self.assertRaises(ToolError):
                self.client(lambda request: response).call("core_webservice_get_site_info")

    def test_redacts_tokens_from_nested_output(self):
        result = self.client(lambda request: None).clean({
            "items": [TOKEN, "https://example.test/file?token=anothersecret&x=1"],
            "privatetoken": "privatevalue"})
        self.assertNotIn(TOKEN, json.dumps(result))
        self.assertNotIn("anothersecret", json.dumps(result))
        self.assertNotIn("privatetoken", result)

    def test_file_validation_blocks_other_hosts_and_endpoints(self):
        def handler(request):
            self.fail("No se debe enviar el token a una URL no permitida")

        client = self.client(handler)
        for url in ["https://other.example/webservice/pluginfile.php/1/a.txt",
                    "http://campus.example/moodle/webservice/pluginfile.php/1/a.txt",
                    BASE + "/login/index.php", BASE + "/webservice/rest/server.php",
                    BASE + "/webservice/pluginfile.php/../rest/server.php",
                    BASE + "/webservice/pluginfile.php/%2e%2e/rest/server.php"]:
            with self.subTest(url=url), self.assertRaises(ToolError):
                client.download(url)

    def test_file_download_with_slash_arguments_disabled(self):
        def handler(request):
            self.assertEqual(request.url.params["file"], "/42/content/apuntes.txt")
            self.assertEqual(parse_qs(request.content.decode()), {"token": [TOKEN]})
            return httpx.Response(200, content=b"Apuntes")

        body, _ = self.client(handler).download(BASE + "/webservice/pluginfile.php?file=%2F42%2Fcontent%2Fapuntes.txt")
        self.assertEqual(body, b"Apuntes")

    def test_response_size_is_limited_during_download(self):
        with patch.object(server, "MAX_BYTES", 4), self.assertRaises(ToolError):
            self.client(lambda request: httpx.Response(200, content=b"12345")).download(FILE_URL)

    def test_html_text_preserves_words_and_removes_scripts(self):
        self.assertEqual(server.text("<p>Uno &amp; dos</p><script>malicioso()</script><p>Tres</p>"), "Uno & dos\nTres")

    def test_course_sections_are_paginated_and_keep_file_indices(self):
        contents = [{"id": i, "name": f"Tema {i}", "modules": [{
            "id": 90, "name": "Apuntes", "contents": [
                {"type": "url", "fileurl": "https://example.test"},
                {"type": "file", "filename": "apuntes.txt", "fileurl": FILE_URL}]}]}
                    for i in range(4)]
        with patch.object(server, "get_client", return_value=self.client(lambda request: httpx.Response(200, json=contents))):
            result = server.get_course_context(7)
            self.assertEqual(len(result["sections"]), 3)
            self.assertEqual(result["next_section_start"], 3)
            self.assertEqual(result["sections"][0]["modules"][0]["contents"][1]["file_index"], 1)
            last = server.get_course_context(7, section_start=3)
            self.assertIsNone(last["next_section_start"])

    def read_file(self, name, body, content_type="text/plain", offset=0):
        def handler(request):
            if request.url.path.endswith("/rest/server.php"):
                return httpx.Response(200, json=[{"id": 1, "name": "Tema", "modules": [{
                    "id": 90, "url": BASE + "/mod/resource/view.php?id=90", "contents": [{
                        "type": "file", "filename": name, "fileurl": FILE_URL + "?token=old"}]}]}])
            self.assertEqual(parse_qs(request.content.decode()), {"token": [TOKEN]})
            self.assertNotIn("token", request.url.query.decode())
            return httpx.Response(200, content=body, headers={"content-type": content_type})

        with patch.object(server, "get_client", return_value=self.client(handler)):
            return server.read_course_file(7, 90, offset=offset)

    def test_reads_text_with_continuation_and_no_token_in_output(self):
        result = self.read_file("apuntes.txt", b"a" * 13000)
        self.assertEqual(len(result["text"]), 12000)
        self.assertEqual(result["next_offset"], 12000)
        self.assertNotIn("token=old", result["source_url"])
        self.assertEqual(len(self.read_file("apuntes.txt", b"a" * 13000, offset=12000)["text"]), 1000)

    def test_pdf_text_is_extracted_and_invalid_pdf_is_rejected(self):
        result = self.read_file("apuntes.pdf", make_pdf(), "application/pdf")
        self.assertIn("Apuntes de prueba", result["text"])
        with self.assertRaises(ToolError):
            self.read_file("apuntes.pdf", b"<html>Login</html>", "text/html")

    def test_missing_module_never_downloads_a_file(self):
        client = self.client(lambda request: httpx.Response(200, json=[]))
        with patch.object(server, "get_client", return_value=client), self.assertRaises(ToolError):
            server.read_course_file(7, 999)


class ProtocolTests(unittest.IsolatedAsyncioTestCase):
    async def test_tools_return_moodle_context_through_mcp(self):
        def handler(request):
            function = parse_qs(request.content.decode())["wsfunction"][0]
            responses = {
                "core_webservice_get_site_info": {"userid": 3, "fullname": "Usuario", "functions": [
                    {"name": name} for name in server.FUNCTIONS], "downloadfiles": 1},
                "core_enrol_get_users_courses": [{"id": 7, "fullname": "Cálculo", "summary": "<p>Derivadas</p>"}],
                "mod_assign_get_assignments": {"courses": [{"assignments": [{
                    "id": 4, "cmid": 90, "name": "Ejercicios", "intro": "<p>Derivar</p>", "duedate": 1700000000}]}]},
                "core_calendar_get_calendar_upcoming_view": {"events": [{
                    "id": 5, "name": "Entrega", "timestart": 1700000000, "course": {"fullname": "Cálculo"}}]},
            }
            return httpx.Response(200, json=responses[function])

        moodle = server.MoodleClient(BASE, TOKEN, transport=httpx.MockTransport(handler))
        with patch.object(server, "get_client", return_value=moodle):
            async with Client(server.mcp) as client:
                status = await client.call_tool("moodle_status")
                self.assertEqual(status.structured_content["missing_functions"], [])
                courses = await client.call_tool("list_courses")
                self.assertEqual(courses.structured_content["courses"][0]["name"], "Cálculo")
                tasks = await client.call_tool("list_assignments", {"course_id": 7})
                self.assertEqual(tasks.structured_content["assignments"][0]["description"], "Derivar")
                self.assertTrue(tasks.structured_content["assignments"][0]["due_at_utc"].endswith("+00:00"))
                events = await client.call_tool("get_upcoming_events")
                self.assertEqual(events.structured_content["events"][0]["course"], "Cálculo")

    async def test_mcp_discovery_validation_and_errors(self):
        with patch.dict(server.os.environ, {"MOODLE_URL": BASE, "MOODLE_TOKEN": ""}):
            async with Client(server.mcp) as client:
                tools = (await client.list_tools()).tools
                self.assertEqual({tool.name for tool in tools}, {
                    "moodle_status", "list_courses", "get_course_context", "list_assignments",
                    "get_upcoming_events", "read_course_file"})
                self.assertTrue(all(tool.annotations.read_only_hint for tool in tools))
                missing = await client.call_tool("moodle_status")
                self.assertTrue(missing.is_error)
                self.assertIn("MOODLE_TOKEN", missing.content[0].text)
                invalid = await client.call_tool("get_course_context", {"course_id": -1})
                self.assertTrue(invalid.is_error)
                self.assertIn("estudiar_curso", [prompt.name for prompt in (await client.list_prompts()).prompts])

    async def test_real_stdio_process_initializes_without_credentials(self):
        params = StdioServerParameters(
            command=sys.executable,
            args=[str(Path(server.__file__).resolve())],
            env={"MOODLE_URL": BASE, "MOODLE_TOKEN": "", "PYTHONIOENCODING": "utf-8"},
        )
        async with Client(params) as client:
            self.assertEqual(len((await client.list_tools()).tools), 6)
            result = await client.call_tool("moodle_status")
            self.assertTrue(result.is_error)
            self.assertIn("MOODLE_TOKEN", result.content[0].text)


if __name__ == "__main__":
    unittest.main()
