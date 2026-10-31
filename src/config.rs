use anyhow::{Context, Result};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Application name used for user configuration and state directories.
pub const APP_NAME: &str = "dcgo-launcher";

pub const DEFAULT_DOWNLOAD_URL: &str =
    "https://www.dcgo.online/Builds/Standalone/DCGO_Standalone.zip";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub download_url: String,
    pub install_dir: PathBuf,
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path)
            .with_context(|| format!("Could not read configuration: {}", path.display()))?;
        let settings: Self = serde_json::from_str(&json)
            .with_context(|| format!("Invalid configuration: {}", path.display()))?;
        settings
            .validate()
            .with_context(|| format!("Invalid configuration: {}", path.display()))?;
        Ok(settings)
    }

    fn validate(&self) -> Result<()> {
        let url =
            reqwest::Url::parse(&self.download_url).context("download_url must be a valid URL")?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https"),
            "download_url must use HTTP or HTTPS"
        );
        anyhow::ensure!(
            !self.install_dir.as_os_str().is_empty(),
            "install_dir must not be empty"
        );
        // Relative paths would depend on the directory used to launch the program.
        anyhow::ensure!(
            self.install_dir.is_absolute(),
            "install_dir must be an absolute path"
        );
        Ok(())
    }

    /// Default installation location: the user's local data directory.
    /// Linux: ~/.local/share/DCGO | Windows: %LOCALAPPDATA%\DCGO
    pub fn default_for_user() -> Result<Self> {
        let base = BaseDirs::new().context("Could not locate the user directory")?;
        Ok(Self {
            download_url: DEFAULT_DOWNLOAD_URL.to_owned(),
            install_dir: base.data_local_dir().join("DCGO"),
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
            .with_context(|| format!("Could not save configuration: {}", path.display()))
    }
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", APP_NAME)
        .context("Could not locate the user configuration directories")
}

/// Configuration file location; DCGO_CONFIG overrides it for custom profiles.
/// Linux: ~/.config/dcgo-launcher/appsettings.json
/// Windows: %APPDATA%\dcgo-launcher\config\appsettings.json
pub fn config_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DCGO_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    Ok(project_dirs()?.config_dir().join("appsettings.json"))
}

/// Keep the installed ETag outside the game directory, which updates replace.
pub fn etag_path() -> Result<PathBuf> {
    Ok(project_dirs()?.data_local_dir().join("installed.etag"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_settings_and_rejects_invalid_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("appsettings.json");
        Settings::default_for_user().unwrap().save(&path).unwrap();
        let settings = Settings::load(&path).unwrap();
        assert!(settings.download_url.starts_with("https://"));
        assert!(!settings.install_dir.as_os_str().is_empty());
        assert!(settings.install_dir.is_absolute());
        for json in [
            r#"{"download_url":"https://example.com/game.zip"}"#,
            r#"{"download_url":"invalid","install_dir":"game"}"#,
            r#"{"download_url":"file:///game.zip","install_dir":"game"}"#,
            r#"{"download_url":"https://example.com/game.zip","install_dir":""}"#,
            r#"{"download_url":"https://example.com/game.zip","install_dir":"game"}"#,
            r#"{"download_url":"https://example.com/game.zip","install_dir":"/x","extra":1}"#,
        ] {
            fs::write(&path, json).unwrap();
            assert!(Settings::load(&path).is_err(), "{json}");
        }
    }

    #[test]
    fn existing_invalid_config_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("appsettings.json");
        fs::write(&path, "{ broken").unwrap();

        assert!(Settings::load(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
    }
}
