# Timer-toast progress bar — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a thin progress bar (fills with elapsed/duration) to each running timer toast, gated by a new global Timers setting (default on).

**Architecture:** A `timer_toast_progress` bool in core `Settings` flows through `timer_toast::sync` → injected into the running toast's `ToastInfo.progress`; the toast JS updates a 4px fill bar each second from `elapsed/duration` (it already computes elapsed for count-up/countdown). The "Time's up!" toast and the Timers window get no bar. Composes with the existing auto-name + count-up features.

**Tech Stack:** Rust (Tauri v2) — `gomaju` (src-tauri) + `gomaju-core`; TypeScript/HTML/CSS; `cargo`, `npm`.

**Spec:** `docs/superpowers/specs/2026-06-15-timer-toast-progress-design.md`

**Branch:** `feat/timer-toast-progress` (created; spec committed). Each task commits its own change. Use the Bash (Git Bash) tool for heredoc commits; end every commit body with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. IDE/rust-analyzer diagnostics may lag mid-edit — trust the actual `cargo`/`npm` output.

---

### Task 1: `timer_toast_progress` setting (core)

**Files:**
- Modify: `crates/gomaju-core/src/config.rs` (`Settings` field + `Default`)
- Modify: `crates/gomaju-core/default_config.toml` (seed)

- [ ] **Step 1: Add the field.** In `crates/gomaju-core/src/config.rs`, in `struct SettingsDto` (the one with `show_timer_toasts` and `timer_count_up`), immediately AFTER the `timer_count_up` field, add:

```rust
    /// Show a progress bar on each running timer toast (fills with elapsed/duration). UI/host-only.
    /// Defaults on.
    #[serde(default = "default_true")]
    pub timer_toast_progress: bool,
```

- [ ] **Step 2: Add to the `Default` impl.** In the same file's `impl Default for SettingsDto` (the block with `timer_count_up: false,`), immediately AFTER `timer_count_up: false,`, add:

```rust
            timer_toast_progress: true,
```

- [ ] **Step 3: Seed it.** In `crates/gomaju-core/default_config.toml`, immediately AFTER the `timer_count_up = false` line, add:

```toml
timer_toast_progress = true
```

- [ ] **Step 4: Verify.** Run: `cargo test -p gomaju-core` — Expected: PASS (the `default_config.toml`-parses-and-is-clean test and the `default_settings_match_settingsdto_default` test stay green). Also `cargo build -p gomaju`.

- [ ] **Step 5: Commit.**
```bash
git add crates/gomaju-core/src/config.rs crates/gomaju-core/default_config.toml
git commit -m "feat(timers): add timer_toast_progress setting (default on)"
```

---

### Task 2: Inject `progress` into the running toast (backend)

**Files:**
- Modify: `src-tauri/src/timer_toast.rs` (`DesiredToast`, `ToastInfo`, `desired_toasts`, `sync`, `build_toast`, tests)

- [ ] **Step 1: Add `progress` to the two structs.** In `src-tauri/src/timer_toast.rs`:

`DesiredToast` (add after `duration_secs`):
```rust
    duration_secs: u32,
    progress: bool,
```

`ToastInfo` (add after `duration_secs`):
```rust
    duration_secs: u32,
    progress: bool,
```

- [ ] **Step 2: Add the `progress` param to `desired_toasts`.** Change its signature and the two `DesiredToast` constructions. The new signature + body:

```rust
fn desired_toasts(
    show_running: bool,
    count_up: bool,
    progress: bool,
    running: &[(String, String, u32, u32)], // (id, name, remaining_secs, duration_secs)
    finished: &[(String, String)],          // (id, name)
) -> Vec<DesiredToast> {
    let mut out = Vec::new();
    if show_running {
        for (id, name, remaining, duration) in running {
            out.push(DesiredToast {
                id: id.clone(),
                label: label_for(id),
                name: name.clone(),
                remaining_secs: *remaining,
                finished: false,
                count_up,
                duration_secs: *duration,
                progress,
            });
        }
    }
    for (id, name) in finished {
        out.push(DesiredToast {
            id: id.clone(),
            label: done_label_for(id),
            name: name.clone(),
            remaining_secs: 0,
            finished: true,
            count_up: false,
            duration_secs: 0,
            progress: false,
        });
    }
    out
}
```

