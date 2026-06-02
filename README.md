# restee

A cross-platform, tray-resident **break reminder for engineers**. It nudges you to
rest your eyes and step away on customizable intervals — gentle *soft* breaks, or
screen-covering *strict* breaks when you need a firmer push.

Built with **Tauri v2** (Rust core + TypeScript/HTML/CSS UI): tiny binaries, low
idle RAM (~tens of MB), no Electron.

## Features

- **Customizable rules** — any number of break rules, each with its own interval,
  duration, and enforcement (soft / strict).
- **Two enforcement tiers** — *soft* (calm, skippable full-screen overlay + chime +
  optional notification) and *strict* (opaque cover on **all monitors**, with a
  configurable escape: hold-to-skip / easy skip / no-easy-escape).
- **Safety floor** — strict breaks always auto-release at the end, and a hidden
  hold-Esc emergency exit means you can never be truly locked out.
- **Activity-aware** — auto-pauses when you're idle; optionally credits time away
  as a completed break so it doesn't nag the moment you return.
- **Tray-resident** — no main window. Start / Pause / Break now / Skip / Settings
  from the tray icon, or via global hotkeys.
- **Launch at login**, single-instance, self-healing TOML config.

> **Honest limitation:** a *truly* unescapable lockout is impossible (the OS always
> reserves Ctrl+Alt+Del, Cmd+Opt+Esc, etc.). Strict breaks are a forceful
> screen *cover*, not an OS-level lock.

## Requirements

- [Rust](https://rustup.rs/) (stable) and [Node.js](https://nodejs.org/) 18+.
- Platform webview: Windows has WebView2 preinstalled; macOS uses WKWebView; Linux
  needs `webkit2gtk` (see Tauri's [prerequisites](https://v2.tauri.app/start/prerequisites/)).

## Develop

```bash
npm install
npm run tauri dev
```

The app starts in the system tray (no window). Use **tray → Break now** to preview a
break, or **tray → Settings…** to edit rules.

Handy test hooks (env vars):
- `RESTEE_BREAK_ON_START=1` — fire a break ~2s after launch.
- `RESTEE_OPEN_SETTINGS=1` — open the settings window on launch.

## Test

```bash
cargo test -p restee-core     # pure engine + config unit/property tests
cargo clippy --workspace --all-targets
```

The timing/priority/idle logic lives in the dependency-free `restee-core` crate, so
it tests in well under a second without compiling Tauri.

## Package

```bash
npm run tauri build
```

Produces installers under `src-tauri/target/release/bundle/`:
- **Windows** — `msi/` (WiX) and `nsis/` (`.exe` setup).
- **macOS** — `dmg/` + `macos/*.app` (build on macOS).
- **Linux** — `deb/`, `rpm/`, `appimage/` (build on Linux; AppImage is the most
  portable, and most reliable for the tray).

Cross-platform installers are produced automatically in CI — see
[`.github/workflows/release.yml`](.github/workflows/release.yml).

### Run a release binary without bundling

To produce a standalone, runnable binary (no installers) — e.g. for quick local
testing:

```bash
cargo build --release --features custom-protocol   # → target/release/restee
```

> **Do not** build a runnable app with a bare `cargo build`/`cargo build --release`.
> Without the `custom-protocol` feature, Tauri compiles the app in **dev mode**, so
> every window tries to load the frontend from the Vite dev server
> (`http://localhost:1420`). With no dev server running you get a blank window /
> `ERR_CONNECTION_REFUSED`. `npm run tauri dev` and `npm run tauri build` enable the
> feature automatically; a plain `cargo build` does not.

### Signing (follow-up)

Builds are currently **unsigned**. For distribution:
- **Windows** — sign the installer with an Authenticode certificate.
- **macOS** — code-sign + notarize (required for Gatekeeper; also for any future
  input-suppression features). Windows toast notifications also render most
  reliably once the app is installed with a proper app identity.

## Configuration

Config is a self-healing TOML file in the OS config dir
(`%APPDATA%\com.restee.app\config.toml` on Windows). Editable in-app via
**Settings**, or by hand. A corrupt file is backed up and defaults are restored.

## Project layout

```
crates/restee-core/   # pure timer/state engine + config DTOs (no Tauri/OS deps)
src/                  # frontend: settings (index.html) + break overlay (overlay.html)
src-tauri/            # Tauri app: tray, idle, overlays, hotkeys, autostart, audio
```

The Rust core decides *when* to break; the Tauri layer turns those decisions into
windows, sounds, and notifications.
