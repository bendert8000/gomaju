# Unsaved-changes guard for Settings & Alarms windows

- **Date:** 2026-06-03
- **Status:** Approved (design)
- **Scope:** `src/main.ts` (Settings), `src/alarms.ts` (Alarms), shared frontend modules, `i18n.ts`, `styles.css`, `capabilities/alarms.json`

## Problem

The Settings and Alarms windows hold editable form state behind an explicit **Save**
button. Closing either window — via the in-app **Close** button or the OS title-bar **X** —
discards in-progress edits silently. Users can lose work without warning.

The **Today's breaks** window auto-saves on every toggle, so it has no unsaved state and is
out of scope.

## Decisions (from brainstorming)

| Question | Decision |
| --- | --- |
| Which windows | **Settings + Alarms** (both have a Save button) |
| Confirmation dialog | **Save / Don't Save / Cancel** (3 buttons) |
| Close triggers guarded | **In-app Close button + OS title-bar X / Alt+F4** |

A 3-button dialog rules out `tauri-plugin-dialog` (native dialogs support at most two
buttons), so the confirmation is a **custom in-window modal**.

## Goals

- No silent data loss: closing a dirty Settings/Alarms window prompts to Save / Don't Save / Cancel.
- A clean (non-dirty) window closes immediately, with no prompt.
- Both close triggers (Close button and window X / Alt+F4) are guarded identically.
- Logic is shared between the two windows, not duplicated.

## Non-goals

- Guarding the Today's breaks window (auto-saves; nothing to lose).
- Autosave or draft persistence.
- Undo/redo.

## Architecture

Frontend-owned guard. The dirty state lives in the form, so the guard logic lives in the
frontend; Rust changes are limited to one capability addition. (Rejected alternative:
backend `on_window_event` coordination — splits logic across Rust/JS for no benefit.)

### 1. Shared confirm modal — `src/confirm-save.ts`

```ts
export type CloseChoice = "save" | "dont_save" | "cancel";
export function confirmUnsaved(): Promise<CloseChoice>;
```

- Builds its own overlay + dialog DOM on first call (no edits to `index.html` / `alarms.html`).
- Buttons: **Save** (primary, default — Enter), **Don't Save**, **Cancel** (Esc, overlay click).
- Title + body + button labels from i18n.
- Focus the Save button on open; restore focus / remove listeners on close.
- Returns a promise resolving to the chosen `CloseChoice`.
- Styled in `styles.css` to match the app (reuses existing button classes
  `btn-primary` / `btn-ghost`).

### 2. Dirty tracking (per window)

- After the initial `render()` and after **every successful save**, capture
  `baseline = JSON.stringify(collect())`.
- `isDirty() === JSON.stringify(collect()) !== baseline`.
- Both sides run through the same `collect()`, so input-normalization round-trips
  (`blankToNull`, `Number(...) || fallback`, `clampInt`, rule defaults) cannot produce
  false positives.

**Settings focus-refresh edge case (also fixes an existing data-loss bug — Codex P1).**
`main.ts` re-pulls the rules grid from disk on every window focus (`refreshRulesFromDisk`) so
edits made in the Break-rules window show up — but today it does so **unconditionally**,
silently discarding any unsaved rule edits in Settings. The focus handler becomes **async**
and **skips the refresh whenever the form is dirty**, then re-baselines only after a clean
refresh has completed (awaited):

```
on focus (async):
  if (isDirty()) return;                  // preserve unsaved edits; don't clobber the grid
  await refreshRulesFromDisk();           // updates `current`, re-renders rules grid
  baseline = JSON.stringify(collect());   // stay "clean" w.r.t. the new disk state
```

This guarantees focus never discards unsaved edits, and a rule toggled in the other window
(while Settings is clean) is still picked up without masquerading as an unsaved edit. Skipping
the refresh while dirty can leave an other-window rule toggle unreflected until the next save —
the same "concurrent multi-window edit" caveat already documented in CLAUDE.md, and acceptable.