Also update the doc comment above it to mention `progress` (one line, e.g. after the `count_up` bullet): `/// - `progress`: the global `timer_toast_progress` setting; propagated to running toasts only.`

- [ ] **Step 3: `sync` reads the setting + passes it.** In `sync`, change the config-block tuple to also read `timer_toast_progress`:

```rust
    let (show_running, count_up, progress, order): (bool, bool, bool, Vec<(String, u32)>) = {
        let cfg = st.config.lock().unwrap();
        let order = cfg
            .countdowns
            .iter()
            .map(|c| (c.id.clone(), c.duration_secs))
            .collect();
        (
            cfg.settings.show_timer_toasts,
            cfg.settings.timer_count_up,
            cfg.settings.timer_toast_progress,
            order,
        )
    };
```

And update the `desired_toasts` call site to pass `progress`:
```rust
    let desired = desired_toasts(show_running, count_up, progress, &running, &finished);
```

(Leave the `running`/`finished` builders, early-out, close/create loops, and `relayout` unchanged.)

- [ ] **Step 4: `build_toast` injects it.** In `build_toast`, add to the `ToastInfo { ... }` (after `duration_secs: d.duration_secs,`):

```rust
        progress: d.progress,
```

- [ ] **Step 5: Update the tests.** In `timer_toast.rs`'s `mod tests`, the 5 existing `desired_toasts(...)` calls now need the extra `progress` arg. Update each call to insert `progress` as the 3rd arg, and add a propagation test. Specifically:
  - `unchecked_mode_shows_only_finished_toasts`: `desired_toasts(false, false, true, &running, &finished)` — then also assert the finished toast has `progress` false: add `assert!(!d[0].progress);`
  - `checked_mode_with_no_finished_shows_running_only`: `desired_toasts(true, false, true, &running, &[])` — add `assert!(d[0].progress);`
  - `count_up_flag_propagates_to_running_toasts`: `desired_toasts(true, true, true, &running, &[])` (unchanged assertions)
  - `running_first_then_finished_in_order`: `desired_toasts(true, false, true, &running, &finished)`
  - (the `id_round_trips_through_done_label` test has no `desired_toasts` call — leave it)
  - Add a new test:
```rust
    #[test]
    fn progress_flag_propagates_to_running_only() {
        let running = vec![run("a", "A", 30, 60)];
        let finished = vec![fin("b", "B")];
        let d = desired_toasts(true, false, true, &running, &finished);
        assert!(d[0].progress, "running toast carries progress");
        assert!(!d[1].progress, "finished toast never shows a bar");
        let off = desired_toasts(true, false, false, &running, &[]);
        assert!(!off[0].progress, "progress off -> running toast has no bar");
    }
```

- [ ] **Step 6: Verify.** Run: `cargo test -p gomaju --lib timer_toast` then `cargo build -p gomaju` — Expected: 6 timer_toast tests PASS; crate compiles.

- [ ] **Step 7: Commit.**
```bash
git add src-tauri/src/timer_toast.rs
git commit -m "feat(timers): inject progress flag into the running toast"
```

---

### Task 3: Render the progress bar (frontend)

**Files:**
- Modify: `timer-toast.html` (bar markup)
- Modify: `src/styles.css` (bar styles)
- Modify: `src/timer-toast.ts` (interface + show/update/hide the bar)

- [ ] **Step 1: Add the bar markup.** In `timer-toast.html`, immediately AFTER the `<div class="timer-toast__row"> … </div>` block (and before `<script …>`), add:

```html
    <div class="timer-toast__bar" id="bar-track"><div class="timer-toast__bar-fill" id="bar"></div></div>
```

- [ ] **Step 2: Add the styles.** In `src/styles.css`, in the timer-toast section (after the `.timer-toast__stop:hover` rule, before the next `/* --- … --- */` comment), add:

