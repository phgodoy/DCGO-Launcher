use anyhow::{Context, Result};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

/// Root directory in the official ZIP archive.
const ZIP_ROOT: &str = "DCGO_Standalone";

/// Relative paths to keep if the game stores user data in its installation.
/// Updates replace the directory; unlisted files missing from the ZIP are removed.
/// Example: &["Game/Saves", "Game/config.ini"].
pub const PRESERVE: &[&str] = &[];

pub fn executable_path(install_dir: &Path) -> PathBuf {
    install_dir.join("Game").join("DCGO.exe")
}

pub fn is_installed(install_dir: &Path) -> bool {
    executable_path(install_dir).is_file()
}

/// Create and return the parent directory used for downloads and staging.
pub fn prepare_parent(install_dir: &Path) -> Result<PathBuf> {
    let parent = match install_dir.parent() {
        Some(p) if p.as_os_str().is_empty() => PathBuf::from("."),
        Some(p) => p.to_path_buf(),
        None => anyhow::bail!("install_dir must not be the filesystem root"),
    };
    fs::create_dir_all(&parent)
        .with_context(|| format!("Could not create directory {}", parent.display()))?;
    Ok(parent)
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Restore the .old directory if an update stopped before replacing it.
pub fn recover_interrupted(install_dir: &Path) -> Result<()> {
    let backup = sibling_with_suffix(install_dir, ".old");
    if !install_dir.exists() && backup.exists() {
        println!("Recovering the previous installation after an interrupted update...");
        fs::rename(&backup, install_dir)
            .with_context(|| format!("Could not restore {}", backup.display()))?;
    }
    Ok(())
}

/// Extract and validate beside the installation, then swap with rollback on failure.
pub fn install_update(zip_path: &Path, install_dir: &Path, preserve: &[&str]) -> Result<()> {
    let parent = prepare_parent(install_dir)?;

    // Stage on the same filesystem to allow atomic directory renames.
    let staging = tempfile::Builder::new()
        .prefix(".dcgo-staging-")
        .tempdir_in(&parent)
        .context("Could not create temporary directory")?;

    let file =
        File::open(zip_path).with_context(|| format!("Could not open {}", zip_path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).context("The downloaded file is not a valid ZIP archive")?;

    archive
        .extract(staging.path())
        .context("Could not extract update")?;

    let extracted_dir = staging.path().join(ZIP_ROOT);

    if !executable_path(&extracted_dir).is_file() {
        anyhow::bail!("Invalid update: Game/DCGO.exe was not found in the ZIP archive");
    }

    println!("Update validated");

    // Copy user data first so a copy failure leaves the installation untouched.
    carry_over(install_dir, &extracted_dir, preserve)?;

    println!("Installing to: {}", install_dir.display());
    swap_directories(&extracted_dir, install_dir)?;

    println!("Files updated");

    Ok(())
}

fn carry_over(old: &Path, new: &Path, preserve: &[&str]) -> Result<()> {
    for relative in preserve {
        let source = old.join(relative);
        if !source.exists() {
            continue;
        }
        let destination = new.join(relative);
        if source.is_dir() {
            copy_directory(&source, &destination)?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source, &destination)
                .with_context(|| format!("Could not preserve {}", source.display()))?;
        }
    }
    Ok(())
}

fn swap_directories(new: &Path, target: &Path) -> Result<()> {
    let backup = sibling_with_suffix(target, ".old");
    if backup.exists() {
        fs::remove_dir_all(&backup)
            .with_context(|| format!("Could not remove {}", backup.display()))?;
    }

    let had_old = target.exists();
    if had_old {
        fs::rename(target, &backup).with_context(|| {
            format!(
                "Could not move the previous installation: {}",
                target.display()
            )
        })?;
    }

    if let Err(error) = fs::rename(new, target) {
        if had_old {
            // Roll back to the previous installation.
            let _ = fs::rename(&backup, target);
        }
        return Err(error).with_context(|| format!("Could not install to {}", target.display()));
    }

    if had_old {
        let _ = fs::remove_dir_all(&backup);
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;

        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if source_path.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)
                .with_context(|| format!("Could not copy {}", source_path.display()))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, files: &[(&str, &str)]) {
        let _ = fs::remove_file(path);
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, content) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    fn exe(dir: &Path) -> String {
        fs::read_to_string(executable_path(dir)).unwrap()
    }

    #[test]
    fn install_then_update_replaces_and_removes_stale_files() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("games/DCGO");
        let zip_path = tmp.path().join("pkg.zip");

        make_zip(
            &zip_path,
            &[
                ("DCGO_Standalone/Game/DCGO.exe", "v1"),
                ("DCGO_Standalone/old.txt", "stale"),
            ],
        );
        install_update(&zip_path, &install_dir, &[]).unwrap();
        assert_eq!(exe(&install_dir), "v1");
        assert!(install_dir.join("old.txt").exists());

        make_zip(&zip_path, &[("DCGO_Standalone/Game/DCGO.exe", "v2")]);
        install_update(&zip_path, &install_dir, &[]).unwrap();

        assert_eq!(exe(&install_dir), "v2");
        assert!(!install_dir.join("old.txt").exists());
        assert!(!sibling_with_suffix(&install_dir, ".old").exists());
    }

    #[test]
    fn invalid_package_leaves_existing_install_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("DCGO");
        let zip_path = tmp.path().join("pkg.zip");

        make_zip(&zip_path, &[("DCGO_Standalone/Game/DCGO.exe", "v1")]);
        install_update(&zip_path, &install_dir, &[]).unwrap();

        make_zip(&zip_path, &[("DCGO_Standalone/readme.txt", "x")]);
        let error = install_update(&zip_path, &install_dir, &[]).unwrap_err();
        assert!(error.to_string().contains("DCGO.exe"));
        assert_eq!(exe(&install_dir), "v1");
    }

    #[test]
    fn preserved_paths_survive_updates() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("DCGO");
        let zip_path = tmp.path().join("pkg.zip");

        make_zip(&zip_path, &[("DCGO_Standalone/Game/DCGO.exe", "v1")]);
        install_update(&zip_path, &install_dir, &[]).unwrap();

        fs::create_dir_all(install_dir.join("Game/Saves")).unwrap();
        fs::write(install_dir.join("Game/Saves/deck.txt"), "my deck").unwrap();
        fs::write(install_dir.join("stale.txt"), "x").unwrap();

        make_zip(&zip_path, &[("DCGO_Standalone/Game/DCGO.exe", "v2")]);
        install_update(&zip_path, &install_dir, &["Game/Saves"]).unwrap();

        assert_eq!(exe(&install_dir), "v2");
        assert_eq!(
            fs::read_to_string(install_dir.join("Game/Saves/deck.txt")).unwrap(),
            "my deck"
        );
        assert!(!install_dir.join("stale.txt").exists());
    }

    #[test]
    fn interrupted_swap_is_recovered() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("DCGO");
        let backup = sibling_with_suffix(&install_dir, ".old");
        fs::create_dir_all(backup.join("Game")).unwrap();
        fs::write(backup.join("Game/DCGO.exe"), "v1").unwrap();

        recover_interrupted(&install_dir).unwrap();

        assert!(is_installed(&install_dir));
        assert!(!backup.exists());
    }
}
