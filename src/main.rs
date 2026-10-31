mod config;
mod download;
mod install;
mod state;
mod ui;

use anyhow::{Context, Result, ensure};
use config::Settings;
use std::{ffi::OsString, process::Command};

fn main() -> Result<()> {
    let command = launch_arguments(std::env::args_os().skip(1).collect())?;
    println!("=== DCGO Launcher ===");

    let config_path = config::config_path()?;
    let Some(settings) = settings_for_launch(&config_path, ui::first_install)? else {
        println!("Installation cancelled.");
        return Ok(());
    };
    println!("Configuration: {}", config_path.display());
    update(&settings)?;
    if let Some(mut command) = game_command(&command, &settings.install_dir) {
        let status = command.status().context("Could not start the game")?;
        ensure!(status.success(), "The game exited with an error: {status}");
    }
    Ok(())
}

fn game_command(args: &[OsString], install_dir: &std::path::Path) -> Option<Command> {
    if let Some((program, args)) = args.split_first() {
        let mut command = Command::new(program);
        command.args(args);
        return Some(command);
    }
    if cfg!(target_os = "windows") {
        let mut command = Command::new(install::executable_path(install_dir));
        command.current_dir(install_dir.join("Game"));
        return Some(command);
    }
    None
}

fn launch_arguments(mut args: Vec<OsString>) -> Result<Vec<OsString>> {
    if !args.is_empty() {
        ensure!(
            args[0] == "--" && args.len() > 1,
            "Usage: dcgo-launcher [-- game-command arguments...]"
        );
        args.remove(0);
    }
    Ok(args)
}

fn update(settings: &Settings) -> Result<()> {
    let install_dir = &settings.install_dir;
    let etag_path = config::etag_path()?;
    let saved_etag = state::load_etag(&etag_path)?;
    let saved_etag = saved_etag
        .as_deref()
        .filter(|_| install::is_installed(install_dir));

    // Download and extract on the same disk so directory renames stay atomic.
    let work_dir = install::prepare_parent(install_dir)?;

    println!("Checking for updates...");
    let Some((zip_file, etag)) =
        download::download_update(settings.download_url.as_str(), saved_etag, &work_dir)?
    else {
        println!("DCGO is up to date.");
        return Ok(());
    };

    println!("Download complete.");

    println!("Extracting update...");
    install::install_update(zip_file.path(), install_dir, install::PRESERVE)?;
    state::save_etag(&etag_path, &etag)?;

    println!("DCGO updated successfully!");

    Ok(())
}

fn settings_for_launch(
    path: &std::path::Path,
    choose: impl FnOnce(Settings) -> Result<Option<Settings>>,
) -> Result<Option<Settings>> {
    let settings = if path.try_exists()? {
        Settings::load(path)?
    } else {
        Settings::default_for_user()?
    };
    // Recover the installation before deciding whether first-time setup is needed.
    install::recover_interrupted(&settings.install_dir)?;
    if install::is_installed(&settings.install_dir) {
        return Ok(Some(settings));
    }
    let Some(settings) = choose(settings)? else {
        return Ok(None);
    };
    settings.save(path)?;
    Ok(Some(settings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn launch_command_preserves_arguments_and_requires_separator() {
        assert!(launch_arguments(vec![]).unwrap().is_empty());
        let args: Vec<OsString> = ["--", "/path with spaces/proton", "run", "/games/DCGO.exe"]
            .into_iter()
            .map(OsString::from)
            .collect();
        assert_eq!(launch_arguments(args.clone()).unwrap(), args[1..]);
        assert!(launch_arguments(vec!["--".into()]).is_err());
        assert!(launch_arguments(vec!["unexpected".into()]).is_err());
    }

    #[test]
    fn game_command_uses_the_platform_default_or_the_supplied_command() {
        let dir = tempfile::tempdir().unwrap();
        let default = game_command(&[], dir.path());
        if cfg!(target_os = "windows") {
            let command = default.unwrap();
            assert_eq!(command.get_program(), install::executable_path(dir.path()));
            assert_eq!(
                command.get_current_dir(),
                Some(dir.path().join("Game").as_path())
            );
        } else {
            assert!(default.is_none());
        }
        let args = [OsString::from("proton"), OsString::from("path with spaces")];
        let command = game_command(&args, dir.path()).unwrap();
        assert_eq!(command.get_program(), "proton");
        assert_eq!(command.get_args().collect::<Vec<_>>(), [&args[1]]);
        assert!(command.get_current_dir().is_none());
    }

    #[test]
    fn existing_config_without_game_still_prompts_and_cancel_preserves_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let mut settings = Settings::default_for_user().unwrap();
        settings.install_dir = tmp.path().join("DCGO");
        settings.save(&path).unwrap();
        let original = fs::read(&path).unwrap();
        let mut prompted = false;
        assert!(
            settings_for_launch(&path, |_| {
                prompted = true;
                Ok(None)
            })
            .unwrap()
            .is_none()
        );
        assert!(prompted);
        assert_eq!(fs::read(&path).unwrap(), original);

        let selected = tmp.path().join("chosen/DCGO");
        settings_for_launch(&path, |mut settings| {
            settings.install_dir = selected.clone();
            Ok(Some(settings))
        })
        .unwrap();
        assert_eq!(Settings::load(&path).unwrap().install_dir, selected);

        fs::create_dir_all(selected.join("Game")).unwrap();
        fs::write(install::executable_path(&selected), "game").unwrap();
        assert!(
            settings_for_launch(&path, |_| panic!(
                "an installed game must not prompt for a directory"
            ))
            .unwrap()
            .is_some()
        );
    }
}