```css
.timer-toast__bar {
  height: 4px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.1);
  overflow: hidden;
}
.timer-toast__bar[hidden] {
  display: none;
}
.timer-toast__bar-fill {
  height: 100%;
  width: 0;
  background: #6aa6ff;
  transition: width 1s linear;
}
```

- [ ] **Step 3: Interface + default.** In `src/timer-toast.ts`, add `progress: boolean;` to `interface ToastInfo` (after `duration_secs: number;`) and `progress: false,` to the `readInjected` default object (after `duration_secs: 0,`).

- [ ] **Step 4: Hide the bar on the finished toast.** In the `if (info.finished) { … }` block, BEFORE its `return;`, add:

```ts
    $("bar-track").hidden = true; // terminal toast: no progress bar
```

- [ ] **Step 5: Drive the bar in the running branch.** Replace the running-toast block (everything from the `// Running toast: count down to 0, or up to the configured duration.` comment through its `if (info.count_up) { … } else { … }`) with this version that also updates the bar:

```ts
  // Progress bar (fills with elapsed/duration, both modes). Hidden when the setting is off.
  const barTrack = $("bar-track");
  barTrack.hidden = !info.progress;
  const bar = $("bar");
  const setBar = (elapsed: number): void => {
    if (info.progress && info.duration_secs > 0) {
      bar.style.width = `${Math.min(100, (elapsed / info.duration_secs) * 100)}%`;
    }
  };

  // Running toast: count down to 0, or up to the configured duration.
  if (info.count_up) {
    let elapsed = Math.max(0, info.duration_secs - info.remaining_secs);
    time.textContent = fmt(elapsed);
    setBar(elapsed);
    window.setInterval(() => {
      elapsed = Math.min(info.duration_secs, elapsed + 1);
      time.textContent = fmt(elapsed);
      setBar(elapsed);
    }, 1000);
  } else {
    let remaining = info.remaining_secs;
    time.textContent = fmt(remaining);
    setBar(info.duration_secs - remaining);
    window.setInterval(() => {
      remaining = Math.max(0, remaining - 1);
      time.textContent = fmt(remaining);
      setBar(info.duration_secs - remaining);
    }, 1000);
  }
```

- [ ] **Step 6: Verify.** Run: `npm run build` — Expected: `tsc` + `vite build` succeed.

- [ ] **Step 7: Commit.**
```bash
git add timer-toast.html src/styles.css src/timer-toast.ts
git commit -m "feat(timers): render the progress bar on running timer toasts"
```

---

### Task 4: Progress toggle in the Timers settings card

**Files:**
- Modify: `index.html` (Timers card checkbox)
- Modify: `src/main.ts` (`SettingsDto` + load/save)
- Modify: `src/i18n.ts` (label)

- [ ] **Step 1: Add the checkbox.** In `index.html`, in the **Timers** `<section class="card">`, AFTER the `timer-mode` `<label class="field"> … </label>` block, add:

```html
        <label class="field field--checkbox">
          <input id="timer-toast-progress" type="checkbox" />
          <span data-i18n="settings.timer_toast_progress_label">Show a progress bar on timer toasts</span>
        </label>
```

- [ ] **Step 2: i18n label.** In `src/i18n.ts`, near the other Timers settings keys (e.g. after `settings.timer_mode_countup`), add:

```ts
  "settings.timer_toast_progress_label": {
    en: "Show a progress bar on timer toasts",
    "zh-Hant": "在計時器提示窗顯示進度條",
  },
```

- [ ] **Step 3: TS interface.** In `src/main.ts`, in `interface SettingsDto`, AFTER `timer_count_up: boolean;`, add:

```ts
  timer_toast_progress: boolean;
```

- [ ] **Step 4: Load.** In `src/main.ts`'s config-apply function, AFTER the line `sel("timer-mode").value = cfg.settings.timer_count_up ? "countup" : "countdown";`, add:

