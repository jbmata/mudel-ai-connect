mod moodle;
mod server;

use rmcp::{ServiceExt, transport::stdio};
use server::MoodleServer;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let server = MoodleServer::from_env();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--check"] {
        return match server.status().await {
            Ok(value) => {
                println!("{value}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    if !args.is_empty() {
        eprintln!("Uso: mudel-ai-connect-server [--check]");
        return std::process::ExitCode::FAILURE;
    }
    // stdout pertenece exclusivamente a MCP; nunca escribir diagnósticos ni claves aquí.
    match server.serve(stdio()).await {
        Ok(service) => match service.waiting().await {
            Ok(_) => std::process::ExitCode::SUCCESS,
            Err(_) => {
                eprintln!("Se cerró la conexión MCP con un error.");
                std::process::ExitCode::FAILURE
            }
        },
        Err(_) => {
            eprintln!("No se pudo iniciar la conexión MCP.");
            std::process::ExitCode::FAILURE
        }
    }
}
