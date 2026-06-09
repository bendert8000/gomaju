# restee

> 語言 / Language：**繁體中文** ｜ [English](#english)

跨平台、常駐系統匣的**工程師休息提醒工具** — 同時也是一個輕量的**時鐘鬧鐘**工具。它會依你自訂的間隔，提醒你讓眼睛休息、起身離開（溫和的*軟性*休息，或在你需要更強約束時，用覆蓋整個螢幕的*強制*休息），並能同時觸發實際時鐘的鬧鐘（每日、每週、每兩週……）。

以 **Tauri v2** 打造（Rust 核心 + TypeScript/HTML/CSS 介面）：極小的執行檔、極低的閒置記憶體用量（約數十 MB），且不使用 Electron。

## 螢幕截圖

<p align="center">
  <img src="docs/screenshots/break-rules.png" alt="休息規則儀表板" width="270">
  &nbsp;&nbsp;
  <img src="docs/screenshots/alarms.png" alt="鬧鐘視窗" width="270">
  &nbsp;&nbsp;
  <img src="docs/screenshots/settings.png" alt="設定視窗" width="270">
  <br>
  <sub><b>休息規則儀表板</b> &nbsp;·&nbsp; <b>鬧鐘</b>（含每兩週） &nbsp;·&nbsp; <b>設定</b></sub>
</p>

## 功能特色

- **休息規則** — 數量不限，每條都有各自的間隔、休息時長與強制等級（軟性／強制）。規則可以**重複**，或只觸發**一次**（之後自動停用）。每條規則可附帶一段選用的多行備註，顯示在休息畫面上。可在完整的**設定**表格中編輯，或從獨立的**休息規則儀表板**（系統匣的 *Breaks…*）快速開關 — 大型卡片附有 開／關 與 重複／單次 切換，儲存後即時重新設定執行中的計時器。
- **時鐘鬧鐘** — 名稱 + 時間 + 重複方式：**單次、每日、每週、每兩週、每月、每年**。每週與每兩週可挑選星期；每兩週從起始日期起每隔一週觸發；每月會夾到當月最後一天；每年則是月 + 日。鬧鐘會以獨特的音調與通知響起 — 即使休息計時器已暫停、或休息畫面正顯示中也一樣 — 但僅在 Restee 執行時（錯過的分鐘不會補觸發）。在專屬的**鬧鐘**視窗中管理。
- **兩種強制等級** — *軟性*（平靜、可略過的全螢幕覆蓋 + 提示音 + 選用通知）與*強制*（覆蓋**所有螢幕**的不透明遮蓋）。強制休息支援可設定的脫離方式：**長按略過**、**輕鬆**一鍵略過，或**無輕鬆脫離**。
- **自訂提示音** — 在**鈴聲**視窗（系統匣的 *Chimes…*）打造你自己的休息與鬧鐘音效：用音符**編寫旋律**（C／G／F 大調的 Do-Re-Mi，含八度與音長），或**匯入音訊檔**（wav／mp3／ogg／flac）。可邊做邊預覽（▶ 預覽 ⇄ ⏸ 暫停；每個音符加入時也會發聲），並拖曳音符重新排序。接著每條休息規則可分別挑選**開始**與**結束**鈴聲，每個鬧鐘也有自己的，音量可逐一設定 — 未設定則使用內建的預設音調。已儲存的預設集存放於各自的 `chimes.toml`。
- **預先提醒** — 休息開始前數秒、不搶焦點的選用倒數提示（可設定；`0` ＝ 關閉）。
- **感知活動狀態** — 在你閒置時自動暫停。預設的*暫停*策略只是凍結倒數；*計入*策略則會把離開的時間**計入**為一次完成的休息，讓你回來時不會立刻被打擾。
- **可選的休息顯示方式** — 大型 `MM:SS` 倒數，或逐漸減少的進度條。
- **休息語錄** — 可選擇在休息畫面顯示一句勵志語錄，每次休息隨機挑選並**在地化**（繁體中文／英文各自一份清單）。可在**設定 → 語錄**卡片中編輯及開關。
- **安全底線** — 強制休息結束時一律自動解除，並有隱藏的長按 Esc 緊急脫離，確保你永遠不會被真正鎖死。
- **常駐系統匣** — 沒有主視窗。*開始／暫停*、*重設休息計時器*、*立即休息*、*Breaks…*、*Alarms…*、*Chimes…*、*Settings…*、*語言*、*結束* — 全部從系統匣圖示操作，圖示也會顯示每條規則的即時倒數。並有選用的全域快捷鍵供 切換／立即休息／略過 使用。
- **多語介面** — 內建**繁體中文（預設）**與**英文**，可從系統匣的*語言*選單切換。
- **開機自動啟動**、單一執行個體、可自我修復的 TOML 設定檔。

> **誠實的限制：**真正無法脫離的鎖定是不可能的（作業系統永遠保留 Ctrl+Alt+Del、Cmd+Opt+Esc 等）。強制休息是強力的螢幕*遮蓋*，並非作業系統層級的鎖定。

## 系統需求

- [Rust](https://rustup.rs/)（stable）與 [Node.js](https://nodejs.org/) 18+。
- 平台 webview：Windows 已預先安裝 WebView2；macOS 使用 WKWebView；Linux 需要 `webkit2gtk`（見 Tauri 的[前置需求](https://v2.tauri.app/start/prerequisites/)）。

## 開發

```bash
npm install
npm run tauri dev
```

App 會在系統匣啟動（無視窗）。用**系統匣 → Break now** 預覽休息、**系統匣 → Breaks…** 開關規則，或**系統匣 → Settings…** 編輯所有設定。

方便的測試掛鉤（環境變數，debug build）：
- `RESTEE_BREAK_ON_START=1` — 啟動後約 2 秒觸發一次休息。
- `RESTEE_OPEN_SETTINGS=1` — 啟動時開啟設定視窗。
- `RESTEE_OPEN_ALARMS=1` — 啟動時開啟鬧鐘視窗。
- `RESTEE_NO_OPEN_RULES=1` — 抑制每次啟動原本都會開啟的休息規則視窗。

## 測試

```bash
cargo test -p restee-core     # 純引擎 + 設定 + 鬧鐘週期的單元／屬性測試
cargo clippy --workspace --all-targets
```

計時／優先序／閒置邏輯與鬧鐘週期比對器都位於不依賴外部套件的 `restee-core` crate，因此無需編譯 Tauri，遠在一秒內就能測完。

## 從原始碼建置

請在你要發佈的目標作業系統上建置 — Tauri 會為各平台打包**原生**安裝程式，因此你無法在 macOS 上交叉建置 Windows 的 `.exe`（反之亦然）。CI 會一次建置所有平台 — 見 [`.github/workflows/release.yml`](.github/workflows/release.yml)。

所有產物都會落在 `target/release/bundle/` 下（工作區根目錄的 `target/`，**不是**在 `src-tauri/` 底下）。

### 前置需求

除了上述[系統需求](#系統需求)（Rust stable + Node 18+；CI 使用 Node 20）外：

- **Windows** — [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（*Desktop development with C++* 工作負載），供 Rust 的 MSVC 工具鏈使用。Windows 10/11 已預先安裝 WebView2。
- **macOS** — Xcode Command Line Tools：`xcode-select --install`。若要跨架構或通用（universal）建置，請加上 Rust 目標：`rustup target add aarch64-apple-darwin x86_64-apple-darwin`。
- **Linux** — `webkit2gtk` 及相關套件（[`release.yml`](.github/workflows/release.yml) 中列出的 `apt` 套件）。

接著安裝一次前端相依套件：`npm install`。

### 版本管理

`package.json` 是權威的 app 版本。請用以下指令讓 Tauri 與 Cargo 的中繼資料保持一致：

```bash
npm run version:set -- 0.2.0
```

`npm run build` 在產出 `dist/` 之前，會檢查 `package.json`、`src-tauri/tauri.conf.json` 與 `src-tauri/Cargo.toml` 使用相同版本。

### 在 Windows 建置

```powershell
npm run tauri build
```

安裝程式會落在 `target\release\bundle\`：

- `msi\` — WiX 安裝程式，例如 `Restee_0.1.0_x64_en-US.msi`
- `nsis\` — NSIS 安裝 `.exe`，例如 `Restee_0.1.0_x64-setup.exe`

若想要免安裝的執行檔，見下方的「獨立執行檔（免安裝）」。

### 在 macOS 建置

```bash
npm run tauri build                                       # 主機架構
npm run tauri build -- --target universal-apple-darwin    # 通用（需兩個 Rust 目標）
# 單一架構：--target aarch64-apple-darwin | --target x86_64-apple-darwin
```

輸出：`macos/Restee.app` + `dmg/Restee_<ver>_<arch>.dmg`。

此建置**未簽署**，因此 Gatekeeper 會阻擋雙擊開啟。請對 app 按右鍵 → **打開**（確認一次），或清除隔離旗標：

```bash
xattr -dr com.apple.quarantine /path/to/Restee.app
```

### 在 Linux 建置

```bash
npm run tauri build   # 先安裝上方前置需求中的 apt 套件
```

會產生 `deb/`、`rpm/` 與 `appimage/` 套件（AppImage 最具可攜性，對系統匣也最可靠）。

### 獨立執行檔（免安裝）

若想要免安裝、可直接執行的執行檔（方便本機測試）：

```bash
cargo build --release --features custom-protocol   # → target/release/restee.exe（Windows）| restee（macOS/Linux）
```

> **請勿**用裸的 `cargo build`／`cargo build --release` 建置可執行的 app。
> 少了 `custom-protocol` feature，Tauri 會以 **dev 模式**編譯，於是每個視窗都會嘗試從 Vite 開發伺服器（`http://localhost:1420`）載入前端。沒有開發伺服器在跑時，你會看到空白視窗／`ERR_CONNECTION_REFUSED`。`npm run tauri dev` 與 `npm run tauri build` 會自動啟用此 feature；裸的 `cargo build` 不會。

這會重用既有的 `dist/`；若你改動了 `src/` 下的任何內容，請先用 `npm run build` 重新產生。在 **Windows** 上，建置前請先停止任何執行中的執行個體 — 執行中的系統匣 app 會鎖住執行檔，否則建置會以 `Access denied (os error 5)` 失敗：

```powershell
Stop-Process -Name restee -Force
```

### 簽署（後續）

目前的建置**未簽署**。若要發佈：
- **Windows** — 以 Authenticode 憑證簽署安裝程式。
- **macOS** — 程式碼簽署 + 公證（Gatekeeper 必需；任何未來的輸入抑制功能也需要）。Windows 的快顯通知，在 app 以正式身分安裝後也最能可靠呈現。

## 設定

所有狀態都存放在作業系統設定目錄中一個可自我修復的 TOML 檔：

- **Windows** — `%APPDATA%\com.restee.app\config.toml`
- **macOS** — `~/Library/Application Support/com.restee.app/config.toml`
- **Linux** — `~/.config/com.restee.app/config.toml`

它保存設定、休息**規則**與**鬧鐘**（以及所選語言）。可在 app 內透過 **Settings／Breaks…／Alarms…** 編輯，或手動編輯。損毀的檔案會被備份（`config.toml.bak`）並還原為預設值。隨附的預設值即內嵌的 [`crates/restee-core/default_config.toml`](crates/restee-core/default_config.toml)。

有兩樣東西存放在 `config.toml` **旁邊**各自的檔案中（首次執行時建立，可即時編輯）：

- **已儲存的鈴聲** — `chimes/chimes.toml`（與任何匯入的音訊檔放在一起），在 **Chimes…** 視窗編輯。規則／鬧鐘透過 `config.toml` 中的 id 參照鈴聲。
- **休息語錄** — `quotes.<locale>.txt`（`quotes.en.txt`／`quotes.zh-Hant.txt`），在 **設定 → 語錄**卡片編輯；休息畫面會從目前語言的清單中取用。

## 專案結構

```
crates/restee-core/  # 純引擎 + 設定 DTO + 鬧鐘週期（無 Tauri/OS 相依）；隨附 default_config.toml
src/                 # 前端（Vite，原生 TS）：index.html（設定）、breaks.html（休息規則儀表板）、
                     #   alarms.html（鬧鐘）、chimes.html（鈴聲編輯器）、overlay.html（休息畫面）、toast.html
                     #   （休息前提示）；共用 rule-editor.ts / notes.ts / quotes-editor.ts
src-tauri/           # Tauri 主程式：系統匣、閒置、覆蓋層、快捷鍵、開機自啟、音訊、通知、鬧鐘排程器
```

不依賴外部套件的 Rust 核心決定*何時*休息（以及鬧鐘是否到期）；Tauri 層把這些決策轉化為視窗、聲音、通知與系統匣介面。

---

<a id="english"></a>

# restee

A cross-platform, tray-resident **break reminder for engineers** — and a lightweight
**clock-alarm** tool. It nudges you to rest your eyes and step away on customizable
intervals (gentle *soft* breaks, or screen-covering *strict* breaks when you need a firmer
push), and can fire wall-clock alarms (daily, weekly, bi-weekly, …) right alongside.

Built with **Tauri v2** (Rust core + TypeScript/HTML/CSS UI): tiny binaries, low idle RAM
(~tens of MB), no Electron.

## Screenshots

<p align="center">
  <img src="docs/screenshots/break-rules.png" alt="Break-rules dashboard" width="270">
  &nbsp;&nbsp;
  <img src="docs/screenshots/alarms.png" alt="Alarms window" width="270">
  &nbsp;&nbsp;
  <img src="docs/screenshots/settings.png" alt="Settings window" width="270">
  <br>
  <sub><b>Break-rules dashboard</b> &nbsp;·&nbsp; <b>Alarms</b> (incl. bi-weekly) &nbsp;·&nbsp; <b>Settings</b></sub>
</p>

## Features

- **Break rules** — any number, each with its own interval, break duration, and enforcement
  (soft / strict). A rule can **repeat** or fire **once** (then it auto-disables). Each rule
  can carry an optional multi-line note shown on the break screen. Edit them in the full
  **Settings** grid, or flip them on/off fast from the standalone **Break-rules dashboard**
  (*Breaks…* in the tray) — big cards with On/Off and Repeat/Once toggles that save and
  reconfigure the running timer live.
- **Clock alarms** — name + time + recurrence: **Once, Daily, Weekly, Bi-weekly, Monthly,
  Yearly**. Weekly and Bi-weekly let you pick weekdays; bi-weekly fires every *other* week
  from a start date; monthly clamps to the month's last day; yearly is month + day. Alarms
  ring with a distinct tone and a notification — even while the break timer is paused or a
  break is on screen — but only while Restee is running (no catch-up for missed minutes).
  Managed in their own **Alarms** window.
- **Two enforcement tiers** — *soft* (calm, skippable full-screen overlay + chime + optional
  notification) and *strict* (opaque cover on **all monitors**). Strict breaks honor a
  configurable escape: **hold-to-skip**, **easy** one-click skip, or **no easy escape**.
- **Custom chimes** — craft your own break and alarm sounds in the **Chimes** window
  (*Chimes…* in the tray): **compose a melody** from musical notes (Do-Re-Mi in C / G / F major,
  with octave and note length) or **import an audio file** (wav / mp3 / ogg / flac). Preview as you go
  (▶ Preview ⇄ ⏸ Pause; each note also sounds as
  you add it) and drag notes to reorder. Then each break rule can pick a **start** and an **end**
  chime, and each alarm its own, with volume set per selection — leave it unset to use the built-in
  default tones. Saved presets live in their own `chimes.toml`.
- **Heads-up warning** — an optional, non-focus-stealing countdown toast a few seconds before
  a break starts (configurable; `0` = off).
- **Activity-aware** — auto-pauses while you're idle. The default *pause* policy just freezes
  the countdown; the *credit* policy instead **credits** time away as a completed break so it
  doesn't nag the moment you return.
- **Your choice of break display** — a large `MM:SS` countdown, or a draining progress bar.
- **Break quotes** — optionally show an inspirational quote on the break screen, picked at
  random each break and **localized** (separate Traditional Chinese / English lists). Edit them,
  and toggle them on/off, in the **Settings → Quotes** card.
- **Safety floor** — strict breaks always auto-release at the end, and a hidden hold-Esc
  emergency exit means you can never be truly locked out.
- **Tray-resident** — no main window. *Start / Pause*, *Reset break timer*, *Break now*,
  *Breaks…*, *Alarms…*, *Chimes…*, *Settings…*, *Language*, *Quit* — all from the tray icon, which
  also shows a live per-rule countdown. Optional global hotkeys for toggle / break-now / skip.
- **Localized** — ships **Traditional Chinese (default)** and **English**, switched from the
  tray's *Language* menu.
- **Launch at login**, single-instance, self-healing TOML config.

> **Honest limitation:** a *truly* unescapable lockout is impossible (the OS always
> reserves Ctrl+Alt+Del, Cmd+Opt+Esc, etc.). Strict breaks are a forceful screen *cover*,
> not an OS-level lock.

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
break, **tray → Breaks…** to toggle rules, or **tray → Settings…** to edit everything.

Handy test hooks (env vars, debug builds):
- `RESTEE_BREAK_ON_START=1` — fire a break ~2s after launch.
- `RESTEE_OPEN_SETTINGS=1` — open the settings window on launch.
- `RESTEE_OPEN_ALARMS=1` — open the alarms window on launch.
- `RESTEE_NO_OPEN_RULES=1` — suppress the break-rules window that otherwise opens on every launch.

## Test

```bash
cargo test -p restee-core     # pure engine + config + alarm-recurrence unit/property tests
cargo clippy --workspace --all-targets
```

The timing/priority/idle logic and the alarm-recurrence matcher live in the dependency-free
`restee-core` crate, so they test in well under a second without compiling Tauri.

## Build from source

Build on the OS you're targeting — Tauri bundles **native** installers per platform, so you
can't cross-build a Windows `.exe` on macOS (or vice-versa). CI builds every platform at once —
see [`.github/workflows/release.yml`](.github/workflows/release.yml).

All bundles land under `target/release/bundle/` (the workspace-root `target/`, **not** under
`src-tauri/`).

### Prerequisites

In addition to the [Requirements](#requirements) above (Rust stable + Node 18+; CI uses Node 20):

- **Windows** — [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  (the *Desktop development with C++* workload) for Rust's MSVC toolchain. WebView2 is preinstalled
  on Windows 10/11.
- **macOS** — Xcode Command Line Tools: `xcode-select --install`. For a cross-arch or universal
  build, add the Rust targets: `rustup target add aarch64-apple-darwin x86_64-apple-darwin`.
- **Linux** — `webkit2gtk` and friends (the `apt` packages listed in
  [`release.yml`](.github/workflows/release.yml)).

Then install the frontend dependencies once: `npm install`.

### Versioning

`package.json` is the canonical app version. Keep Tauri and Cargo metadata aligned with:

```bash
npm run version:set -- 0.2.0
```

`npm run build` checks that `package.json`, `src-tauri/tauri.conf.json`, and
`src-tauri/Cargo.toml` all use the same version before producing `dist/`.

### Build for Windows

```powershell
npm run tauri build
```

Installers land under `target\release\bundle\`:

- `msi\` — WiX installer, e.g. `Restee_0.1.0_x64_en-US.msi`
- `nsis\` — NSIS setup `.exe`, e.g. `Restee_0.1.0_x64-setup.exe`

For a no-installer binary, see [Standalone binary](#standalone-binary-no-installer) below.

### Build for macOS

```bash
npm run tauri build                                       # host architecture
npm run tauri build -- --target universal-apple-darwin    # universal (needs both Rust targets)
# single arch: --target aarch64-apple-darwin | --target x86_64-apple-darwin
```

Output: `macos/Restee.app` + `dmg/Restee_<ver>_<arch>.dmg`.

The build is **unsigned**, so Gatekeeper blocks a double-click. Right-click the app → **Open**
(confirm once), or clear the quarantine flag:

```bash
xattr -dr com.apple.quarantine /path/to/Restee.app
```

### Build for Linux

```bash
npm run tauri build   # after installing the apt packages from Prerequisites above
```

Produces `deb/`, `rpm/`, and `appimage/` bundles (AppImage is the most portable, and the most
reliable for the tray).

### Standalone binary (no installer)

For a quick runnable binary with no installer (handy for local testing):

```bash
cargo build --release --features custom-protocol   # → target/release/restee.exe (Windows) | restee (macOS/Linux)
```

> **Do not** build a runnable app with a bare `cargo build`/`cargo build --release`.
> Without the `custom-protocol` feature, Tauri compiles the app in **dev mode**, so
> every window tries to load the frontend from the Vite dev server
> (`http://localhost:1420`). With no dev server running you get a blank window /
> `ERR_CONNECTION_REFUSED`. `npm run tauri dev` and `npm run tauri build` enable the
> feature automatically; a plain `cargo build` does not.

This reuses the existing `dist/`; if you changed anything under `src/`, refresh it first with
`npm run build`. On **Windows**, stop any running instance before building — a running tray app
locks the binary, so the build otherwise fails with `Access denied (os error 5)`:

```powershell
Stop-Process -Name restee -Force
```

### Signing (follow-up)

Builds are currently **unsigned**. For distribution:
- **Windows** — sign the installer with an Authenticode certificate.
- **macOS** — code-sign + notarize (required for Gatekeeper; also for any future
  input-suppression features). Windows toast notifications also render most
  reliably once the app is installed with a proper app identity.

## Configuration

All state lives in a single, self-healing TOML file in the OS config dir:

- **Windows** — `%APPDATA%\com.restee.app\config.toml`
- **macOS** — `~/Library/Application Support/com.restee.app/config.toml`
- **Linux** — `~/.config/com.restee.app/config.toml`

It holds the settings, break **rules**, and **alarms** (plus the chosen language). Edit it
in-app via **Settings / Breaks… / Alarms…**, or by hand. A corrupt file is backed up
(`config.toml.bak`) and defaults are restored. The shipped defaults are the embedded
[`crates/restee-core/default_config.toml`](crates/restee-core/default_config.toml).

Two things live in their own files **next to** `config.toml` (seeded on first run, edited live):

- **Saved chimes** — `chimes/chimes.toml` (alongside any imported audio files), edited in the
  **Chimes…** window. A rule/alarm references a chime by id from `config.toml`.
- **Break quotes** — `quotes.<locale>.txt` (`quotes.en.txt` / `quotes.zh-Hant.txt`), edited in
  the **Settings → Quotes** card; the break screen draws from the active language's list.

## Project layout

```
crates/restee-core/  # pure engine + config DTOs + alarm recurrence (no Tauri/OS deps); ships default_config.toml
src/                 # frontend (Vite, vanilla TS): index.html (Settings), breaks.html (Break-rules dashboard),
                     #   alarms.html (Alarms), chimes.html (Chimes editor), overlay.html (break screen), toast.html
                     #   (pre-break toast); shared rule-editor.ts / notes.ts / quotes-editor.ts
src-tauri/           # Tauri host: tray, idle, overlays, hotkeys, autostart, audio, notifications, alarm scheduler
```

The dependency-free Rust core decides *when* to break (and whether an alarm is due); the
Tauri layer turns those decisions into windows, sounds, notifications, and tray UI.
