# CLAUDE.md

Guidance for AI agents (and humans) working in this repo. Keep it short and current.

restee is a cross-platform, tray-resident break reminder built with **Tauri v2**
(Rust core + TypeScript/HTML/CSS UI). The dependency-free `restee-core` crate decides
*when* to break; the `src-tauri` layer turns those decisions into windows, sounds,
tray UI, and notifications.

## Build & run

| Goal | Command |
| --- | --- |
| Dev (hot reload, runs Vite dev server) | `npm run tauri dev` |
| Full release + installers | `npm run tauri build` |
| Quick runnable release binary (no installers) | `cargo build --release --features custom-protocol` |
| Core engine tests (fast, no Tauri) | `cargo test -p restee-core` |
| Lint | `cargo clippy --workspace --all-targets` |

### ⚠️ Never build a runnable app with plain `cargo build [--release]`

The frontend is loaded via `WebviewUrl::App(...)`, which resolves to either the dev
server (`devUrl` = `http://localhost:1420`) or the **embedded** assets. Tauri's build
script decides this:

```rust
let dev = !custom_protocol;   // dev mode UNLESS the `custom-protocol` feature is on
```

- `npm run tauri dev` / `npm run tauri build` toggle `custom-protocol` automatically.
- A bare `cargo build`/`cargo build --release` does **not** → the binary comes out in
  **dev mode** → every window (settings, overlays) tries to load from the Vite dev
  server. With no server running you get **`ERR_CONNECTION_REFUSED`** / a blank window.

So for a standalone binary, always pass `--features custom-protocol` (declared in
`src-tauri/Cargo.toml`). The release binary lands at `target/release/restee.exe`
(workspace target dir at the repo root, not under `src-tauri/`).

`npm run tauri build` also runs `npm run build` (`tsc && vite build`) to refresh
`dist/`. If you only `cargo build --features custom-protocol`, you reuse the existing
`dist/` — rebuild the frontend separately (`npm run build`) if `src/` changed.

## Notifications (platform notes)

- Break/soft notifications use `tauri-plugin-notification` (`runtime::show_notification`).
- The **startup** "Restee is running now" toast is special: `runtime::show_startup_notification`
  auto-dismisses after ~2s. The plugin exposes no control over toast lifetime, and a
  native Windows banner can't be shown for less than the OS minimum (~5s). So on
  Windows we drive the WinRT toast directly (`windows` crate) and call
  `ToastNotifier::Hide` after 2s, which clears both the banner and the Action Center
  entry. Other platforms (and any WinRT failure) fall back to the plugin.

## Layout

```
crates/restee-core/   # pure timer/state engine + config DTOs (no Tauri/OS deps)
src/                  # frontend: settings (index.html), overlay.html, toast.html
src-tauri/            # Tauri app: tray, idle, overlays, hotkeys, autostart, audio, notifications
```

## Dev/test hooks (debug builds only)

- `RESTEE_BREAK_ON_START=1` — fire a break ~2s after launch.
- `RESTEE_OPEN_SETTINGS=1` — open the settings window on launch.
- Frontends log `restee: window content loaded: <label>` once their page renders —
  a useful signal that embedded assets actually loaded (it never fires in a broken
  dev-mode binary).