### 3. Shared guard installer — `src/unsaved-guard.ts`

```ts
interface GuardHooks {
  collect: () => unknown;        // current form -> serializable snapshot
  save: () => Promise<boolean>;  // true on success (persisted)
  close: () => void;             // perform the actual window close
}
export function installUnsavedGuard(hooks: GuardHooks): {
  isDirty: () => boolean;   // used by the Settings focus handler (skip refresh while dirty)
  markSaved: () => void;    // re-baseline after a save, or after a clean focus-refresh
};
```

`requestClose()` (internal):

```
if (!isDirty()) { close(); return; }
switch (await confirmUnsaved()) {
  case "cancel":    return;                     // stay open
  case "save":      if (await save()) close();  // close only if persisted
                    return;
  case "dont_save": close(); return;            // discard + close
}
```

Wiring — a single **in-flight guard** so rapid X/Close can't stack modals or double-save
(Codex P2):

```
let inFlight = false;
async function guardedClose() {
  if (inFlight) return;                  // coalesce duplicate close requests
  inFlight = true;
  try { await requestClose(); } finally { inFlight = false; }
}
// In-app Close button:
closeBtn.addEventListener("click", guardedClose);
// OS X / Alt+F4: ALWAYS preventDefault, then drive closing ourselves:
getCurrentWindow().onCloseRequested((e) => { e.preventDefault(); void guardedClose(); });
```

`close()` invokes the existing `cmd_close_settings` / `cmd_close_alarms`. **Those commands are
changed to call Rust `window.destroy()` instead of `window.close()`** (see §4), so the
guard-approved programmatic close does NOT re-emit `close-requested` — eliminating re-entrancy
(no `closing` flag) and needing no JS window-destroy permission, since `destroy()` runs in
Rust, which is not capability-gated. (Codex P1: the original `window.close()` + flag
pass-through is fragile and would have needed `core:window:allow-destroy`.)

### 4. Small refactors

`save()` in `main.ts` and `alarms.ts` returns `Promise<boolean>`:
- `true` when `cmd_save_*` resolves (config/alarms persisted).
- `false` when it throws (error message already shown).
- **Partial success** (Settings: config saved but a hotkey failed to register —
  `hotkey_errors` non-empty) counts as **saved → `true`**: the data is persisted, so the
  window may close; the warning stays visible until then.

After a successful save, call `markSaved()` to re-baseline.

**Rust close helpers → `destroy()` (Codex P1).** `settings_window::close` and
`alarms_window::close` currently call `window.close()` (which emits `close-requested`). Change
both to `window.destroy()` so the guard-approved programmatic close bypasses the JS
`onCloseRequested` handler entirely — no re-entrancy and no extra capability. The Break-rules
window's `close()` is left as-is (no guard there).

### 5. Capabilities

`capabilities/alarms.json` has `"permissions": []`. `onCloseRequested` registers an event
listener (`plugin:event|listen`), so add **`core:event:allow-listen`** to the alarms
capability (use `core:event:default` instead if the returned unlisten is ever called — it
bundles listen + unlisten). Settings' `core:default` already covers event listen. **No**
window-event-specific or `core:window:allow-destroy` permission is needed, because the close
itself runs in Rust via `destroy()` (§4), not through a JS window API. (Verified against the
Tauri v2 core-permissions reference during Codex review.)

### 6. i18n keys (en / zh-Hant)

| Key | en | zh-Hant |
| --- | --- | --- |
| `confirm.unsaved_title` | Save changes before closing? | 關閉前要儲存變更嗎？ |
| `confirm.unsaved_msg` | Your changes will be lost if you don't save them. | 若不儲存，您的變更將會遺失。 |
| `confirm.save` | Save | 儲存 |
| `confirm.dont_save` | Don't Save | 不儲存 |
| `confirm.cancel` | Cancel | 取消 |

(`common.close` etc. already exist; reuse where possible.)

## Behavior summary

