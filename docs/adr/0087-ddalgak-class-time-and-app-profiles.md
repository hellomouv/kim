# ADR 0087: Ddalgak window-class and time conditions, no modifier conditions, per-app profiles

- Status: accepted
- Date: 2026-11-12
- Issue: #100 (follow-up of Step 77, #82, ADR 0078); requirements `DDALGAK-FR-072`,
  `DDALGAK-SEC-011`

## Context

Step 77 gave `when.apps`, `except_apps` and `window_title`. #100 asked for AutoHotkey's
`ahk_class`, time conditions "when sensible", modifier-presence conditions "if a real use case
remains after modes and layers", and opt-in profile switching per app.

## Decision

1. **`when.window_class`** `{contains | equals}`: `GetClassNameW` of the foreground window, read
   at trigger time like the title, only for a known (not elevated, not Ddalgak's own) process;
   lowercase comparison; ranks like a title in §7.1.
2. **`when.time`** `{from, to, days}`: the local Windows clock (`GetLocalTime`), `from` up to
   `to`, crossing midnight when `to` < `from` (the range belongs to the day it starts). No
   network and no other clock source; an unknown clock never matches. The simulator tests
   against `local_time` from `--context`, otherwise now.
3. **No modifier- or lock-key-presence conditions.** Holding a key is what modes with `hold`
   layers express (and they release everything on every path); the remaining case, a lock key's
   toggle state (`GetKeyState("CapsLock", "T")`), cannot be read reliably from the low-level
   hook's thread, whose key state is not the foreground's. A condition that is sometimes wrong
   is worse than none. Revisit if the hook tracks lock state itself.

4. **Per-app profiles** (`settings.app_profiles {enabled, map: [{apps, profile}]}`), off unless
   the user enables it. The host loads the mapped profile **in memory** (`rules::load_profile`)
   instead of rewriting `rules.json` on every focus change, so the file keeps the user's choice
   and a crash never leaves a per-app profile behind. The foreground watcher posts a message; a
   400 ms one-shot timer debounces rapid focus changes (no polling). Never decided by an elevated
   or unknown window, a locked session or the secure desktop. Shown in `status.profile`
   (`saved`, `by_app`, `app_profiles`) and the tray tooltip; turning it off restores the saved
   profile.

## Consequences

- Rules v1 `when` gains two fields (additive; older versions keep such rules inactive with
  `condition_unsupported`).
- The editor's conditions section gains the class and a time window; its Profiles page gains
  *Switch profiles by program*.
- Not done: per-app profiles while the engine is off (no watcher then), and switching by window
  title or class (programs only).
