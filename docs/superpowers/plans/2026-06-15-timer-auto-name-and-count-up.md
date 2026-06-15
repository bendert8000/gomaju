# Timer auto-name + count-up mode — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the editable timer name with an auto-derived locale-aware name (`"02:30 timer"` / `"02:30 計時器"`), and add a global Countdown/Count-up mode to the Timers settings card.

**Architecture:** The name is computed (never stored) from `duration_secs` + the active locale, in the backend only (the Timers window shows no name). Count-up is a display-only transform (`elapsed = duration − remaining`); the engine and fire instant are unchanged. Both features share the toast reconciler (`timer_toast::sync`) and the running-toast injection, so they are sequenced: auto-name first (T1–T4), then count-up (T5–T8), then docs/verify (T9).

**Tech Stack:** Rust (Tauri v2) — `gomaju` (src-tauri) + `gomaju-core`; TypeScript/HTML frontend; `cargo`, `npm`.

**Spec:** `docs/superpowers/specs/2026-06-15-timer-auto-name-design.md`

**Branch:** `feat/timer-auto-name` (already created; spec already committed). Each task commits its own change.

**Conventions:** `cargo build -p gomaju` / `cargo test -p gomaju` for the app crate; `cargo test -p gomaju-core` for core; `cargo clippy --workspace --all-targets`; `npm run build` for the frontend. Never *run* a plain build binary. Use the Bash (Git Bash) tool for heredoc commits; end every commit body with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

### Task 1: Core `format_clock` helper (TDD)

**Files:**
- Modify: `crates/gomaju-core/src/countdown.rs` (add `format_clock` + tests)

- [ ] **Step 1: Write the failing tests.** In `crates/gomaju-core/src/countdown.rs`, add inside the existing `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn format_clock_mm_ss_under_an_hour() {
        assert_eq!(format_clock(1), "00:01");
        assert_eq!(format_clock(90), "01:30");
        assert_eq!(format_clock(150), "02:30");
        assert_eq!(format_clock(3599), "59:59");
    }

    #[test]
    fn format_clock_h_mm_ss_at_and_past_an_hour() {
        assert_eq!(format_clock(3600), "1:00:00");
        assert_eq!(format_clock(5400), "1:30:00");
        assert_eq!(format_clock(359_999), "99:59:59");
    }
```

- [ ] **Step 2: Run to verify failure.** Run: `cargo test -p gomaju-core --lib countdown::tests::format_clock_mm_ss_under_an_hour` — Expected: FAIL (`cannot find function 'format_clock'`).

- [ ] **Step 3: Implement.** In `crates/gomaju-core/src/countdown.rs`, add (after `sanitize_countdowns`, before the `#[cfg(test)]` module):

```rust
/// Format a duration as a clock string: `mm:ss`, or `h:mm:ss` once it reaches an hour. Hours are
/// not zero-padded; minutes and seconds are (`90 -> "01:30"`, `3600 -> "1:00:00"`). Used to build a
/// timer's auto-derived display name (timers have no user-set name).
pub fn format_clock(duration_secs: u32) -> String {
    let h = duration_secs / 3600;
    let m = (duration_secs % 3600) / 60;
    let s = duration_secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}
```

- [ ] **Step 4: Run to verify pass.** Run: `cargo test -p gomaju-core --lib countdown` — Expected: the 2 new tests PASS (plus existing sanitize tests).

- [ ] **Step 5: Commit.**
```bash
git add crates/gomaju-core/src/countdown.rs
git commit -m "feat(timers): add core format_clock helper for auto-names"
```

---

### Task 2: Host `timer_display_name` + i18n word (TDD)

**Files:**
- Modify: `src-tauri/src/i18n.rs` (add `timers.timer_word`)
- Modify: `src-tauri/src/countdown.rs` (add `timer_display_name` + tests)

- [ ] **Step 1: Add the i18n word.** In `src-tauri/src/i18n.rs`, in the `tr` match, after the `"notif.timer_title" => ...` arm, add:

```rust
        "timers.timer_word" => pick(locale, "timer", "計時器"),
```

- [ ] **Step 2: Write the failing test.** In `src-tauri/src/countdown.rs`, add a `#[cfg(test)]` test (if a `mod tests` already exists there, add inside it; otherwise create one). Note this module already has a `mod tests` with run-state tests — add to it:

