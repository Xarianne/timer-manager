# Timer Manager

A small desktop app for creating and managing **systemd user timers** without
writing unit files by hand. It is a single Rust binary built with
[egui](https://github.com/emilk/egui).

Timers made in the app are ordinary `.service` and `.timer` files in
`~/.config/systemd/user/`, so systemd runs them and they keep working if you
uninstall the app.

## Screenshots

![The timer list](docs/timer-list.png)

![The New timer form](docs/new-timer.png)

## Features

- **Timer list** with an on/off switch, the schedule in plain English, the
  command it runs, and the next and last run time.
- **Run now** waits for the job to finish and shows whether it succeeded, with
  the output from that run.
- **New timer form** with presets, a script picker, and a live "next run"
  preview. Every option has a short plain-language description.
- **Service options:** type, working directory, environment variables, timeout,
  retry on failure, low priority.
- **Timer options:** random delay, also run after boot, catch up after being off.
- **Extra settings:** raw systemd lines for `[Unit]`, `[Service]` and `[Timer]`
  for anything the form doesn't cover. Duplicate settings are flagged.
- **Validated before saving:** `systemd-analyze verify` checks the generated
  files, so a typo can't leave a broken unit behind.
- **Safe editing:** only timers created by this app can be edited in the form.
  Other timers can be run, enabled, disabled, and opened in your text editor.
  Any timer with a unit file in `~/.config/systemd/user` can be deleted, after a
  confirmation. Timers shipped with the system can't be deleted.

## Requirements

- Linux with systemd (user instance) and `systemd-analyze`
- A Wayland or X11 session
- Rust (edition 2024) to build it

Optional, used when present:

- `kdialog` or `zenity` for the **Browse** button
- [Dank Material Shell](https://github.com/AvengeMedia/DankMaterialShell): the
  app reads its theme colors (`~/.cache/DankMaterialShell/dms-colors.json`),
  its transparency setting, and the Inter and Material Symbols fonts it ships
- Noto Sans (via fontconfig) for the bold headings

Without these, the app falls back to a built-in dark palette and egui's built-in
fonts, and icons are left out.

## Install on Fedora

Timer Manager is packaged in a [COPR](https://copr.fedorainfracloud.org/coprs/xariann/tools/)
repository for Fedora 44, 45 and Rawhide (x86_64). Enable it, then install:

```sh
sudo dnf copr enable xariann/tools
sudo dnf install timer-manager
```

Updates arrive with the rest of your system updates (`sudo dnf upgrade`). To
remove it, run `sudo dnf remove timer-manager`; timers you created stay in
`~/.config/systemd/user/`.

## Build from source

```sh
cargo build --release
install -Dm755 target/release/timer-manager ~/.local/bin/timer-manager
install -Dm644 packaging/timer-manager.desktop ~/.local/share/applications/timer-manager.desktop
```

## Usage

```sh
timer-manager          # My timers
timer-manager --all    # All user timers, including ones shipped with the system
timer-manager --new    # Open directly on the New timer form
```

The window has no title bar: use the close button at the top right.
Transparency needs a compositor that supports it. Blur depends on your
compositor settings (for example Hyprland's `decoration:blur`).

## Tests

```sh
cargo test --release
```

The backend tests create, run and delete short-lived timers named `tm-selftest*`
in your real user systemd, so they need a working `systemctl --user`. They clean
up after themselves.

## Status

Developed and used on Fedora with Hyprland. The backend is covered by tests; the
interface has been checked by hand on that setup only.

## License

GPL-3.0. See [LICENSE](LICENSE).
