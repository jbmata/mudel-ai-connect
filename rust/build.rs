use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=MUDEL_SERVER_BINARY");
    if env::var_os("CARGO_FEATURE_DESKTOP").is_none() {
        return;
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("server.bin");
    if let Some(source) = env::var_os("MUDEL_SERVER_BINARY") {
        let source = fs::canonicalize(source)
            .expect("MUDEL_SERVER_BINARY debe apuntar al servidor compilado");
        println!("cargo:rerun-if-changed={}", source.display());
        fs::copy(source, output).expect("No se pudo incluir el servidor en el instalador");
    } else {
        // Las comprobaciones de código y de la ventana no necesitan un servidor empaquetado.
        fs::write(output, []).unwrap();
    }
}