```rust
    #[test]
    fn timer_display_name_formats_with_locale() {
        assert_eq!(timer_display_name(150, "en"), "02:30 timer");
        assert_eq!(timer_display_name(150, "zh-Hant"), "02:30 計時器");
        assert_eq!(timer_display_name(3600, "en"), "1:00:00 timer");
    }
```

- [ ] **Step 3: Run to verify failure.** Run: `cargo test -p gomaju --lib countdown::tests::timer_display_name_formats_with_locale` — Expected: FAIL (`cannot find function 'timer_display_name'`).

- [ ] **Step 4: Implement.** In `src-tauri/src/countdown.rs`, add (near the top-level functions, e.g. after `state_str`):

```rust
/// A timer's auto-derived display name: its duration as a clock plus the localized word —
/// `"02:30 timer"` / `"02:30 計時器"`, `"1:00:00 timer"` past an hour. Timers have no user-set name;
/// this is computed wherever the name is shown (notification + toasts) so it follows the locale.
pub fn timer_display_name(duration_secs: u32, locale: &str) -> String {
    format!(
        "{} {}",
        gomaju_core::countdown::format_clock(duration_secs),
        crate::i18n::tr(locale, "timers.timer_word")
    )
}
```

- [ ] **Step 5: Run to verify pass.** Run: `cargo test -p gomaju --lib countdown` — Expected: the new test PASSES (plus existing run-state tests).

- [ ] **Step 6: Commit.**
```bash
git add src-tauri/src/i18n.rs src-tauri/src/countdown.rs
git commit -m "feat(timers): add timer_display_name + localized timer word"
```

---

### Task 3: Use the computed name in the scheduler + toast sync

**Files:**
- Modify: `src-tauri/src/countdown.rs` (`spawn_scheduler`: notification body + `to_finish`)
- Modify: `src-tauri/src/timer_toast.rs` (`sync`: running-toast name + `order` carries duration)

This removes the last reads of the stored `name` from Rust (so Task 4 can drop the field).

- [ ] **Step 1: Scheduler — compute the fired name.** In `src-tauri/src/countdown.rs::spawn_scheduler`, in step 2's `for id in due` loop, the block currently is:

```rust
                if let Some(def) = defs.get(&id) {
                    fired.push((def.name.clone(), def.chime_id.clone(), def.chime_volume_pct));
                    if !show_toasts {
                        to_finish.push((id.clone(), def.name.clone()));
                    }
                }
```

Replace both `def.name.clone()` with the computed name:

```rust
                if let Some(def) = defs.get(&id) {
                    let display = timer_display_name(def.duration_secs, &locale);
                    fired.push((display.clone(), def.chime_id.clone(), def.chime_volume_pct));
                    if !show_toasts {
                        to_finish.push((id.clone(), display));
                    }
                }
```

(`locale` is already snapshotted in step 1; `timer_display_name` is in this same module.)

- [ ] **Step 2: Toast sync — carry duration, compute the running name.** In `src-tauri/src/timer_toast.rs::sync`, change the config block so `order` carries `duration_secs` instead of `name`, and grab the locale:

```rust
    // Config first (released): stack/display order (id + duration) + the show-toasts setting.
    let (show_running, order): (bool, Vec<(String, u32)>) = {
        let cfg = st.config.lock().unwrap();
        let order = cfg
            .countdowns
            .iter()
            .map(|c| (c.id.clone(), c.duration_secs))
            .collect();
        (cfg.settings.show_timer_toasts, order)
    };
    let locale = crate::i18n::current_locale(app);
```

