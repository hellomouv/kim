# ADR 0074 — Noeul capture history and pin backup

- **Status:** Accepted (Step 60, 2026-11-12)
- **Requirements:** `NOEUL-FR-050`, `NOEUL-FR-051`, `NOEUL-FR-066`, `NOEUL-FR-067`,
  `NOEUL-SEC-001`, `NOEUL-SEC-005`, `NOEUL-SEC-006`, `NOEUL-SEC-008`
- **Builds on:** [ADR 0006](0006-state-and-configuration-storage.md) (the user-only data folder),
  [ADR 0035](0035-noeul-smart-selection-and-precision.md) (the rectangle history),
  [ADR 0041](0041-noeul-privacy-tools.md) (redactions are final in every output)

## Decision

1. **Memory history by default.** A capture that ends with pin, copy or save keeps **the picture
   it produced** — its annotations and redactions drawn in, never the raw frozen pixels under a
   redaction — numbered, in memory, bounded by `[history] count` (1–100, default 20) and
   `memory_mb` (16–2048, default 256; the oldest go first; a single picture larger than the
   budget is not kept). Cancelled captures join only with `keep_cancelled = "on"`.
2. **Private captures.** `capture.private` (`P`, overlay) marks the capture: it joins neither
   the pictures nor the rectangles, in memory or on disk.
3. **Disk history is opt-in.** `[history] disk = "on"` writes each kept picture as PNG (no
   metadata) to `%LOCALAPPDATA%\mouv\noeul\history\`, with an index (schema `noeul.history` v1,
   SHA-256 per picture), pruned by count and `keep_days`, read back at the next start. Turning it
   off stops writing; existing files stay until `noeul history clear` (never deleted silently).
   No folder choice: the user-only data folder is the point (Snipaste's "choose a folder" is
   intentionally not offered).
4. **Pin backup is opt-in and offered, never automatic.** `[pins] backup = "on"`: a change to the
   pins arms a one-second timer (no polling) and the pins are written (schema `noeul.pins` v1:
   place, zoom, turn, mirror, opacity, thumbnail, visibility, group, source; pictures in
   `pins\img\<sha256>.png`, deduplicated). Annotations are **drawn into** the backed-up picture:
   a redaction never sits on disk over the pixels it hides, text is never stored as text.
   At the next start Noeul **offers** the pins (a notice, the tray item "Restore pins from last
   time", `noeul pin restore`); until restored or cleared they stay in the backup. A restore
   skips pins that are open (stable keys, no duplicates), checks each picture (decoded and
   compared with its content key; damaged → skipped and reported), moves a pin no monitor shows
   onto one, never restores click-through, and never captures (`NOEUL-SEC-001`).
5. **Integrity.** Temporary file + rename for every write; a set-aside `*.corrupt-<time>` for an
   unreadable index (kept, never parsed again); a newer schema version leaves the store
   read-only.
6. **CLI.** `noeul history list|pin [n]|forget <n>|clear`, `noeul pin restore`, `noeul pin backup
   status|save|clear`; clearing and forgetting ask first (`--yes`); `history clear`, `pin backup
   clear` and `pin backup status` also work without a running Noeul, so private data can always
   be removed. New error code `history_entry_not_found` (3).

## Consequences

- An in-memory copy per capture costs memory (bounded, freed by `history clear`); the real-instance
  memory checks of earlier Steps now count it explicitly.
- Editable annotations are not restored after a restart (they come back drawn in); the pin's
  own editing within a session is unchanged.

## Not done (tracked)

Auto-save of every capture to a folder (`HIST-06`), a Clear item in the tray and a settings window
for these settings, a searchable visual history. **Not tested:** a real reboot (the restart tests
quit and start the instance), multi-monitor relocation on real hardware (the synthetic desktop
and the unit tests cover the rule).
