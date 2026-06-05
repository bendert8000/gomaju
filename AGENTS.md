# Repository Guidelines

## Project Structure & Module Organization

Restee is a Tauri v2 tray app: TypeScript/HTML/CSS UI plus Rust runtime and core logic. Frontend entrypoints live in `src/` and pair with `index.html`, `breaks.html`, `alarms.html`, `chimes.html`, `overlay.html`, and `toast.html`.

`src-tauri/src/` owns windows, tray behavior, audio, commands, notifications, hotkeys, idle detection, and platform integration. `crates/restee-core/src/` owns pure timer, config, alarm, quote, and chime logic. Defaults are `crates/restee-core/default_*.toml`; docs are in `docs/`; outputs are `dist/` and root `target/`.

## Build, Test, and Development Commands

- `npm install` - install dependencies.
- `npm run tauri dev` - run the tray app with Vite hot reload.
- `npm run build` - run `tsc` and build frontend assets into `dist/`.
- `npm run tauri build` - build release bundles.
- `cargo build --release --features custom-protocol` - build a standalone binary.
- `cargo test -p restee-core` - run core tests.
- `cargo clippy --workspace --all-targets` - lint Rust.

Do not use plain `cargo build` for a standalone app; without `custom-protocol`, windows load from Vite. If `src/` changed before a direct Cargo release build, run `npm run build` first.

Debug hooks: `RESTEE_BREAK_ON_START=1`, `RESTEE_OPEN_SETTINGS=1`, `RESTEE_OPEN_ALARMS=1`, `RESTEE_NO_OPEN_RULES=1`.

## Coding Style & Naming Conventions

TypeScript is strict ES2020 with `noUnusedLocals` and `noUnusedParameters`. Use DTO types mirroring Rust structs and kebab-case filenames such as `chime-preview.ts`. Rust follows `rustfmt`, snake_case functions/modules, PascalCase types, and focused modules. Comment only non-obvious behavior.

## Testing Guidelines

Prefer unit tests beside the code using `#[cfg(test)] mod tests`. Put timing, recurrence, sanitization, and config behavior in `restee-core` where tests stay fast. For UI or Tauri command changes, add Rust tests when logic is separable and manually verify with `npm run tauri dev`.

## Commit & Pull Request Guidelines

Recent history uses Conventional Commits, for example `feat: chime preview buttons...`, `fix: cmd_save_quotes...`, and `docs: sync spec/plan...`. Use a short type prefix and specific summary.

Pull requests should describe behavior changed, list commands run, include screenshots for UI changes, and link issues or specs under `docs/superpowers/`.

## Architecture & Agent Notes

Preserve storage ownership: `config.toml` holds settings, rules, and alarms; `chimes.toml` holds saved chimes and imported audio metadata; `quotes.toml` holds per-locale quote lists. Save paths generally sanitize, write, then swap cache.

Rules are edited in both Settings and the Breaks dashboard, so avoid stale saves. Alarms are wall-clock, minute-edge triggered, and do not catch up missed fires. See `CLAUDE.md` before changing scheduler, persistence, notification, audio preview, or multi-window behavior.