(`app` is the `&AppHandle` param here — pass it directly, not `&app`, matching `build_toast`'s existing `current_locale(app)` call.)

Then update the `running` builder to compute the name from duration + locale:

```rust
    // Running set (released): running timers in config order, with their computed name + remaining.
    let running: Vec<(String, String, u32)> = {
        let map = st.countdown_runtime.lock().unwrap();
        order
            .iter()
            .filter_map(|(id, dur)| match map.get(id) {
                Some(run @ CountdownRun::Running { .. }) => Some((
                    id.clone(),
                    crate::countdown::timer_display_name(*dur, &locale),
                    crate::countdown::remaining_secs(run, now),
                )),
                _ => None,
            })
            .collect()
    };
```

And update the `finished` builder so it iterates the new `order` tuple shape (id is `.0`, duration `.1` unused there — names come from `finished_toasts`):

```rust
        order
            .iter()
            .filter_map(|(id, _dur)| fin.get(id).map(|name| (id.clone(), name.clone())))
            .collect()
```

(The `valid` prune set is `order.iter().map(|(id, _)| id.as_str())` — already shape-correct. `desired_toasts`'s signature is unchanged in this task.)

- [ ] **Step 3: Build + tests.** Run: `cargo build -p gomaju` then `cargo test -p gomaju --lib timer_toast` — Expected: compiles (note: `name` field still exists, just no longer read); the 4 `desired_toasts` tests still PASS.

> Verify with `rg "\.name" src-tauri/src/countdown.rs src-tauri/src/timer_toast.rs` that no countdown `.name` read remains (alarm/rule `.name` in other files is unrelated).

- [ ] **Step 4: Commit.**
```bash
git add src-tauri/src/countdown.rs src-tauri/src/timer_toast.rs
git commit -m "feat(timers): use computed display name in notification + toasts"
```

---

### Task 4: Drop the stored `name` field (core + frontend, atomic)

**Files:**
- Modify: `crates/gomaju-core/src/countdown.rs` (remove `name` from `CountdownDto` + test helper)
- Modify: `crates/gomaju-core/default_config.toml` (remove the 7 `name = "…"` lines)
- Modify: `src/timers.ts` (remove the name input + interface/collect/add)
- Modify: `src/i18n.ts` (remove unused `timers.name_ph` / `default_name` / `new_name`)

- [ ] **Step 1: Remove the field from the DTO.** In `crates/gomaju-core/src/countdown.rs`, delete the `pub name: String,` line from `struct CountdownDto`. In the `cd()` test helper, delete the `name: "T".into(),` line.

- [ ] **Step 2: Remove names from the seed.** In `crates/gomaju-core/default_config.toml`, delete the `name = "…"` line from each of the 7 `[[countdowns]]` blocks (the `1m`/`3m`/`5m`/`10m`/`15m`/`30m`/`1h` presets), leaving their `id` / `duration_secs` / chime fields.

- [ ] **Step 3: Remove the name from the frontend.** In `src/timers.ts`:
  - In `interface CountdownDto`, delete the `name: string;` line.
  - In `timerRow`'s template literal, delete the name input line:
    `<input class="timer-name" type="text" placeholder="${t("timers.name_ph")}" />`
  - Delete the assignment `q<HTMLInputElement>(row, ".timer-name").value = v.def.name;`
  - In `collectTimers`, delete the `name: q<HTMLInputElement>(row, ".timer-name").value.trim() || t("timers.default_name"),` line.
  - In the `add-timer` click handler's `def`, delete the `name: t("timers.new_name"),` line.

- [ ] **Step 4: Remove unused i18n keys.** In `src/i18n.ts`, delete the `"timers.name_ph"`, `"timers.default_name"`, and `"timers.new_name"` entries.

- [ ] **Step 5: Verify.** Run, expecting all green:
```bash
cargo test -p gomaju-core
cargo test -p gomaju
npm run build
```
Expected: core tests pass (`default_config` still parses + is sanitize-clean; sanitize tests pass), app tests pass, `tsc`+`vite` succeed. (Old `config.toml` files with leftover `name=` load fine — serde ignores unknown fields; no `deny_unknown_fields` on `CountdownDto`.)

- [ ] **Step 6: Commit.**
```bash
git add crates/gomaju-core/src/countdown.rs crates/gomaju-core/default_config.toml src/timers.ts src/i18n.ts
git commit -m "feat(timers): drop the editable timer name (auto-derived now)"
```

---

### Task 5: `timer_count_up` setting (core config)

**Files:**
- Modify: `crates/gomaju-core/src/config.rs` (`Settings` field + `Default`)
- Modify: `crates/gomaju-core/default_config.toml` (seed)

- [ ] **Step 1: Add the field.** In `crates/gomaju-core/src/config.rs`, in `struct Settings`, after the `show_timer_toasts` field, add:

```rust
    /// Count timers up from zero to the configured duration instead of down to zero. Display-only —
    /// the fire instant is unchanged. Defaults off (countdown) so existing configs keep behavior.
    #[serde(default)]
    pub timer_count_up: bool,
```

- [ ] **Step 2: Add to the `Default` impl.** In the same file's `impl Default for Settings`, after `show_timer_toasts: true,`, add:

```rust
            timer_count_up: false,
```

- [ ] **Step 3: Seed it.** In `crates/gomaju-core/default_config.toml`, after the `show_timer_toasts = true` line, add:

```toml
timer_count_up = false
```

- [ ] **Step 4: Verify.** Run: `cargo test -p gomaju-core` — Expected: PASS (the `default_config.toml` parse + sanitize-clean tests cover the new field).

- [ ] **Step 5: Commit.**
```bash
git add crates/gomaju-core/src/config.rs crates/gomaju-core/default_config.toml
git commit -m "feat(timers): add global timer_count_up setting (default off)"
```

---

### Task 6: Count-up in the view + running toast (backend)

**Files:**
- Modify: `src-tauri/src/commands.rs` (`CountdownView.count_up` in both `cmd_get_countdowns` / `cmd_save_countdowns`)
- Modify: `src-tauri/src/timer_toast.rs` (`DesiredToast`/`ToastInfo`/`desired_toasts`/`sync`/`build_toast` + tests)

- [ ] **Step 1: Add `count_up` to `CountdownView`.** In `src-tauri/src/commands.rs`, add to `struct CountdownView`:

```rust
    /// Global setting: count up to the duration (true) vs down to zero (false). Same for every view.
    pub count_up: bool,
```

In `cmd_get_countdowns`, read it alongside `defs` and stamp each view. Replace the `defs` read + view build so it is:

```rust
    let (defs, count_up) = {
        let cfg = state.config.lock().unwrap();
        (cfg.countdowns.clone(), cfg.settings.timer_count_up)
    };
    let map = state.countdown_runtime.lock().unwrap();
    let views = defs
        .into_iter()
        .map(|def| {
            let run = map.get(&def.id);
            let remaining = match run {
                Some(r) => crate::countdown::remaining_secs(r, now),
                None => def.duration_secs, // idle -> show the full duration
            };
            CountdownView {
                state: crate::countdown::state_str(run),
                remaining_secs: remaining,
                count_up,
                def,
            }
        })
        .collect();
```

In `cmd_save_countdowns`, the written `config` is in scope; add `count_up: config.settings.timer_count_up,` to the `CountdownView { … }` it builds.

- [ ] **Step 2: Add `count_up` + `duration_secs` to the toast structs.** In `src-tauri/src/timer_toast.rs`:

`DesiredToast` becomes:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct DesiredToast {
    id: String,
    label: String,
    name: String,
    remaining_secs: u32,
    finished: bool,
    count_up: bool,
    duration_secs: u32,
}
```

`ToastInfo` becomes:

```rust
#[derive(Serialize)]
struct ToastInfo<'a> {
    id: &'a str,
    name: &'a str,
    remaining_secs: u32,
    finished: bool,
    count_up: bool,
    duration_secs: u32,
}
```

- [ ] **Step 3: Update `desired_toasts`.** Change its signature + body so running toasts carry `count_up` + `duration_secs` (finished toasts get `false` / `0`):

```rust
fn desired_toasts(
    show_running: bool,
    count_up: bool,
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
        });
    }
    out
}
```

- [ ] **Step 4: Update `sync` to supply them.** In `sync`, add `timer_count_up` to the config block and the `duration` to the running tuple:

```rust
    let (show_running, count_up, order): (bool, bool, Vec<(String, u32)>) = {
        let cfg = st.config.lock().unwrap();
        let order = cfg
            .countdowns
            .iter()
            .map(|c| (c.id.clone(), c.duration_secs))
            .collect();
        (cfg.settings.show_timer_toasts, cfg.settings.timer_count_up, order)
    };
    let locale = crate::i18n::current_locale(app);
