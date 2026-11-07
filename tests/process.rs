//! Pruebas del binario real, sin credenciales ni red: modo `--check`, cuenta guardada
//! y conexión MCP por stdio.

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Output, Stdio},
};

const BINARY: &str = env!("CARGO_BIN_EXE_mudel-ai-connect-server");

fn base_command(home: &Path) -> Command {
    let mut command = Command::new(BINARY);
    command
        .env_remove("MOODLE_URL")
        .env_remove("MOODLE_TOKEN")
        .env("LOCALAPPDATA", home)
        .env("XDG_DATA_HOME", home)
        .env("HOME", home);
    command
}

fn run(command: &mut Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("la salida debe ser UTF-8")
}

#[test]
fn check_errors_are_utf8_and_do_not_echo_the_token() {
    let home = tempfile::tempdir().unwrap();
    let output = run(
        base_command(home.path())
            .arg("--check")
            .env("MOODLE_URL", "http://invalid.example")
            .env("MOODLE_TOKEN", "SecretNeverEcho987"),
        b"",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("HTTPS"), "{stderr}");
    assert!(!stderr.contains("SecretNeverEcho987"));
}

#[test]
fn unknown_arguments_print_usage_and_fail() {
    let home = tempfile::tempdir().unwrap();
    let output = run(base_command(home.path()).arg("--nope"), b"");
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).contains("Uso:"));
}

#[cfg(windows)]
#[test]
fn saved_accounts_stay_separate_and_never_expose_the_token() {
    let home = tempfile::tempdir().unwrap();
    let directory = home.path().join("mudel-ai-connect");
    std::fs::create_dir(&directory).unwrap();
    let plugin = directory.join("credentials.json");
    let saved = directory.join("credentials.dpapi.json");
    std::fs::write(&plugin, b"plugin unchanged").unwrap();
    let url = "https://127.0.0.1:1";
    let token = "FakeSecretOnlyForThisTest987";

    for value in [token.to_string(), format!("{token}Replacement")] {
        let output = run(
            base_command(home.path()).arg("--setup"),
            format!("{url}\n{value}\n").as_bytes(),
        );
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
        let shown = [output.stdout.clone(), output.stderr.clone()].concat();
        assert!(!text(&shown).contains(&value));
        assert!(
            !text(&std::fs::read(&saved).unwrap()).contains(&value),
            "el token debe guardarse cifrado"
        );
        assert_eq!(std::fs::read(&plugin).unwrap(), b"plugin unchanged");
    }

    let output = run(base_command(home.path()).arg("--check"), b"");
    assert_eq!(output.status.code(), Some(1));
    let shown = text(&[output.stdout.clone(), output.stderr.clone()].concat());
    assert!(shown.contains("No se pudo consultar Moodle"), "{shown}");
    assert!(!shown.contains(token));

    let original = std::fs::read(&saved).unwrap();
    for input in [
        &b"http://invalid.example\nNeverSaveThis123\n"[..],
        &b"https://campus.example\n"[..],
    ] {
        let output = run(base_command(home.path()).arg("--setup"), input);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(std::fs::read(&saved).unwrap(), original);
        assert!(!text(&[output.stdout, output.stderr].concat()).contains("NeverSaveThis123"));
    }

    // Una cuenta del instalador dañada no afecta a la del plugin.
    std::fs::write(&saved, "corrupt").unwrap();
    let output = run(
        base_command(home.path()).args(["--setup", "--plugin"]),
        format!("{url}\nPluginSecret123\n").as_bytes(),
    );
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(std::fs::read(&saved).unwrap(), b"corrupt");
    let stored = std::fs::read(&plugin).unwrap();
    assert_ne!(stored, b"plugin unchanged");
    assert!(!text(&stored).contains("PluginSecret123"));

    let output = run(base_command(home.path()).args(["--check", "--plugin"]), b"");
    let shown = text(&[output.stdout.clone(), output.stderr.clone()].concat());
    assert!(shown.contains("No se pudo consultar Moodle"), "{shown}");
    assert!(!shown.contains("PluginSecret123"));

    let output = run(base_command(home.path()).arg("--check"), b"");
    assert!(text(&output.stderr).contains("cuenta guardada"));

    let output = run(
        base_command(home.path())
            .arg("--check")
            .env("MOODLE_URL", "https://campus.example")
            .env("MOODLE_TOKEN", ""),
        b"",
    );
    assert!(text(&output.stderr).contains("MOODLE_TOKEN"));
    let output = run(
        base_command(home.path())
            .arg("--check")
            .env("MOODLE_URL", "https://other.example"),
        b"",
    );
    assert!(text(&output.stderr).contains("juntas"));

    std::fs::remove_file(&saved).unwrap();
    let output = run(base_command(home.path()).arg("--check"), b"");
    let stderr = text(&output.stderr);
    assert!(stderr.contains("Falta configurar"), "{stderr}");
    assert!(!stderr.contains("No se pudo consultar Moodle"));
    assert!(!stderr.contains("PluginSecret123"));
}