| State | Close button | Window X / Alt+F4 |
| --- | --- | --- |
| No edits | closes | closes |
| Edits, choose **Save** | save → close (stays open if save fails) | same |
| Edits, choose **Don't Save** | close, discard | same |
| Edits, choose **Cancel** | stays open | stays open |

## Edge cases

- **Save fails** (exception): error shown, window stays open, dirty preserved.
- **Save partial** (hotkey error): persisted → treated as saved → closes.
- **Rapid double close** (X then X): the `inFlight` guard (§3) coalesces — only one modal.
- **Modal already open**: further close requests are ignored until it resolves.
- **Other-window rule toggle** while Settings open + clean: re-baselined on focus, no false prompt.
- **Alarms hidden fields** (fields for a non-selected repeat mode) are canonicalized away by
  `collectAlarms()` and aren't user-editable while hidden, so they can't produce a missed
  dirty edit — accepted (Codex P2).
- **Dirty Settings save vs a concurrent other-window rule toggle:** because focus skips the
  rules refresh while dirty (to protect the active window's edits), a later **Save** writes the
  Settings grid's rules and can revert a rule toggled in the Break-rules window meanwhile.
  **Explicitly accepted** — this is the existing "concurrent multi-window edit" caveat in
  CLAUDE.md. The guard prevents silent loss of the *active* window's edits; it does not merge
  edits across windows. (Codex second-pass blocker, resolved by dropping the over-promise
  rather than adding cross-window merge logic — the resolution Codex itself sanctioned.)

## Testing / verification

Engine logic is unchanged, so no `restee-core` tests. Manual verification via
`npm run tauri dev`, for **both** Settings and Alarms and **both** close triggers:

1. Open, change nothing, Close / X → closes immediately (no modal).
2. Edit a field, Close / X → modal appears.
3. Modal **Cancel** → window stays, edit intact.
4. Modal **Don't Save** → window closes; reopen → edit gone.
5. Modal **Save** → window closes; reopen → edit persisted.
6. Edit, **Save** in modal with a deliberately bad hotkey (Settings) → persists + closes, warning was shown.
7. Settings: edit a hotkey, toggle a rule in the Break-rules window, refocus Settings, Close
   → modal still appears and the hotkey edit is **preserved** (focus did not discard it).
   Choosing **Save** here writes the Settings grid's (stale) rules and may revert that
   concurrent other-window toggle — the accepted multi-window caveat (see Edge cases).

## Files touched

- `src/confirm-save.ts` (new) — modal.
- `src/unsaved-guard.ts` (new) — shared guard installer.
- `src/main.ts` — `save()` returns bool; install guard; async focus handler that skips refresh when dirty + re-baselines when clean; Close button + window X route through the guard.
- `src/alarms.ts` — `save()` returns bool; install guard; Close button + window X route through the guard.
- `src/styles.css` — modal styles.
- `src/i18n.ts` — new `confirm.*` keys.
- `src-tauri/src/settings_window.rs`, `src-tauri/src/alarms_window.rs` — `close()` uses `window.destroy()` instead of `window.close()`.
- `src-tauri/capabilities/alarms.json` — add `core:event:allow-listen`.

## Codex review (2026-06-03)

Independent read-only review (gpt-5.5, high reasoning) folded in:
- **P1 (capability):** resolved §5 — `core:event:allow-listen` on alarms.
- **P1 (close re-entrancy):** resolved §3/§4 — Rust `destroy()` instead of JS `close()` + flag.
- **P1 (focus discards rule edits):** resolved §2 — async focus handler skips refresh while dirty.
- **P2 (await refresh before baseline; in-flight guard):** resolved §2 / §3.
- **P2 (alarms hidden fields):** accepted (canonicalized + not user-editable).
- **P3 (saved config + failed hotkey = close):** confirmed sound.

Second pass (blocker-only) flagged that test 7 over-promised "Save doesn't clobber the toggle"
while §2 skips refresh-while-dirty. **Resolved** by dropping that guarantee (test 7 + Edge
cases now state the accepted multi-window caveat); no merge machinery added — the resolution
Codex offered.
