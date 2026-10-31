use anyhow::{Context, Result, ensure};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Paragraph, Wrap},
};
use std::{fs, io::IsTerminal, path::PathBuf};

use crate::config::Settings;

pub fn first_install(mut settings: Settings) -> Result<Option<Settings>> {
    ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "Open the launcher in a terminal to choose the installation directory"
    );
    let mut terminal = ratatui::init();
    let result = (|| {
        let mut input = settings.install_dir.to_string_lossy().into_owned();
        let mut error = String::new();
        loop {
            terminal.draw(|frame| {
                let areas = Layout::vertical([
                    Constraint::Length(4),
                    Constraint::Length(3),
                    Constraint::Min(1),
                ])
                .margin(1)
                .split(frame.area());
                frame.render_widget(
                    Paragraph::new("Welcome! Choose where to install DCGO.\nUse a dedicated game directory that is empty or does not exist yet.")
                        .block(Block::bordered().title(" DCGO · First-time setup "))
                        .wrap(Wrap { trim: false }),
                    areas[0],
                );
                let field = Paragraph::new(input.as_str())
                    .block(Block::bordered().title(" Full path · type to edit "));
                let width = areas[1].width.saturating_sub(2).max(1);
                let offset = ratatui::text::Line::from(input.as_str())
                    .width()
                    .saturating_sub(usize::from(width) - 1);
                frame.render_widget(field.scroll((0, offset.min(u16::MAX as usize) as u16)), areas[1]);
                frame.render_widget(
                    Paragraph::new(format!("Enter: install   Esc: cancel   Ctrl+U: clear\n\n{error}"))
                        .style(Style::default().fg(Color::Yellow))
                        .wrap(Wrap { trim: false }),
                    areas[2],
                );
            })?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.clear()
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    input.push(c)
                }
                KeyCode::Enter => match validate_destination(&input) {
                    Ok(path) => {
                        settings.install_dir = path;
                        return Ok(Some(settings));
                    }
                    Err(err) => error = format!("{err:#}"),
                },
                _ => {}
            }
        }
    })();
    ratatui::restore();
    result
}

fn validate_destination(input: &str) -> Result<PathBuf> {
    let path = PathBuf::from(input);
    ensure!(
        path.is_absolute(),
        "Enter an absolute path for the game directory"
    );
    ensure!(
        path.file_name().is_some(),
        "Choose a directory, not the disk root"
    );
    // Prevent directory replacement from deleting unrelated files.
    if path.try_exists()? {
        ensure!(path.is_dir(), "The selected path is not a directory");
        ensure!(
            fs::read_dir(&path)?.next().transpose()?.is_none(),
            "The directory contains files; choose an empty directory"
        );
    }
    ensure!(
        !input
            .split(['/', '\\'])
            .any(|part| matches!(part, "." | "..")),
        "Enter the full path without . or .. components"
    );
    let parent = crate::install::prepare_parent(&path)?;
    tempfile::NamedTempFile::new_in(&parent).context("Cannot write to the selected location")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_requires_an_absolute_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(validate_destination("").is_err());
        assert!(validate_destination("DCGO").is_err());
        assert!(
            validate_destination(tmp.path().ancestors().last().unwrap().to_str().unwrap()).is_err()
        );
        let path = tmp.path().join("DCGO");
        assert_eq!(validate_destination(path.to_str().unwrap()).unwrap(), path);
        fs::create_dir(&path).unwrap();
        assert!(validate_destination(path.to_str().unwrap()).is_ok());
        fs::write(path.join("save.txt"), "save").unwrap();
        assert!(validate_destination(path.to_str().unwrap()).is_err());
        assert_eq!(fs::read_to_string(path.join("save.txt")).unwrap(), "save");
        assert!(validate_destination(path.join("save.txt").to_str().unwrap()).is_err());
    }
}