/// Cliente MCP mínimo: JSON por líneas sobre el stdio del proceso.
struct Session {
    child: std::process::Child,
    reader: BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Session {
    fn start(home: &Path) -> Self {
        let mut child = base_command(home)
            .env("MOODLE_URL", "https://campus.example")
            .env("MOODLE_TOKEN", "")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let reader = BufReader::new(child.stdout.take().unwrap());
        let mut session = Session {
            child,
            reader,
            next: 0,
        };
        let init = session.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "process-test", "version": "0"}}),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "mudel-ai-connect");
        session.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        session
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(
                self.reader.read_line(&mut line).unwrap() > 0,
                "el servidor cerró stdout"
            );
            let value: Value = serde_json::from_str(&line).expect("stdout solo debe tener MCP");
            if value["id"] == id {
                return value;
            }
        }
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn failed(response: &Value) -> bool {
    response.get("error").is_some() || response["result"]["isError"] == true
}

#[test]
fn real_process_discovers_tools_and_rejects_invalid_calls() {
    let home = tempfile::tempdir().unwrap();
    let mut session = Session::start(home.path());

    let tools = session.request("tools/list", json!({}));
    let tools = tools["result"]["tools"].as_array().unwrap();
    let mut names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "download_course_file",
            "get_course_context",
            "get_upcoming_events",
            "list_assignments",
            "list_courses",
            "moodle_status",
            "read_course_file",
        ]
    );
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert_eq!(
            tool["annotations"]["readOnlyHint"],
            name != "download_course_file",
            "{name}"
        );
        assert_eq!(tool["annotations"]["destructiveHint"], false, "{name}");
    }

    let missing = session.call("moodle_status", json!({}));
    assert!(failed(&missing));
    assert!(missing.to_string().contains("MOODLE_TOKEN"), "{missing}");

    for (name, arguments) in [
        ("get_course_context", json!({"course_id": -1})),
        (
            "get_course_context",
            json!({"course_id": 7, "section_start": -1}),
        ),
        (
            "get_course_context",
            json!({"course_id": 7, "section_limit": 11}),
        ),
        ("list_assignments", json!({"course_id": 0})),
        ("read_course_file", json!({"course_id": 7, "module_id": -1})),
        (
            "download_course_file",
            json!({"course_id": 7, "module_id": 90, "path": "/tmp/arbitrary"}),
        ),
    ] {
        let response = session.call(name, arguments.clone());
        assert!(failed(&response), "{name} {arguments}: {response}");
    }

    let prompts = session.request("prompts/list", json!({}));
    assert!(prompts.to_string().contains("estudiar_curso"));
    let prompt = session.request(
        "prompts/get",
        json!({"name": "estudiar_curso",
               "arguments": {"course_id": "7", "pregunta": "Explica el tema 1"}}),
    );
    let body = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(body.contains("Curso solicitado: 7."));
    assert!(body.contains("Petición del usuario: Explica el tema 1"));
}
