# DCGO Launcher

A Rust installer and updater for DCGO on Linux and Windows, with a Ratatui
terminal interface. No Python scripts are required.

## First run

Keep the launcher outside the game installation directory. Run it in a terminal:

- Linux: `./dcgo-launcher-linux-x86_64`
- Windows: `.\dcgo-launcher.exe`

If the game is missing, choose an absolute path to an empty or new directory.
Press Enter to install, Esc to cancel, or Ctrl+U to clear the path. The launcher
saves the location, downloads the game, and validates the ZIP before installing.

On later runs it checks for updates, downloads only changed versions, and
replaces the installation. An interrupted directory swap is recovered on startup.
If installation or update fails, the launcher stops before starting the game.

- **Windows:** the launcher starts `Game/DCGO.exe` after a successful check.
- **Linux:** the launcher only installs or updates unless given a launch command.
  The Windows game requires Proton; see the Steam setup below.

Updates replace the entire game directory. Extra files inside it are removed
unless listed in `PRESERVE` in `src/install.rs` (currently empty). Keep personal
files outside the installation directory.

## Steam on Linux

Install the game in a terminal first. Add `Game/DCGO.exe` as a non-Steam game,
then select Proton in its compatibility settings. Set its launch options to:

```text
"/absolute/path/to/dcgo-launcher-linux-x86_64" -- %command%
```

Each click on Play runs the updater before the Steam/Proton command. Arguments
after `--` are passed directly to the process without shell parsing. Keep the
same game path in Steam and the launcher configuration. If the installation
is missing, run the launcher in a terminal again to use the setup screen.

## Configuration

The launcher creates `appsettings.json` automatically:

- Linux: `~/.config/dcgo-launcher/appsettings.json`
- Windows: `%APPDATA%\dcgo-launcher\config\appsettings.json`

It contains `download_url` and the absolute `install_dir`. Set `DCGO_CONFIG` to
use another configuration file. Linux follows XDG directory settings. The
installed ETag is stored separately in the application's local data directory.

## Build and test

Install a Rust toolchain supporting edition 2024, then run:

```sh
cargo test --locked
cargo build --release --locked
```

Native build output: `target/release/dcgo-launcher` on Linux or
`target/release/dcgo-launcher.exe` on Windows. A Windows build also needs the
platform's C build tools because some dependencies compile native code.

To cross-compile Windows x86_64 from Linux, install MinGW-w64 (including the
`x86_64-w64-mingw32-gcc` compiler), then run:

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --locked --target x86_64-pc-windows-gnu
```

Windows cross-build output:
`target/x86_64-pc-windows-gnu/release/dcgo-launcher.exe`.
Test that executable on Windows before distributing it. Linux binaries should
also be built on a distribution compatible with the intended users' glibc.
