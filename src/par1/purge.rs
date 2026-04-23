use std::path::{Path, PathBuf};

pub(crate) fn purge_par1_files(par1_files: &[PathBuf]) -> std::io::Result<()> {
    for path in par1_files {
        remove_if_exists(path)?;
    }
    Ok(())
}

pub(crate) fn purge_par1_backups(backups: &[PathBuf]) -> std::io::Result<()> {
    for path in backups {
        remove_if_exists(path)?;
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
