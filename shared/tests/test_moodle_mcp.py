import json
import os
import subprocess
import sys
import tempfile
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

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "shared"))

import moodle_mcp as server  # noqa: E402
import plugin_runtime as runtime  # noqa: E402  (moodle_mcp añade local/ a la ruta)

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


class DownloadTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.downloads = Path(directory.name) / "Descargas"
        self.downloads.mkdir()
        for patcher in (patch.object(runtime, "downloads_dir", return_value=self.downloads),
                        patch.object(runtime, "data_dir", return_value=Path(directory.name) / "estado"),
                        patch.object(server, "require_permission")):
            patcher.start()
            self.addCleanup(patcher.stop)
        self.files_requested = 0

    def download(self, name="Tema 1.pdf", body=b"%PDF-1.7 apuntes", content_type="application/pdf",
                 course="Control <b>Avanzado</b>", course_id=7, filesize=None):
        def handler(request):
            if request.url.path.endswith("/rest/server.php"):
                function = parse_qs(request.content.decode())["wsfunction"][0]
                return httpx.Response(200, json={
                    "core_webservice_get_site_info": {"userid": 3},
                    "core_enrol_get_users_courses": [{"id": 7, "fullname": course}],
                    "core_course_get_contents": [{"id": 1, "name": "Tema", "modules": [{
                        "id": 90, "url": BASE + "/mod/resource/view.php?id=90", "contents": [{
                            "type": "file", "filename": name, "filesize": filesize or len(body),
                            "fileurl": FILE_URL + "?forcedownload=1&token=old"}]}]}],
                }[function])
            self.files_requested += 1
            self.assertEqual(parse_qs(request.content.decode()), {"token": [TOKEN]})
            self.assertNotIn("token", request.url.query.decode())
            return httpx.Response(200, content=body, headers={"content-type": content_type})

        client = server.MoodleClient(BASE, TOKEN, transport=httpx.MockTransport(handler))
        with patch.object(server, "get_client", return_value=client):
            return server.download_course_file(course_id, 90)

    def saved(self):
        return sorted(str(path.relative_to(self.downloads)) for path in self.downloads.rglob("*") if path.is_file())

    def test_saves_any_format_in_a_course_folder_without_the_token(self):
        result = self.download("Diapositivas.pptx", b"PK\x03\x04 diapositivas", "application/octet-stream")
        expected = self.downloads / "Mudel AI Connect" / "Control Avanzado" / "Diapositivas.pptx"
        self.assertEqual(Path(result["path"]), expected)
        self.assertEqual(expected.read_bytes(), b"PK\x03\x04 diapositivas")
        self.assertFalse(result["already_downloaded"])
        self.assertNotIn(TOKEN, json.dumps(result))
        self.assertNotIn("token=old", json.dumps(result))
        self.assertEqual(self.saved(), [str(Path("Mudel AI Connect/Control Avanzado/Diapositivas.pptx"))])

    def test_repeating_reuses_the_file_and_never_overwrites_a_different_one(self):
        first = self.download(body=b"%PDF- v1")
        again = self.download(body=b"%PDF- v1")
        self.assertEqual(again["path"], first["path"])
        self.assertTrue(again["already_downloaded"])
        changed = self.download(body=b"%PDF- v2")
        self.assertEqual(Path(changed["path"]).name, "Tema 1 (1).pdf")
        self.assertEqual(Path(first["path"]).read_bytes(), b"%PDF- v1")
        self.assertEqual(len(self.saved()), 2)

    def test_hostile_names_stay_inside_the_course_folder(self):
        for name, expected in [("../../../Startup/x.pdf", "_.._.._Startup_x.pdf"), (".bashrc", "bashrc"), ("CON.txt", "_CON.txt"),
                               ("..", "archivo"), ('a<b>:"c"?.txt', "a_b___c__.txt"), ("nota. ", "nota")]:
            with self.subTest(name=name):
                path = Path(self.download(name, b"texto")["path"])
                self.assertEqual(path.parent, self.downloads / "Mudel AI Connect" / "Control Avanzado")
                self.assertEqual(path.name, expected)
        folder = Path(self.download(course="../Windows")["path"]).parent
        self.assertEqual(folder.parent, self.downloads / "Mudel AI Connect")

    def test_programs_courses_of_others_and_big_files_are_refused_before_downloading(self):
        for kwargs, message in [({"name": "setup.exe"}, "programas"), ({"name": "apuntes.PDF.js"}, "programas"),
                                ({"course_id": 8}, "no está entre"),
                                ({"filesize": server.MAX_DOWNLOAD_BYTES + 1}, "200 MiB")]:
            with self.subTest(kwargs=kwargs), self.assertRaisesRegex(ToolError, message):
                self.download(**kwargs)
        self.assertEqual(self.files_requested, 0)
        self.assertEqual(self.saved(), [])

    def test_failed_downloads_leave_nothing_behind(self):
        with self.assertRaisesRegex(ToolError, "permiso de descarga"):
            self.download(body=b'{"error": "nopermissions"}', content_type="application/json")
        with patch.object(server, "MAX_DOWNLOAD_BYTES", 4), self.assertRaisesRegex(ToolError, "límite"):
            self.download(body=b"12345", filesize=1)
        self.assertEqual(self.saved(), [])

    def test_uses_the_chosen_folder_and_falls_back_to_downloads_when_it_is_missing(self):
        chosen = self.downloads.parent / "Universidad"
        chosen.mkdir()
        default = self.downloads / "Mudel AI Connect" / "Control Avanzado" / "Tema 1.pdf"
        runtime.save_download_folder(chosen)
        result = self.download()
        self.assertEqual(Path(result["path"]), chosen / "Control Avanzado" / "Tema 1.pdf")
        self.assertNotIn("Descargas", result["note"])
        runtime.save_download_folder(chosen / "USB desconectado")
        result = self.download()
        self.assertEqual(Path(result["path"]), default)
        self.assertIn("No se encontró la carpeta elegida", result["note"])
        self.assertFalse((chosen / "USB desconectado").exists())
        (runtime.data_dir() / "downloads.json").write_text("roto", encoding="utf-8")
        result = self.download()
        self.assertEqual(Path(result["path"]), default)
        self.assertIn("Vuelve a elegirla", result["note"])
        runtime.save_download_folder(None)
        self.assertEqual(Path(self.download()["path"]), default)

    @unittest.skipUnless(sys.platform == "win32", "Marca de origen de Windows")
    def test_windows_marks_the_file_as_downloaded_from_the_internet(self):
        path = self.download()["path"]
        with open(f"{path}:Zone.Identifier", encoding="utf-8") as stream:
            mark = stream.read()
        self.assertIn("ZoneId=3", mark)
        self.assertNotIn(TOKEN, mark)