```ts
  inp("timer-toast-progress").checked = cfg.settings.timer_toast_progress;
```

- [ ] **Step 5: Save.** In `src/main.ts`'s `collectConfig()`, in the `settings: { … }` object, AFTER `timer_count_up: sel("timer-mode").value === "countup",`, add:

```ts
      timer_toast_progress: inp("timer-toast-progress").checked,
```

- [ ] **Step 6: Verify.** Run: `npm run build` — Expected: `tsc` + `vite build` succeed.

- [ ] **Step 7: Commit.**
```bash
git add index.html src/main.ts src/i18n.ts
git commit -m "feat(timers): add progress-bar toggle to the Timers settings card"
```

---

### Task 5: Docs + full verification

**Files:**
- Modify: `CLAUDE.md` (Timers-toast bullet)

- [ ] **Step 1: Update CLAUDE.md.** In the "## Timers" section's timer-toast bullet, fold in the progress bar. Find the sentence listing the injected fields (it currently reads `injects `{id,name,remaining_secs,finished,count_up,duration_secs}``) and update it to:

```markdown
  `{id,name,remaining_secs,finished,count_up,duration_secs,progress}` (name = the auto-derived display
  name); a **running** toast counts locally — down to 0, or up to `duration_secs` when `count_up` — and,
  when `progress` (the `settings.timer_toast_progress` setting, default on) is set, fills a 4px bar with
  `elapsed/duration`; the host closes it on finish/stop (no event push, empty capability). The toggles
  (show-toasts + Countdown/Count-up direction + progress bar) live in the **Timers** card in Settings
  (`index.html`).
```

(Match the existing surrounding wording; the key additions are the `progress` field, the bar, and the setting.)

- [ ] **Step 2: Full test suite.** Run: `cargo test -p gomaju-core && cargo test -p gomaju` — Expected: all PASS.

- [ ] **Step 3: Lint.** Run: `cargo clippy --workspace --all-targets` — Expected: no new warnings.

- [ ] **Step 4: Frontend + release build.** Run: `npm run build` then `cargo build --release --features custom-protocol` — Expected: both succeed; binary at `target/release/gomaju.exe`.

- [ ] **Step 5: Manual verification (`npm run tauri dev`).**
  - Timers settings card shows the "Show a progress bar on timer toasts" checkbox (checked by default).
  - Start a short timer with toasts on → a 4px bar at the toast's bottom fills smoothly toward full as it counts; at full the timer fires (countdown toast closes; "Time's up!" toast shows, with NO bar).
  - Works the same in Count-up mode (bar still fills toward completion).
  - Uncheck the setting + Save → new running toasts show no bar.

- [ ] **Step 6: Commit.**
```bash
git add CLAUDE.md
git commit -m "docs(timers): document the timer-toast progress bar"
```

---

## Self-review notes (reconciled against the spec)

- **Spec coverage:** setting = T1; backend inject (`ToastInfo`/`DesiredToast`/`desired_toasts`/`sync`/`build_toast`) = T2; frontend bar (html/css/ts, both modes, finished-hidden, off-hidden) = T3; settings UI (checkbox + main.ts + i18n) = T4; docs + verify = T5. Every spec section maps to a task.
- **Type consistency:** `Settings.timer_toast_progress: bool` (T1) ↔ `SettingsDto.timer_toast_progress: boolean` (T4); `DesiredToast.progress` + `ToastInfo.progress` (T2) ↔ TS `ToastInfo.progress` (T3); `desired_toasts(show_running, count_up, progress, running, finished)` — the `progress` param is inserted as the 3rd arg and ALL 5 existing test call sites + the `sync` call site are updated (T2/T3 steps). HTML ids `bar-track` / `bar` match the `$("bar-track")`/`$("bar")` lookups.
- **No `CONFIG_VERSION` bump** (additive bool with `default_true`); old configs default the bar on.
- **Scope:** running toast only; no `CountdownView`/Timers-window change; composes with auto-name + count-up (the bar uses the already-injected `duration_secs`/`remaining_secs`/`count_up`).
