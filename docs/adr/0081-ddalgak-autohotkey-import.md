# ADR 0081 — Importing AutoHotkey v2 scripts: a read-only converter feeding the profile import

Status: Accepted (Step 80) · Requirements: `DDALGAK-FR-063`, `DDALGAK-SEC-013` · Related:
[DDALGAK_AHK_MIGRATION](../DDALGAK_AHK_MIGRATION.md), [DDALGAK_AHK_MATRIX](../DDALGAK_AHK_MATRIX.md),
ADR 0052 (profiles and import)

## Context

People moving from AutoHotkey want their hotkeys, remaps and hotstrings in Ddalgak without
rewriting them. AutoHotkey scripts are programs: an importer that ran or evaluated them would be
an arbitrary-code runtime, and one that guessed at semantics would silently behave differently.

## Decision

1. **A line-based converter** (`ddalgak::ahk::convert`) reads the script's text and recognizes a
   fixed, declarative subset of AutoHotkey v2 (§2 of the migration guide). It never runs,
   evaluates or includes anything: no AutoHotkey, no shell, no expression evaluation, no
   `#Include`.
2. **Unknown is manual, never approximated.** Every line is classified — converted, converted
   with a warning, needs manual migration, or nothing to convert — with its line, column and
   reason. A hotkey whose action has any unsupported line is not converted at all; hotkeys under
   an unsupported `#HotIf` are listed too.
3. **The output is an ordinary rules document** with one profile, every rule turned off. It goes
   through the existing profile import (Step 75): the preview with permissions, exact targets,
   conflicts and renames; the import adds the profile never active, every rule off and without
   consent. So `ddalgak profile import file.ahk [--dry-run]` and the editor's *Import profile…*
   accept `.ahk` (and `.ah2`) files and add the report (`ahk`) to their output.
4. A script in which nothing converts is refused (`import_invalid`); its dry run still reports
   every line.
5. Fixtures with expected documents and reports (`docs/contracts/examples/ddalgak-ahk-v1`) are the
   reference; a test reproduces them and validates and loads the result.

## Consequences

- The importer cannot cause side effects, and imported rules cannot act before the user reviews
  and enables them.
- The supported subset grows by adding fixtures and cases, never by running code.
- While building the wizard, the import dialog's buttons were moved above the (long) report and
  preview: they were unreachable once the dialog grew taller than the window.
