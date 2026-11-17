//! Sustitución transaccional del servidor, compartida por instalador y actualizador.

use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub const EXE: &str = if cfg!(windows) {
    "mudel-ai-connect-server.exe"
} else {
    "mudel-ai-connect-server"
};
pub(crate) const IO_ERROR: &str =
    "No se pudieron guardar los cambios. Comprueba los permisos y cierra las aplicaciones de IA.";

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("La configuración es un enlace simbólico. No se ha sustituido; configúrala manualmente.".into());
    }
    let parent = path.parent().ok_or(IO_ERROR)?;
    fs::create_dir_all(parent).map_err(|_| IO_ERROR)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| IO_ERROR)?;
    temporary.write_all(bytes).map_err(|_| IO_ERROR)?;
    temporary.as_file().sync_all().map_err(|_| IO_ERROR)?;
    temporary.persist(path).map_err(|_| IO_ERROR)?;
    Ok(())
}

pub(crate) fn lock(directory: &Path) -> Result<File, String> {
    fs::create_dir_all(directory).map_err(|_| IO_ERROR)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("install.lock"))
        .map_err(|_| IO_ERROR)?;
    file.try_lock().map_err(
        |_| "Hay otra instalación o cambio de permisos en marcha. Espera a que termine.",
    )?;
    Ok(file)
}

pub(crate) fn server_matches(directory: &Path, payload: &[u8]) -> bool {
    !payload.is_empty()
        && fs::read(directory.join(EXE))
            .is_ok_and(|bytes| Sha256::digest(bytes) == Sha256::digest(payload))
}

pub(crate) struct ServerChange {
    target: PathBuf,
    pub(crate) backup: PathBuf,
    had_old: bool,
    published: bool,
    /// El servidor anterior estaba en uso: solo se apartó su ejecutable y se conserva la carpeta.
    in_place: bool,
}

impl ServerChange {
    pub(crate) fn apply(target: &Path, payload: Option<&[u8]>) -> Result<Option<Self>, String> {
        if payload.is_some_and(|bytes| server_matches(target, bytes))
            || (payload.is_none() && !target.exists())
        {
            return Ok(None);
        }
        if fs::symlink_metadata(target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(IO_ERROR.into());
        }
        let parent = target.parent().ok_or(IO_ERROR)?;
        fs::create_dir_all(parent).map_err(|_| IO_ERROR)?;
        let staging = tempfile::Builder::new()
            .prefix("mudel-new-")
            .tempdir_in(parent)
            .map_err(|_| IO_ERROR)?;
        if let Some(payload) = payload {
            atomic_write(&staging.path().join(EXE), payload)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(staging.path().join(EXE), fs::Permissions::from_mode(0o700))
                    .map_err(|_| IO_ERROR)?;
            }
            atomic_write(
                &staging.path().join("LICENSE"),
                include_bytes!("../LICENSE"),
            )?;
        }
        let backup = tempfile::Builder::new()
            .prefix("mudel-previous-")
            .tempdir_in(parent)
            .map_err(|_| IO_ERROR)?
            .keep();
        let mut change = Self {
            target: target.into(),
            backup,
            had_old: target.exists(),
            published: false,
            in_place: false,
        };
        if change.had_old && fs::rename(target, change.backup.join("server")).is_err() {
            // Windows no deja renombrar una carpeta con un programa en marcha, pero sí su
            // ejecutable: se aparta solo ese archivo y el programa sigue con su copia.
            if !target.join(EXE).is_file()
                || fs::rename(target.join(EXE), change.backup.join("aside")).is_err()
            {
                let _ = fs::remove_dir(&change.backup);
                return Err(IO_ERROR.into());
            }
            change.in_place = true;
        }
        if change.in_place {
            if payload.is_some() {
                change.published = true;
                let placed = fs::rename(staging.path().join(EXE), target.join(EXE)).is_ok()
                    && atomic_write(&target.join("LICENSE"), include_bytes!("../LICENSE")).is_ok();
                if !placed {
                    change.rollback()?;
                    return Err(IO_ERROR.into());
                }
            } else {
                // Desinstalar: se borra lo que no esté en uso.
                let _ = remove_tree(target);
            }
        } else if payload.is_some() {
            if fs::rename(staging.path(), target).is_err() {
                change.rollback()?;
                return Err(IO_ERROR.into());
            }
            change.published = true;
        }
        Ok(Some(change))
    }

    pub(crate) fn rollback(&self) -> Result<(), String> {
        if self.in_place {
            if self.published {
                let _ = fs::remove_file(self.target.join(EXE));
            }
            fs::create_dir_all(&self.target).map_err(|_| IO_ERROR)?;
            fs::rename(self.backup.join("aside"), self.target.join(EXE)).map_err(|_| IO_ERROR)?;
            return fs::remove_dir(&self.backup).map_err(|_| IO_ERROR.into());
        }
        if self.published {
            remove_tree(&self.target)?;
        }
        if self.had_old {
            fs::rename(self.backup.join("server"), &self.target).map_err(|_| IO_ERROR)?;
        }
        fs::remove_dir(&self.backup).map_err(|_| IO_ERROR)?;
        Ok(())
    }

    pub(crate) fn finish(&self) -> Result<(), String> {
        // Si la copia anterior sigue en uso (una app con el servidor abierto), no es un fallo:
        // `sweep` la borrará en la siguiente ejecución.
        let _ = remove_tree(&self.backup);
        Ok(())
    }
}

pub(crate) fn remove_tree(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    #[allow(clippy::permissions_set_readonly_false)] // En Windows quita el atributo; no cambia las ACL.
    fn writable(path: &Path) -> std::io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Ok(());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                writable(&entry?.path())?;
            }
        }
        if metadata.permissions().readonly() {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)?;
        }
        Ok(())
    }
    #[cfg(windows)]
    writable(path).map_err(|_| IO_ERROR)?;
    fs::remove_dir_all(path).map_err(|_| IO_ERROR.into())
}

pub(crate) fn sweep_previous(bin: &Path) {
    for entry in fs::read_dir(bin).into_iter().flatten().flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("mudel-previous-")
        {
            let _ = remove_tree(&entry.path());
        }
    }
}