class DownloadsFolderTests(unittest.TestCase):
    def test_finds_the_downloads_folder_of_the_user(self):
        folder = runtime.downloads_dir()
        self.assertTrue(folder.is_absolute())
        if sys.platform == "win32" and (Path.home() / "Downloads").is_dir() and not os.environ.get("CI"):
            self.assertTrue(folder.is_dir())


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
                    "get_upcoming_events", "read_course_file", "download_course_file"})
                for tool in tools:
                    # Ninguna cambia Moodle; la descarga escribe en el PC, sin borrar ni sobrescribir.
                    self.assertFalse(tool.annotations.destructive_hint, tool.name)
                    self.assertEqual(tool.annotations.read_only_hint, tool.name != "download_course_file", tool.name)
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
            self.assertEqual(len((await client.list_tools()).tools), 7)
            result = await client.call_tool("moodle_status")
            self.assertTrue(result.is_error)
            self.assertIn("MOODLE_TOKEN", result.content[0].text)


class CheckCommandTests(unittest.TestCase):
    def test_check_writes_utf8_whatever_the_console_encoding_is(self):
        env = {key: value for key, value in os.environ.items() if key != "PYTHONIOENCODING"}
        env.update(MOODLE_URL=BASE, MOODLE_TOKEN="", PYTHONUTF8="0")
        result = subprocess.run([sys.executable, str(Path(server.__file__).resolve()), "--check"],
                                env=env, capture_output=True, timeout=60)
        self.assertEqual(result.returncode, 1)
        self.assertIn("válido", result.stderr.decode("utf-8"))


if __name__ == "__main__":
    unittest.main()
