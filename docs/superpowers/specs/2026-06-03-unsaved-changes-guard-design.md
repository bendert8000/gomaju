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

**Settings focus-refresh edge case.** `main.ts` already re-pulls the rules grid from disk on
window focus (`refreshRulesFromDisk`) so edits made in the Break-rules window show up.
After that re-render, **re-baseline only if the form was not already dirty**:

```
on focus:
  wasDirty = isDirty()
  refreshRulesFromDisk()          // updates `current`, re-renders rules grid
  if (!wasDirty) baseline = JSON.stringify(collect())   // stay "clean" w.r.t. new disk state
```

This keeps a rule toggled in the other window from masquerading as an unsaved edit, while
genuine in-progress edits remain flagged.

### 3. Shared guard installer — `src/unsaved-guard.ts`

```ts
interface GuardHooks {
  collect: () => unknown;        // current form -> serializable snapshot
  save: () => Promise<boolean>;  // true on success (persisted)
  close: () => void;             // perform the actual window close
}
export function installUnsavedGuard(hooks: GuardHooks): {
  markSaved: () => void;         // re-baseline after a save / load
  refreshBaselineIfClean: () => void; // for the focus-refresh case
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

Wiring:
- In-app **Close** button → `requestClose()` (replaces the direct `invoke("cmd_close_*")`).
- **Window X / Alt+F4** → `getCurrentWindow().onCloseRequested((e) => { e.preventDefault(); requestClose(); })`.
- `close()` calls the existing `cmd_close_settings` / `cmd_close_alarms` command, guarded by
  a module-level `closing` flag so the resulting `close-requested` re-entry passes through
  without re-prompting:

```
let closing = false;
onCloseRequested((e) => { if (closing) return; e.preventDefault(); requestClose(); });
function close() { closing = true; invoke("cmd_close_*"); }
```

Reusing `cmd_close_*` (rather than `destroy()`) keeps the alarms capability minimal: only an
event-listen permission is added, not a window-close/destroy permission.

### 4. Small refactors

`save()` in `main.ts` and `alarms.ts` returns `Promise<boolean>`:
- `true` when `cmd_save_*` resolves (config/alarms persisted).
- `false` when it throws (error message already shown).
- **Partial success** (Settings: config saved but a hotkey failed to register —
  `hotkey_errors` non-empty) counts as **saved → `true`**: the data is persisted, so the
  window may close; the warning stays visible until then.

After a successful save, call `markSaved()` to re-baseline.

### 5. Capabilities

`capabilities/alarms.json` currently has `"permissions": []`. Add the minimal permission
needed for `onCloseRequested` (close-event listening). Settings already has `core:default`,
which covers it. **Exact permission identifier to be confirmed against the Tauri v2 schema
during implementation** (candidate: `core:event:default` or `core:event:allow-listen`);
verify the window still closes via `cmd_close_alarms` afterward.

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
- **Rapid double close** (X then X): `closing` flag / modal-open guard prevents a second modal.
- **Modal already open**: ignore further close requests until it resolves.
- **Other-window rule toggle** while Settings open + clean: re-baselined on focus, no false prompt.

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
   → modal still appears (hotkey edit), and Saving does not clobber the toggle.

## Files touched

- `src/confirm-save.ts` (new) — modal.
- `src/unsaved-guard.ts` (new) — shared guard installer.
- `src/main.ts` — `save()` returns bool; install guard; focus re-baseline; Close button routes through guard.
- `src/alarms.ts` — `save()` returns bool; install guard; Close button routes through guard.
- `src/styles.css` — modal styles.
- `src/i18n.ts` — new `confirm.*` keys.
- `src-tauri/capabilities/alarms.json` — add close-event listen permission.