```

Change the `running` builder to a 4-tuple `(id, name, remaining, duration)`:

```rust
    let running: Vec<(String, String, u32, u32)> = {
        let map = st.countdown_runtime.lock().unwrap();
        order
            .iter()
            .filter_map(|(id, dur)| match map.get(id) {
                Some(run @ CountdownRun::Running { .. }) => Some((
                    id.clone(),
                    crate::countdown::timer_display_name(*dur, &locale),
                    crate::countdown::remaining_secs(run, now),
                    *dur,
                )),
                _ => None,
            })
            .collect()
    };
```

And update the call: `let desired = desired_toasts(show_running, count_up, &running, &finished);`

- [ ] **Step 5: Inject into the toast.** In `build_toast`, add the two fields to the `ToastInfo` it serializes:

```rust
    let json = serde_json::to_string(&ToastInfo {
        id: &d.id,
        name: &d.name,
        remaining_secs: d.remaining_secs,
        finished: d.finished,
        count_up: d.count_up,
        duration_secs: d.duration_secs,
    })
    .unwrap_or_else(|_| "null".into());
```

- [ ] **Step 6: Update the `desired_toasts` tests.** In `timer_toast.rs`'s `mod tests`, update the helper + the 4 tests for the new signature. Replace `run`/the call sites so running tuples are 4-tuples and `desired_toasts` takes `count_up`:

```rust
    fn run(id: &str, name: &str, secs: u32, dur: u32) -> (String, String, u32, u32) {
        (id.to_string(), name.to_string(), secs, dur)
    }
    // ... fin() unchanged ...

    #[test]
    fn unchecked_mode_shows_only_finished_toasts() {
        let running = vec![run("a", "A", 30, 60)];
        let finished = vec![fin("b", "B")];
        let d = desired_toasts(false, false, &running, &finished);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].label, "timer-done-b");
        assert!(d[0].finished);
        assert_eq!(d[0].id, "b");
    }

    #[test]
    fn checked_mode_with_no_finished_shows_running_only() {
        let running = vec![run("a", "A", 30, 60), run("b", "B", 5, 5)];
        let d = desired_toasts(true, false, &running, &[]);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].label, "timer-toast-a");
        assert_eq!(d[0].remaining_secs, 30);
        assert_eq!(d[0].duration_secs, 60);
        assert!(!d[0].finished);
        assert!(!d[0].count_up);
        assert_eq!(d[1].label, "timer-toast-b");
    }

    #[test]
    fn count_up_flag_propagates_to_running_toasts() {
        let running = vec![run("a", "A", 30, 60)];
        let d = desired_toasts(true, true, &running, &[]);
        assert!(d[0].count_up);
        assert_eq!(d[0].duration_secs, 60);
    }

    #[test]
    fn running_first_then_finished_in_order() {
        let running = vec![run("a", "A", 30, 60)];
        let finished = vec![fin("b", "B"), fin("c", "C")];
        let d = desired_toasts(true, false, &running, &finished);
        let labels: Vec<&str> = d.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, ["timer-toast-a", "timer-done-b", "timer-done-c"]);
        assert_eq!((d[1].finished, d[2].finished), (true, true));
    }

    #[test]
    fn id_round_trips_through_done_label() {
        assert_eq!(id_from_done_label("timer-done-xyz"), Some("xyz"));
        assert_eq!(id_from_done_label("timer-toast-xyz"), None);
    }
