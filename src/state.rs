use anyhow::{Context, Result};
use std::{fs, io::ErrorKind, path::Path};

/// Installed version ETag, or None if no ETag has been saved.
pub fn load_etag(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(etag) if etag.is_empty() => Ok(None),
        Ok(etag) => Ok(Some(etag)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("Could not read saved ETag"),
    }
}

pub fn save_etag(path: &Path, etag: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Could not create {}", parent.display()))?;
    }
    fs::write(path, etag).context("Could not save installed update ETag")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_roundtrip_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/installed.etag");

        assert_eq!(load_etag(&path).unwrap(), None);
        save_etag(&path, "\"v1\"").unwrap();
        assert_eq!(load_etag(&path).unwrap().as_deref(), Some("\"v1\""));
    }
}
