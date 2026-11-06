use mudel_ai_connect_server::{credentials, server::MoodleServer};
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let kind = if let Some(index) = args.iter().position(|arg| arg == "--plugin") {
        args.remove(index);
        credentials::Kind::Plugin
    } else {
        credentials::Kind::Installed
    };
    if args == ["--setup"] {
        return match credentials::setup(kind) {
            Ok(()) => {
                println!("Cuenta guardada en el almacén seguro del sistema.");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    if !args.is_empty() && args != ["--check"] {
        eprintln!("Uso: mudel-ai-connect-server [--check | --setup] [--plugin]");
        return std::process::ExitCode::FAILURE;
    }
    let server = MoodleServer::from_account(kind);
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