```

- [ ] **Step 7: Verify.** Run: `cargo test -p gomaju --lib timer_toast` then `cargo build -p gomaju` — Expected: 5 timer_toast tests PASS; crate compiles.

- [ ] **Step 8: Commit.**
```bash
git add src-tauri/src/commands.rs src-tauri/src/timer_toast.rs
git commit -m "feat(timers): plumb count_up + duration into view and running toast"
```

---

### Task 7: Count-up display in the frontend

**Files:**
- Modify: `src/timer-toast.ts` (running branch counts up when `count_up`)
- Modify: `src/timers.ts` (`CountdownView.count_up`; readout shows elapsed)

- [ ] **Step 1: Toast — count up.** In `src/timer-toast.ts`, add the two fields to `interface ToastInfo` and the `readInjected` default:

```ts
interface ToastInfo {
  id: string;
  name: string;
  remaining_secs: number;
  finished: boolean;
  count_up: boolean;
  duration_secs: number;
}
```
```ts
const info = readInjected<ToastInfo>("__GOMAJU_TIMER_TOAST__", {
  id: "",
  name: "",
  remaining_secs: 0,
  finished: false,
  count_up: false,
  duration_secs: 0,
});
```

Replace the running-countdown block (the part after `// Running countdown toast.`, i.e. the `let remaining = …` + `setInterval`) with a count-up-aware version:

```ts
  // Running toast: count down to 0, or up to the configured duration.
  if (info.count_up) {
    let elapsed = Math.max(0, info.duration_secs - info.remaining_secs);
    time.textContent = fmt(elapsed);
    window.setInterval(() => {
      elapsed = Math.min(info.duration_secs, elapsed + 1);
      time.textContent = fmt(elapsed);
    }, 1000);
  } else {
    let remaining = info.remaining_secs;
    time.textContent = fmt(remaining);
    window.setInterval(() => {
      remaining = Math.max(0, remaining - 1);
      time.textContent = fmt(remaining);
    }, 1000);
  }
```

(Keep the `stop.title`/`addEventListener` lines for the running ✕ above this block exactly as they are.)

- [ ] **Step 2: Timers window — show elapsed in count-up.** In `src/timers.ts`:
  - Add `count_up: boolean;` to `interface CountdownView`.
  - Change `applyRunState` to take a display value rather than raw remaining. Replace its signature + remaining-readout line:

```ts
function applyRunState(row: HTMLElement, state: RunState, displaySecs: number): void {
  row.dataset.state = state;
  const toggle = q<HTMLButtonElement>(row, ".timer-toggle");
  toggle.textContent =
    state === "running"
      ? t("timers.pause")
      : state === "paused"
        ? t("timers.resume")
        : t("timers.start");
  const rem = q<HTMLElement>(row, ".timer-remaining");
  rem.textContent = state === "idle" ? "" : fmtClock(displaySecs);
}
```

  - Add a helper near `fmtClock` to pick the display value from a view:

```ts
/** The live number to show for a view: elapsed (0→duration) in count-up mode, else remaining. */
function displaySecs(v: CountdownView): number {
  return v.count_up ? Math.max(0, v.def.duration_secs - v.remaining_secs) : v.remaining_secs;
}
```

  - Update the two `applyRunState` call sites to pass `displaySecs(v)`:
    - in `timerRow`: `applyRunState(row, v.state, displaySecs(v));`
    - in `refresh`: `applyRunState(row, v.state, displaySecs(v));`

- [ ] **Step 3: Verify.** Run: `npm run build` — Expected: `tsc` + `vite` succeed.

- [ ] **Step 4: Commit.**
```bash
git add src/timer-toast.ts src/timers.ts
git commit -m "feat(timers): count up in the toast + timers-window readout"
```

---

### Task 8: Count-up control in the Timers settings card

**Files:**
- Modify: `index.html` (Timers card select)
- Modify: `src/main.ts` (`SettingsDto.timer_count_up` + load/save)
- Modify: `src/i18n.ts` (select labels)

- [ ] **Step 1: Add the select.** In `index.html`, in the **Timers** `<section class="card">`, after the `show_timer_toasts_hint` `<p class="muted">…</p>`, add:

```html
        <label class="field">
          <span data-i18n="settings.timer_mode_label">Timer direction</span>
          <select id="timer-mode">
            <option value="countdown" data-i18n="settings.timer_mode_countdown">Countdown</option>
            <option value="countup" data-i18n="settings.timer_mode_countup">Count up</option>
          </select>
        </label>
```

- [ ] **Step 2: i18n labels.** In `src/i18n.ts`, near the other `settings.*` Timers keys (`settings.timers_heading`), add:

```ts
  "settings.timer_mode_label": { en: "Timer direction", "zh-Hant": "計時方向" },
  "settings.timer_mode_countdown": { en: "Countdown", "zh-Hant": "倒數計時" },
  "settings.timer_mode_countup": { en: "Count up", "zh-Hant": "正數計時" },
```

- [ ] **Step 3: TS interface.** In `src/main.ts`, in `interface SettingsDto`, after `show_timer_toasts: boolean;`, add:

```ts
  timer_count_up: boolean;
```

- [ ] **Step 4: Load.** In `src/main.ts`'s config-apply function, after `inp("show-timer-toasts").checked = cfg.settings.show_timer_toasts;`, add:

```ts
  sel("timer-mode").value = cfg.settings.timer_count_up ? "countup" : "countdown";
```

- [ ] **Step 5: Save.** In `src/main.ts`'s `collectConfig`, in the `settings: { … }` object, after `show_timer_toasts: inp("show-timer-toasts").checked,`, add:

```ts
      timer_count_up: sel("timer-mode").value === "countup",
```

- [ ] **Step 6: Verify.** Run: `npm run build` — Expected: `tsc` + `vite` succeed. (Confirm `sel` is the existing `<select>` helper used for `app-locale`/`idle-policy`.)

- [ ] **Step 7: Commit.**
```bash
git add index.html src/main.ts src/i18n.ts
git commit -m "feat(timers): add Countdown/Count-up control to Timers settings card"
```

---

### Task 9: Docs + full verification

**Files:**
- Modify: `CLAUDE.md` (Timers section)

- [ ] **Step 1: Update CLAUDE.md.** In the "## Timers" section, update the opening definition of a countdown to reflect the auto-name + count-up. Replace the first sentence/bullet that defines a countdown so it reads (keep the surrounding run-state / scheduler / toast paragraphs intact):

```markdown
- A **countdown** is a reusable duration preset (`duration_secs` 1..=359_999 + chime); it has **no
  user-set name** — its display name is auto-derived per locale as `"{mm:ss|h:mm:ss} {timer-word}"`
  (`gomaju_core::countdown::format_clock` + `countdown::timer_display_name`, e.g. `"02:30 timer"` /
  `"02:30 計時器"`), computed wherever the name is shown (fire notification + toasts). A global
  `settings.timer_count_up` (Timers settings card; default off) switches all timers between counting
  **down** to zero and counting **up** to the configured duration — a display-only transform
  (`elapsed = duration − remaining`); the engine and fire instant are unchanged. It's one-shot
  (fires once, then idle).
```

(If the existing wording differs, fold these facts in without dropping the existing run-state/persistence notes.)

- [ ] **Step 2: Full test suite.** Run: `cargo test -p gomaju-core && cargo test -p gomaju` — Expected: all PASS.

- [ ] **Step 3: Lint.** Run: `cargo clippy --workspace --all-targets` — Expected: no new warnings.

- [ ] **Step 4: Frontend + release build.** Run: `npm run build` then `cargo build --release --features custom-protocol` — Expected: both succeed; binary at `target/release/gomaju.exe`.

- [ ] **Step 5: Manual verification (`npm run tauri dev`).**
  - Timers window has **no name field**; rows show only duration / chime / controls.
  - Toasts ON, start a 2:30 timer → running toast label reads "02:30 timer"; let it finish → notification + "Time's up!" toast read "02:30 timer". Switch app language → after reopening, a new timer's toast/notification read "02:30 計時器".
  - Timers card → set **Count up**, Save. Start a 2:30 timer → the running toast and the timers-window readout count **up** 00:00→02:30; it still fires at 2:30. Set back to **Countdown** → counts down 02:30→00:00.
  - Existing `config.toml` from before still loads (leftover `name=` ignored); the 7 default presets appear as "01:00 timer" … "1:00:00 timer".

- [ ] **Step 6: Commit.**
```bash
git add CLAUDE.md
git commit -m "docs(timers): document auto-name + count-up mode"
```

---

## Self-review notes (reconciled against the spec)

- **Spec coverage:** auto-name = T1 (format_clock) + T2 (timer_display_name + word) + T3 (consumers) + T4 (drop field + frontend + seed); count-up = T5 (setting) + T6 (view + toast backend) + T7 (frontend display) + T8 (settings UI); docs/verify = T9. Every spec section maps to a task.
- **Sequencing:** T3 removes all Rust reads of `.name` *before* T4 drops the field (so the breaking change lands cleanly). T4 changes core DTO + frontend payload together (so `cmd_save_countdowns` deser stays consistent). Count-up (T5–T8) builds on the auto-name'd `sync` (which already carries `duration_secs`).
- **Type consistency:** `format_clock(u32)->String`; `timer_display_name(u32,&str)->String`; `Settings.timer_count_up: bool`; `CountdownView.count_up: bool`; `DesiredToast`/`ToastInfo` add `count_up: bool` + `duration_secs: u32` (alongside the prior `finished`); `desired_toasts(show_running, count_up, running:&[(id,name,remaining,duration)], finished:&[(id,name)])`; TS `count_up`/`duration_secs` mirror the injected snake_case. The `timer-mode` `<select>` maps `"countup"`↔`true`.
- **No `deny_unknown_fields`** on `CountdownDto`/`Settings`/`ConfigFile` (verified — only `quotes.rs`/`progress.rs`), so dropping `name` and adding `timer_count_up` are backward-compatible with no `CONFIG_VERSION` bump.
