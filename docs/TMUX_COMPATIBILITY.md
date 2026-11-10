# tmux Compatibility Matrix

Status: **current** (validated, Step 26) · Reference: tmux 3.8 · Matrix version: 2 (2026-11-11; version 1: 2026-11-03) · Owner:
mouv · Tracks [SUBAK-FR-060](PRODUCT_REQUIREMENTS.md)

> **Compatibility statement (Step 26, 2026-11-11; validated by `cargo xtask docs-check`).**
> Subak is a **tmux-like** multiplexer for Windows, **not** a drop-in tmux. Of the 606 rows of
> the tmux 3.8 inventory below, **88 are `implemented`** and **140 `partial`** — each with
> automated evidence that exists in the repository — **352 are `deferred`** (tracked in follow-up
> groups F1–F10 of [#97](https://github.com/hellomouv/kim/issues/97)) and **26 `unsupported`**
> by design; none is left unclassified. Full tmux parity is **not** claimed.
>
> | Level | What you can rely on |
> | --- | --- |
> | **tmux-like UX** | Terminal (session) → internal window → pane → process; `Ctrl+B` prefix with tmux's default keys for windows, panes, layouts, copy mode, the command prompt, buffers and detach; sessions keep running with no client and survive every client event. |
> | **Command-level compatibility** | `subak command '<tmux commands>'` and the window's `Ctrl+B :` read tmux's syntax (quoting, `;`, aliases) for 40 commands: sessions, windows, panes, splits, resize, swap, rotate, layouts and presets, break/join, send-keys, capture, buffers, `setw` for three window options, `set-hook` for 17 hooks, `display-message -p` with a format subset, `last-*`, `list-commands`. Targets are Subak's (`work:1.0`, ids) with tmux's tokens after a Terminal name. Flags not listed in a row are refused with a hint, never ignored. |
> | **Not supported (yet or by design)** | the options engine beyond three window options, most formats and their modifiers, a drawn status line, styles, custom key tables, `source-file`/`if-shell`/`run-shell`, control mode, `switch-client`, linked windows and session groups, emacs copy-mode keys, marked panes, multi-user sockets. A tmux config file does not load. |
>
> Before Step 26 this matrix was an inventory and a plan; Steps 10–25 delivered the rows above.

## 0. How to read this matrix

**Reference.** The inventory follows the official tmux manual page of the **tmux 3.8 release**
(git tag `3.8`, commit `7f2a35ad3321f9ba57a1062ca73b1f3ff26aca53`; `tmux.1` revision 1.1173,
`$Mdocdate: September 23 2026`), fetched from
<https://raw.githubusercontent.com/tmux/tmux/3.8/tmux.1>. Where the manual page is incomplete,
the same tag's sources were used: commands and aliases from `cmd-*.c` (92 commands), default
key bindings from `key-bindings.c`, option names from `options-table.c`. tmux `master` at commit `a73616d781f2` (2026-10-30) was also read; its
unreleased changes are listed in §24 and are **not** part of the matrix.

**Terms.** Subak's object model is Terminal → internal Window → Pane → process
([SUBAK_PHILOSOPHY.md](SUBAK_PHILOSOPHY.md) §2). A Subak **Terminal** is a tmux **session** and
appears as one GUI tab. A Subak internal **Window** is a tmux **window**; it is *not* a GUI tab.

**Columns.**

| Column | Meaning |
| --- | --- |
| tmux feature | What tmux does, in a few words |
| tmux reference | Command (alias), key, option, format or syntax, as in tmux 3.8 |
| Subak equivalent (planned) | How Subak intends to provide it; for `deferred`/`unsupported` the *Rationale* |
| Status | One of the five values below |
| Target Step | Step issue expected to deliver it (`10, 12` = Host part in 10, CLI part in 12) |
| Evidence | Automated test name/path and date (see §26); `—` until then |

**Status values.** `planned` — intended, not built · `partial` — some documented behaviour has
automated evidence · `implemented` — the whole row has automated evidence · `deferred` — not
planned before Step 26; may be revisited · `unsupported` — will not be provided, by design.
`implemented` is used since Step 11 (four environment rows) and Step 12 (invocation, `has-session`, `new -d`); Step 13 made six scrollback rows `partial`; Step 15 made five client-view rows `partial`; Step 21 made `rename-window` `implemented` and ten window and pane rows `partial`. Step 25 ([ADR 0059](adr/0059-subak-advanced-tmux-behaviours.md)) made 61 of its rows `implemented` or `partial` and moved the other 292 to `deferred`, each naming a follow-up group F1–F10 of [#97](https://github.com/hellomouv/kim/issues/97); Subak does **not** claim full tmux parity.

**Target Steps.**

| Step | Scope |
| --- | --- |
| 10 | Session/Terminal CRUD in the Host |
| 11 | Pane programs and their environment ([PTY_PROCESSES](PTY_PROCESSES.md)) |
| 12 | Subak CLI: `new`, `ls`, `info`, `attach`, `detach`, `rename`, `kill`, `host status` |
| 13 | Scrollback and replay (`capture-pane`, `clear-history`) |
| 21 | Internal Windows and Panes (create, list, select, rename, kill) |
| 22 | Splitting and layout (split, resize, swap, zoom, rotate, basic layouts) |
| 23 | Prefix and key-binding engine; default key table; `Ctrl+B Ctrl+B` literal prefix |
| 24 | Command prompt, target syntax, copy mode, paste buffers, join/break-pane, detach |
| 25 | Advanced: layout presets, linked windows, synchronize-panes, command chains, hooks, options, status line, alerts, `switch-client`, `if-shell`/`run-shell`, `display-panes`, `choose-tree`, environment, `source-file` |
| 26 | Full validation and the published compatibility statement |
| post-26 | Deferred; no Step assigned |

**Conventions.** "Host op" = an operation of the Host's IPC API (Step 10), later exposed by the
CLI and GUI. "CLI verb TBD" = the CLI spelling is decided in the listed Step; fixed names are the Step 12
verbs above and the Step 13 verbs `subak capture`, `subak history` and `subak clear-history` (ADR 0030). Keys written `Ctrl+B x` are Subak GUI keys; `C-b x` are tmux keys.
"tmux-syntax commands" = tmux command lines accepted by Subak's prompt (`Ctrl+B :`) and by
`source-file`; a CLI entry point for them is TBD.

## 1. Invocation (`tmux` command-line flags)

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Run a command against the running server | `tmux <command> [flags]` | `subak <verb>` CLI with human and `--json` output (KIM-FR-010). *Partial:* `subak new\|ls\|info\|status\|attach\|detach\|rename\|stop\|kill\|rm` ([SUBAK_CLI](SUBAK_CLI.md)); a tmux-syntax entry point later (Steps 24–25) | partial | 12 | `crates/subak/tests/subak_cli.rs::every_command_answers_one_schema_valid_document_with_stable_exit_codes` (2026-11-04) |
| Run the default command when none is given | `tmux` (no args) → `default-client-command` (`new-session`) | *Rationale:* nothing starts implicitly (ADR 0027 §2): bare `subak` prints help; `subak new --attach` is the explicit equivalent | unsupported | — | — |
| Shortest unambiguous command prefix | e.g. `tmux list-se` | Accepted by the tmux-syntax parser (prompt, `source-file`) *Rationale for the move:* Subak takes full names and tmux aliases only; prefixes would guess. *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Force 256 colours | `-2` | Always full colour in the GUI; no flag needed *Rationale:* the GUI always renders 24-bit colour and the console client passes the program's output through; a flag would change nothing. *Step 26 validation:* reclassified. | unsupported | — | — |
| UTF-8 output | `-u` | Always UTF-8 (KIM-NFR-013); no flag needed. *Partial:* the CLI's output and `--json` are UTF-8, and `attach` switches its console to code page 65001 and restores it; the GUI later | partial | 12 | `crates/subak/tests/subak_cli.rs::console_attach_restores_the_console_on_every_exit_path_it_survives` (2026-11-04) |
| Control mode | `-C`, `-CC` | *Rationale:* control mode is deferred (§19); Subak's own IPC and `--json` CLI serve integrations | deferred | post-26 | — |
| Run a shell command as a login shell | `-c shell-command` | *Rationale:* tmux-as-login-shell has no Windows analogue | unsupported | — | — |
| Behave as a login shell | `-l` | *Rationale:* no-op compatibility flag for login shells; no Windows analogue | unsupported | — | — |
| Do not daemonize the server | `-D` | `subak host` in the foreground for debugging (flag TBD) *Step 26 validation:* `subak host serve` runs the Host in the foreground. | implemented | 10 | `crates/subak/tests/host_ipc.rs::a_pipe_squatter_is_refused_before_any_data_is_sent` (2026-11-11) |
| Alternative configuration file | `-f file` | Subak config path option (TBD); a tmux-syntax file is read with `source-file` *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Help | `-h` | `subak --help` (KIM-FR-011) | implemented | 12 | `crates/subak/tests/cli.rs::help_is_honest_about_the_prototype_status` (2026-11-04) |
| Named or explicit server socket | `-L socket-name`, `-S socket-path` | *Rationale:* no socket compatibility; one Host per user over local, same-user IPC (SUBAK-SEC-010) | unsupported | — | — |
| Do not start the server | `-N` | `subak new --no-start` never starts the Host (`host_not_running` instead); every other command never starts one | implemented | 12 | `crates/subak/tests/subak_cli.rs::every_command_answers_one_schema_valid_document_with_stable_exit_codes` (2026-11-04) |
| Declare outer-terminal features | `-T features` | *Rationale:* applies to an outer terminal; the GUI renders natively. Revisit if a console-rendering client is built | deferred | post-26 | — |
| Print version | `-V` | `subak --version` | implemented | 12 | `crates/subak/tests/cli.rs::version_prints_package_version_and_succeeds` (2026-11-04) |
| Verbose logging | `-v`, `-vv` | `--verbose` (KIM-FR-011); local Host log without terminal content (KIM-SEC-012). *Partial:* `-v` adds client diagnostics on stderr (endpoint, error codes, the key records an attach reads); the Host log later | partial | 12 | `crates/subak/tests/conpty_host.rs::console_attach_sends_raw_keys_and_follows_window_resizes` (2026-11-04) |
| Exit status and error messages | exit codes, `no server running`, etc. | Documented exit codes and stable error codes (KIM-FR-011; [CLI_CONTRACT §5–§6](CLI_CONTRACT.md), `subak-cli-v1`) | implemented | 12 | `crates/subak/tests/subak_cli.rs::every_command_answers_one_schema_valid_document_with_stable_exit_codes` (2026-11-04) |

## 2. Commands (all 92, with aliases)

Grouped by the manual page section that documents them. `set-window-option` and
`show-window-options` are registered in `cmd-*.c` but no longer documented in `tmux.1`.

### 2.1 Clients and sessions

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Attach a client to a session | `attach-session` (`attach`) | `subak attach <terminal>`; GUI opens or focuses the Terminal's tab. *Partial:* `subak attach [<terminal>]` with `-r` as `--read-only` (Step 12); `subak-gui [<terminal>]` shows the Host's Terminals as tabs and opens that one (Steps 16–17, [SUBAK_GUI](SUBAK_GUI.md)); `-d` later | partial | 12, 16, 17 | `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04), `crates/subak/tests/gui_window.rs::the_window_opens_an_existing_terminal_and_types_into_its_process_without_a_new_shell` (2026-11-04) |
| Detach a client | `detach-client` (`detach`) | `subak detach`; closing the GUI is a detach (SUBAK-FR-015). *Partial:* `Ctrl+B d` and `subak detach <terminal>` / `--client <id>` (IPC `client.detach`); closing (or crashing) the Step 16 window is a detach, and a window detached by `subak detach` shows a banner and reattaches on Enter | partial | 12, 16 | `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04), `crates/subak/tests/subak_cli.rs::client_detach_over_ipc_ends_only_attachments_and_tells_the_detached_client` (2026-11-04), `crates/subak/tests/gui_window.rs::closing_or_killing_the_window_never_ends_the_host_or_the_pane` (2026-11-04) |
| Test whether a session exists | `has-session` (`has`) | `subak info <terminal>` exits 0, or 3 (`target_not_found`) | implemented | 12 | `crates/subak/tests/subak_cli.rs::every_command_answers_one_schema_valid_document_with_stable_exit_codes` (2026-11-04) |
| Kill the server and every session | `kill-server` | Kill every Terminal (explicit, confirmed when busy; SUBAK-FR-014); the Host then exits (SUBAK-FR-002). CLI verb TBD *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Kill a session | `kill-session` | Host delete op; `subak kill <terminal>` with `--dry-run` and `--force` (SUBAK-FR-014). *Partial:* `terminal.kill` and `subak kill <terminal>` (`--dry-run`, consent or `--yes`; busy Terminals need it); GUI `Ctrl+W` (Step 17: by id, asks when busy); `-a`, `-C` not yet | partial | 10, 12, 17 | `crates/subak/tests/session_lifecycle.rs::stop_kill_and_rm_have_distinct_explicit_guarded_meanings`, `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04), `crates/subak/tests/gui_window.rs::ctrl_t_creates_and_ctrl_w_closes_asking_first_when_busy_never_hitting_another_terminal` (Step 17) |
| List clients | `list-clients` (`lsc`) | `subak host status` lists attached clients. *Partial:* `subak status` and `subak host status` list every client with its id, kind, pid and attachments; `-F`/`-t` not yet | partial | 12 | `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04) |
| List commands | `list-commands` (`lscm`) | List supported tmux-syntax commands with usage *Rationale for the move:* with the help of Step 25. *Partial:* `list-commands` (`lscm`) lists every command Subak reads with its aliases; no `-F`. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| List sessions | `list-sessions` (`ls`) | `subak ls` (human and `--json`; SUBAK-FR-012). *Partial:* `subak ls` (human and `--json`); Claude status (Step 28) and `-F`/`-f` not yet | partial | 12 | `crates/subak/tests/session_lifecycle.rs::zero_one_and_many_terminals_have_isolated_process_trees`, `crates/subak/tests/subak_cli.rs::every_command_answers_one_schema_valid_document_with_stable_exit_codes` (2026-11-04) |
| Lock a client | `lock-client` (`lockc`) | *Rationale:* the OS lock screen protects the desktop; Subak never locks a session | unsupported | — | — |
| Lock the server | `lock-server` (`lock`) | *Rationale:* as `lock-client` | unsupported | — | — |
| Lock a session | `lock-session` (`locks`) | *Rationale:* as `lock-client` | unsupported | — | — |
| Create a session | `new-session` (`new`) | Host create op from a profile; `subak new` and GUI `Ctrl+T` (SUBAK-FR-011). *Partial:* `terminal.create` and `subak new [<name>] [-- <command> \| --command-line <line>]`; GUI `Ctrl+T` (Step 17, the Host's default shell); profiles (Step 33) not yet | partial | 10, 12, 17 | `crates/subak/tests/session_lifecycle.rs::zero_one_and_many_terminals_have_isolated_process_trees`, `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04) |
| Redraw, resize or pan a client | `refresh-client` (`refresh`) | Redraw and client flags; control-mode parts (`-A`, `-B`, `-C`) deferred with §19 *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Rename a session | `rename-session` (`rename`) | Host rename op; `subak rename` and GUI tab rename. *Partial:* `terminal.rename` and `subak rename` (also while clients are attached); GUI rename in place (`Ctrl+Shift+R`, a double click; Step 17) | partial | 10, 12, 17 | `crates/subak/tests/session_lifecycle.rs::names_are_unique_ignoring_case_and_composition_and_renames_keep_ids`, `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04) |
| Grant other users access | `server-access` | *Rationale:* the Host is per-user by design; IPC is restricted to the same user (SUBAK-SEC-010) | unsupported | — | — |
| Show the message log | `show-messages` (`showmsgs`) | Host message log, local only *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Run commands from a file | `source-file` (`source`) | Parse a tmux-syntax file (§17) *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Start the server | `start-server` (`start`) | The Host starts on demand (SUBAK-FR-002); explicit start verb TBD. *Partial:* `subak new` starts it on demand and `subak host start` explicitly | partial | 10 | `crates/subak/tests/session_lifecycle.rs::exits_restarts_and_reconnects_leave_no_stale_state` (2026-11-04) |
| Suspend a client | `suspend-client` (`suspendc`) | *Rationale:* no SIGTSTP or job control on Windows; minimizing the GUI is the analogue | unsupported | — | — |
| Switch a client to another session | `switch-client` (`switchc`) | Show another Terminal in the current client; `-T` (key table) is part of Step 23 *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |

### 2.2 Windows and panes

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Enter copy mode | `copy-mode` | Copy mode with vi and emacs tables (§7) *Partial:* `copy-mode` in the prompt and `Ctrl+B [`; its flags are Step 25. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Break a pane out into a new window | `break-pane` (`breakp`) | Move a Pane into a new internal Window *Partial:* `break-pane [-d] [-n name] [-s pane]` / IPC `pane.break`, `Ctrl+B !`; `-a`/`-b`/`-P`/`-F` later. | partial | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Capture pane contents | `capture-pane` (`capturep`) | `subak capture`: screen/scrollback as text or JSON (SUBAK-FR-012, ADR 0030). *Partial:* `subak capture [<target>]` and IPC `pane.capture` (screen, history ranges, text or VT, joined rows, redaction by default; also of a stopped Terminal, [SCROLLBACK §4](SCROLLBACK.md)); buffers (`-b`) and the GUI later | partial | 13 | `crates/subak/tests/scrollback_replay.rs::history_is_bounded_evicts_the_oldest_lines_and_clears_on_request`, `crates/subak/tests/subak_cli.rs::every_command_answers_one_schema_valid_document_with_stable_exit_codes` (2026-11-04) |
| Choose a client from a list | `choose-client` | Client chooser overlay *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Choose a session, window or pane from a tree | `choose-tree` | Tree chooser overlay; the GUI session manager covers Terminals (SUBAK-FR-032) *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Customize options interactively | `customize-mode` | *Rationale:* a native settings UI is the natural home; revisit after the options engine | deferred | post-26 | — |
| Fuzzy session/window switcher (new in 3.8) | `switch-mode` | Fuzzy Terminal/Window switcher overlay *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Show pane numbers | `display-panes` (`displayp`) | Pane-number overlay; typing a number focuses that Pane *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Find a window by name, title or content | `find-window` (`findw`) | Search Windows by name, title or content *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Join a pane into another window | `join-pane` (`joinp`) | Move a Pane into another Window as a split *Partial:* `join-pane [-bdhv] [-l size] -s pane [-t pane]` / IPC `pane.join`, within one Terminal. | partial | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Kill a pane | `kill-pane` (`killp`) | Close the Pane and end its process tree (SUBAK-FR-016). *Partial:* `subak pane kill <pane>` / IPC `pane.kill` ends only that pane's job, asks (or `--yes`) and acts by id, needs `force` for a busy pane; **deviation:** the last running pane of a Terminal is refused (`subak kill` ends a Terminal); `-a` later | partial | 21 | `crates/subak/tests/windows_panes.rs::only_an_intentional_kill_ends_and_only_its_target` (2026-11-04) |
| Kill a window | `kill-window` (`killw`) | Close the Window and end its Panes' processes. *Partial:* `subak window kill <window>` / IPC `window.kill`, as `kill-pane`; the last running window is refused; `-a` later | partial | 21 | `crates/subak/tests/windows_panes.rs::only_an_intentional_kill_ends_and_only_its_target` (2026-11-04) |
| Select the previous pane | `last-pane` (`lastp`) | Focus the previously active Pane *Rationale for the move:* needs the previous selection kept by the Host. `last-pane`, `Ctrl+B ;`, target `{last}`/`!`. | implemented | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Select the previous window | `last-window` (`last`) | Focus the previously current Window *Rationale for the move:* needs the previous selection kept by the Host. `last-window`, `Ctrl+B l`, target `{last}`/`!`. | implemented | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Link a window into another session | `link-window` (`linkw`) | Show one Window in several Terminals *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| List panes | `list-panes` (`lsp`) | Host op and CLI listing with stable IDs (SUBAK-FR-010). *Partial:* `subak pane ls <terminal|window>` (also `list-panes`) / IPC `pane.list` with ids, indexes, pids and the active pane; `-a`, `-s`, `-F`, `-f` later | partial | 21 | `crates/subak/tests/windows_panes.rs::a_terminal_holds_windows_with_independent_pane_processes` (2026-11-04) |
| List windows | `list-windows` (`lsw`) | Host op and CLI listing with stable IDs. *Partial:* `subak window ls <terminal>` (also `list-windows`) / IPC `window.list`; `-a`, `-F`, `-f` later | partial | 21 | `crates/subak/tests/windows_panes.rs::a_terminal_holds_windows_with_independent_pane_processes` (2026-11-04) |
| Move a pane | `move-pane` (`movep`) | Move a Pane between Windows (like `join-pane`); floating-pane positions (`-P`, `-M`, `-X`, `-Y`) deferred with `new-pane` *Partial:* `move-pane` is an alias of `join-pane`. | partial | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Move or renumber a window | `move-window` (`movew`) | Reorder or move a Window; `-r` renumbers *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Create a window | `new-window` (`neww`) | New internal Window in the current Terminal (not a GUI tab). *Partial:* `subak window new <terminal>` (also `new-window`) / IPC `window.create`: lowest free index, `-n` as `--name`, `-d` as `--no-select`, `-c`/`-e` as `--cwd`/`--env`, a program as argv or `--command-line`; `-a`, `-b`, `-k`, `-S`, `-P` later | partial | 21 | `crates/subak/tests/windows_panes.rs::a_terminal_holds_windows_with_independent_pane_processes` (2026-11-04) |
| Create a floating pane (new in 3.8) | `new-pane` (`newp`) | *Rationale:* floating and modal panes are new in tmux 3.8; the GUI overlay design needs an ADR; tiled panes come first | deferred | post-26 | — |
| Select the next window | `next-window` (`next`) | Focus the next Window; `-a` (next with alert) in Step 25 *Partial:* `Ctrl+B n` in the window; the CLI selects by index (`subak window select`). | partial | 23 | `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Select the next layout | `next-layout` (`nextl`) | Cycle layout presets `next-layout`, `Ctrl+B Space`; tmux's order. | implemented | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11), `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Pipe pane output to a command | `pipe-pane` (`pipep`) | The Host streams pane output (`-O`) or input (`-I`) to a command *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Select the previous layout | `previous-layout` (`prevl`) | Cycle layout presets backwards `previous-layout`. | implemented | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11), `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Select the previous window | `previous-window` (`prev`) | Focus the previous Window *Partial:* `Ctrl+B p` in the window; the CLI selects by index. | partial | 23 | `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Rename a window | `rename-window` (`renamew`) | Rename the internal Window (SUBAK-FR-010): `subak window rename <window> <name>` (also `rename-window`) / IPC `window.rename`; the id stays, names need not be unique, not only digits | implemented | 21 | `crates/subak/tests/windows_panes.rs::a_terminal_holds_windows_with_independent_pane_processes` (2026-11-04) |
| Resize or zoom a pane | `resize-pane` (`resizep`) | Resize a split by cells or `%`; `-Z` zoom; `-M` mouse drag. *Partial:* `subak pane resize <pane> <left|right|up|down> [cells]` (also `resize-pane`) / IPC `pane.adjust`, `subak pane zoom` / `pane.zoom` (`-Z`), and dragging a border in the window (`-M`); `-x`/`-y` absolute sizes and `%` later | partial | 22 | `crates/subak/tests/pane_layout.rs::splits_resize_zoom_swap_and_close_keep_processes_and_real_pty_sizes` (2026-11-11), `crates/subak/tests/gui_window.rs::a_split_window_draws_every_pane_and_its_keys_and_mouse_split_focus_resize_and_zoom` (2026-11-11) |
| Resize a window | `resize-window` (`resizew`) | Manual Window size (with `window-size`) *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Restart a pane's process | `respawn-pane` (`respawnp`) | Start a new process in a dead Pane; `-k` kills a live one (confirmed) *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Restart a window's process | `respawn-window` (`respawnw`) | As `respawn-pane` for a Window *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Rotate panes | `rotate-window` (`rotatew`) | Rotate Pane positions (`-U`, `-D`). *Partial:* `subak window rotate <window> [up|down]` (also `rotate-window`) / IPC `window.rotate`; `-Z` (keep zoom) later | partial | 22 | `crates/subak/tests/pane_layout.rs::rotate_and_basic_layouts_rearrange_without_ending_anything` (2026-11-11) |
| Select a layout | `select-layout` (`selectl`) | Basic layouts in 22; presets, layout strings and `-o` undo in 25. *Partial:* `subak window layout <window> <even-horizontal|even-vertical|tiled>` (also `select-layout`) / IPC `window.select_layout` | partial | 22, 25 | `crates/subak/tests/pane_layout.rs::rotate_and_basic_layouts_rearrange_without_ending_anything` (2026-11-11) |
| Select a pane | `select-pane` (`selectp`) | Focus a Pane, incl. `-L`/`-R`/`-U`/`-D`; `-T` title; `-m`/`-M` mark (24); `-d`/`-e` input off/on (25). *Partial:* `subak pane select <pane>` (also `select-pane`) / IPC `pane.select` by index or id, for every client (the GUI follows); since Step 22 also `subak pane select <pane> <left|right|up|down|next|previous>` / `direction` (by the layout, wrapping), `Alt+Arrow` and a click in the window; `-T` later | partial | 21, 22 | `crates/subak/tests/windows_panes.rs::only_an_intentional_kill_ends_and_only_its_target` (2026-11-04) |
| Select a window | `select-window` (`selectw`) | Focus a Window by index, ID or name; `-T` toggles to last. *Partial:* `subak window select <window>` (also `select-window`) / IPC `window.select` by index, name or id, for every client (the GUI follows); `-T`, `-l`, `-n`, `-p` later | partial | 21 | `crates/subak/tests/windows_panes.rs::only_an_intentional_kill_ends_and_only_its_target` (2026-11-04) |
| Split a pane | `split-window` (`splitw`) | Split `-h`/`-v`, `-l` size or `%`, `-b` before, `-f` full width; GUI split (SUBAK-FR-035). *Partial:* `subak pane split <target> [--horizontal|--vertical] [--before] [--cells N|--percent N] [--no-select] [-- <program> …]` (also `split-window`) / IPC `pane.split`, and `Alt+Shift+Plus` / `Alt+Shift+Minus` in the window; `-f` later | partial | 22 | `crates/subak/tests/pane_layout.rs::splits_resize_zoom_swap_and_close_keep_processes_and_real_pty_sizes` (2026-11-11), `crates/subak/tests/gui_window.rs::a_split_window_draws_every_pane_and_its_keys_and_mouse_split_focus_resize_and_zoom` (2026-11-11) |
| Swap panes | `swap-pane` (`swapp`) | Swap Pane positions (`-U`, `-D`, `-s`/`-t`). *Partial:* `subak pane swap <pane> <other|next|previous>` (also `swap-pane`) / IPC `pane.swap` within one window; across windows with `join-pane` (Step 24) | partial | 22 | `crates/subak/tests/pane_layout.rs::splits_resize_zoom_swap_and_close_keep_processes_and_real_pty_sizes` (2026-11-11) |
| Swap windows | `swap-window` (`swapw`) | Swap Window indexes *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Unlink a window | `unlink-window` (`unlinkw`) | Remove a linked Window from one Terminal *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |

### 2.3 Key bindings

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Bind a key | `bind-key` (`bind`) | Key-binding engine: `-T` table, `-n` root table, `-r` repeatable, `-N` note *Partial:* `subak hotkeys set <action> <keys>` binds actions (with `@prefix` sequences, `repeat` = `-r`) in `hotkeys.json`; arbitrary commands and `-T`/`-n` tables later. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_leader_follows_hotkeys_json_and_profiles_overlay_it` (2026-11-11) |
| List key bindings | `list-keys` (`lsk`) | List bindings and notes (`-N`, `-T`, `-1`, `-a`, `-P`) *Partial:* `subak hotkeys list` and the window's keyboard help (`F1`, `Ctrl+B ?`). | partial | 23 | `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Send keys to a pane | `send-keys` (`send`) | Host send-input op (SUBAK-FR-013) with tmux key names; `-l`, `-H`, `-N`, `-R`; `-X` copy-mode commands in 24; `-M` mouse in 25 *Rationale for the move:* with `pane.send_keys` (Step 24). *Partial:* `send-keys [-l] [-t pane] keys…` / IPC `pane.send_keys` with tmux key names; `-X`, `-N`, `-H`, `-R`, `-M` later. | partial | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Send the prefix key | `send-prefix` | Send `Ctrl+B` (or `prefix2` with `-2`) to the Pane *Partial:* `Ctrl+B Ctrl+B` in the window and the console client; no `send-prefix` command, no `prefix2`. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11) |
| Unbind a key | `unbind-key` (`unbind`) | Remove a binding (`-a` all, `-n`, `-T`, `-q`) *Partial:* `subak hotkeys disable <action>` / `reset`. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_leader_follows_hotkeys_json_and_profiles_overlay_it` (2026-11-11) |

### 2.4 Options, hooks and environment

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Set an option | `set-option` (`set`) | Options engine with server/session/window/pane scopes (§9) *Partial:* the window options `synchronize-panes`, `monitor-activity`, `monitor-bell` (`set -w`, on/off or toggle) / IPC `window.set_option`; no `-g`, other scopes or other options. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Show options | `show-options` (`show`) | Show options and inheritance (`-A`), value only (`-v`) *Partial:* `show-options` shows those window options / IPC `window.options`; no `-A`, `-v`. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Set a window option (legacy) | `set-window-option` (`setw`) | Alias of `set-option -w` *Partial:* `setw` is the same command. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Show window options (legacy) | `show-window-options` (`showw`) | Alias of `show-options -w` *Partial:* `showw` is the same command. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Set a hook | `set-hook` | Hook engine (§12) *Partial:* `set-hook [-u] name command` for 17 hooks (ADR 0059 §5), one command line each, memory only / IPC `hook.set`; no `-a`, `-g`, `-p`, `-w`, `-R`, `hook[N]`. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Show hooks | `show-hooks` | List hooks and subscriptions *Partial:* `show-hooks` / IPC `hook.list` lists the set hooks and the names Subak runs. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Set an environment variable | `set-environment` (`setenv`) | Terminal or global environment (§15) *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Show the environment | `show-environment` (`showenv`) | Show variables; `-s` shell syntax; `-h` hidden *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |

### 2.5 Status line, prompts and menus

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Clear prompt history | `clear-prompt-history` (`clearphist`) | Clear command-prompt history (per type with `-T`) *Rationale for the move:* prompt history commands with Step 25. *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Command prompt | `command-prompt` | `Ctrl+B :` prompt for tmux-syntax commands; `-I`, `-p`, `-1`, `-k`, `-N`, `-T` *Partial:* `Ctrl+B :` in the window (a line of tmux commands, history with Up/Down) and `subak command`; `-I`, `-p`, templates later. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Confirm before running a command | `confirm-before` (`confirm`) | Confirmation prompt (`-p`, `-y`, `-c`); the `x`/`&` confirmation exists from Step 23 *Rationale for the move:* kill keys ask through the Host's busy rule (ADR 0057); a general `confirm-before` with Step 25. *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Show a menu | `display-menu` (`menu`) | *Rationale:* GUI context menus are designed natively first; tmux menu definitions later | deferred | post-26 | — |
| Show a message or print a format | `display-message` (`display`) | Message line; `-p` prints an expanded format to stdout *Partial:* `display-message [-p] [-t target] [format]`: `-p` prints (CLI), otherwise the window's banner; formats as in §10 (a subset); `Ctrl+B i`. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11), `crates/subak/src/tmuxcmd.rs::tests::step_25_commands_tokens_and_formats` (2026-11-11) |
| Show a popup | `display-popup` (`popup`) | *Rationale:* a popup overlay needs a GUI design and ADR | deferred | post-26 | — |
| Show prompt history | `show-prompt-history` (`showphist`) | Show command-prompt history *Rationale for the move:* prompt history commands with Step 25. *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |

### 2.6 Buffers

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Choose a paste buffer | `choose-buffer` | Buffer chooser overlay *Rationale for the move:* tree modes are Step 25. *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Clear pane history | `clear-history` (`clearhist`) | Clear the Pane's scrollback; `-H` also clears hyperlinks. *Partial:* `subak clear-history [<target>]` (asks or `--yes`, `--dry-run`) and IPC `pane.clear_history`; the screen stays; hyperlinks are kept in the history since Step 15 and go with its lines; `-H` alone (hyperlinks only) is not offered yet | partial | 13 | `crates/subak/tests/scrollback_replay.rs::history_is_bounded_evicts_the_oldest_lines_and_clears_on_request` (2026-11-04) |
| Delete a paste buffer | `delete-buffer` (`deleteb`) | Delete a buffer `delete-buffer [-b name]`, `subak buffer delete`, `Ctrl+B -`. | implemented | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| List paste buffers | `list-buffers` (`lsb`) | List buffers (`-F`, `-f`, `-O`) *Partial:* `list-buffers`, `subak buffer ls`, `Ctrl+B #` (names, sizes, a redacted first line); no `-F` formats. | partial | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| Load a buffer from a file | `load-buffer` (`loadb`) | Load a file or stdin (`-`); `-w` also sets the clipboard *Rationale for the move:* file access for buffers is Step 25 (privacy review). *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Paste a buffer | `paste-buffer` (`pasteb`) | Paste into a Pane; `-p` bracketed, `-r`, `-s`, `-d` *Partial:* `paste-buffer [-b name] [-d] [-t pane]`, `subak buffer paste`, `Ctrl+B ]`; `-p` (bracketed), `-r`, `-s` later. | partial | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11), `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Save a buffer to a file | `save-buffer` (`saveb`) | Write a buffer to a file or stdout; `-a` appends *Rationale for the move:* file access for buffers is Step 25 (privacy review). *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Set a paste buffer | `set-buffer` (`setb`) | Set, append (`-a`) or rename (`-n`) a buffer; `-w` clipboard *Partial:* `set-buffer [-a] [-b name] data`, `subak buffer set`; `-n` (rename) and `-w` later. | partial | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| Show a paste buffer | `show-buffer` (`showb`) | Print a buffer `show-buffer [-b name]`, `subak buffer show`. | implemented | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |

### 2.7 Miscellaneous

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Show a big clock in a pane | `clock-mode` | *Rationale:* novelty with low value; the OS clock is visible | deferred | post-26 | — |
| Run a command conditionally | `if-shell` (`if`) | Shell or format (`-F`) condition, then a tmux-syntax command; uses the profile shell, not `/bin/sh` (§18) *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Run a shell command | `run-shell` (`run`) | The Host runs a command (`-b`, `-d` delay, `-C` tmux command, `-c` cwd, `-t` output pane) *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Wait for, signal or lock a channel | `wait-for` (`wait`) | Named channels: wait, `-S` signal, `-L` lock, `-U` unlock *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |

## 3. Notable command flags

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Create or attach if it exists | `new-session -A` | Attach when the named Terminal exists (flag TBD) *Rationale:* `subak new --attach` attaches to the Terminal it creates; attaching to an existing one of the same name instead is not built. *Step 26 validation:* follow-up F6 of #97. | deferred | post-26 | — |
| Create without attaching | `new-session -d` | Detached creation (the CLI default; `--attach` attaches) | implemented | 12 | `crates/subak/tests/subak_cli.rs::every_command_answers_one_schema_valid_document_with_stable_exit_codes` (2026-11-04) |
| Session and first window names | `new-session -s name -n name` | Terminal name and first Window name. *Partial:* the Terminal name (`subak new <name>`); the first window is named after its program and renamed with `subak window rename` (Step 21); `-n` later | partial | 12 | `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04) |
| Start directory, environment, size | `new-session -c`, `-e VAR=val`, `-x`, `-y` | Profile cwd and environment (SUBAK-FR-011); default size. *Partial:* `--cwd`, `--size` and `--env NAME=VALUE` / `--env NAME` / `--unset-env NAME` (Step 11, [PTY_PROCESSES §3](PTY_PROCESSES.md)); profiles with Step 33 | partial | 11 | `crates/subak/tests/session_lifecycle.rs::names_are_unique_ignoring_case_and_composition_and_renames_keep_ids`, `crates/subak/tests/pty_shells.rs::explicit_programs_get_their_arguments_exactly_and_report_exit_codes` (2026-11-04) |
| Print the created object | `new-session -P -F format`, `new-window -P` | `--json` output (KIM-FR-010); `-F` formats in Step 25. *Partial:* `subak new --json` prints the created Terminal (`subak-cli-v1`) | partial | 12 | `crates/subak/tests/subak_cli.rs::every_command_answers_one_schema_valid_document_with_stable_exit_codes` (2026-11-04) |
| Command given as argv (no shell) | `new-window vi file`, `split-window cmd args` | Start the program directly without a shell. *Partial:* `subak window new|pane new|pane split … -- <program> [args…]` (each word quoted, no shell) | partial | 21, 22 | `crates/subak/tests/windows_panes.rs::exits_stop_and_kill_follow_the_hierarchy` (2026-11-04) |
| Session groups | `new-session -t target-session` | *Rationale:* grouped sessions share windows with independent current windows; linked windows (Step 25) come first | deferred | post-26 | — |
| Detach other clients | `attach-session -d`, `new-session -D` | Detach every other client of the Terminal *Partial:* `subak detach <terminal>` (IPC `client.detach`) detaches every client of a Terminal and tells them who did; an `attach -d` flag is not built. *Step 26 validation:* | partial | 12 | `crates/subak/tests/subak_cli.rs::client_detach_over_ipc_ends_only_attachments_and_tells_the_detached_client` (2026-11-11) |
| Detach all others or a whole session | `detach-client -a`, `-s target-session` | Detach selected clients. *Partial:* `-s` as `subak detach <terminal>`, `-t` as `--client <id>`; `-a` (all but the current client) not yet | partial | 12 | `crates/subak/tests/subak_cli.rs::client_detach_over_ipc_ends_only_attachments_and_tells_the_detached_client` (2026-11-04) |
| Send SIGHUP to the client's parent | `attach-session -x`, `new-session -X`, `detach-client -P` | *Rationale:* no SIGHUP on Windows | unsupported | — | — |
| Skip `update-environment` | `attach-session -E`, `new-session -E`, `new-window -E` | Same flag semantics (§15) *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Kill all other sessions, windows or panes | `kill-session -a`, `kill-window -a`, `kill-pane -a` | Multi-target delete, confirmed when busy *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Clear alert flags | `kill-session -C` | Clear bell/activity/silence flags *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Window placement and reuse | `new-window -a`, `-b`, `-d`, `-k`, `-S` | Insert after/before, do not focus, replace, select existing. *Partial:* `-d` as `--no-select`; `-a`, `-b`, `-k`, `-S` later | partial | 21 | `crates/subak/tests/windows_panes.rs::a_terminal_holds_windows_with_independent_pane_processes` (2026-11-04) |
| Split size, side and span | `split-window -l N`, `-l N%`, `-b`, `-f`, `-d` | Split options. *Partial:* `--cells N`, `--percent N`, `--before`, `--no-select`; `-f` (full width) later | partial | 22 | `crates/subak/tests/pane_layout.rs::directional_focus_minimum_sizes_dry_runs_and_refusals` (2026-11-11) |
| Keep zoom while navigating | `-Z` on `select-pane`, `last-pane`, `swap-pane`, `rotate-window`, `split-window` | Zoom-preserving navigation. *Rationale for the move:* Step 22 unzooms on every such change, as tmux does without `-Z`; keeping the zoom is an option of Step 25 *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Capture options | `capture-pane -p -e -J -N -S -E -a -b -C -M -P -q -T` | Text (`-p`), styles (`-e`), joined lines (`-J`), ranges (`-S`/`-E`), alternate screen (`-a`). *Partial:* `-p` (always), `-e` = `--vt`, `-J` = `--join`, `-S`/`-E` = `--start`/`--end` with the same numbering and `-`; `-N`, `-a`, `-b`, `-C`, `-M`, `-P`, `-q`, `-T` later | partial | 13 | `crates/subak/tests/scrollback_replay.rs::history_is_bounded_evicts_the_oldest_lines_and_clears_on_request`, `crates/subak-term/src/screen.rs::tests::capture_ranges_join_and_edges` (2026-11-04) |
| Copy-mode entry options | `copy-mode -u -d -e -H -M -q -S -s -k` | Scroll on entry, exit at bottom, mouse entry, quit all modes *Rationale for the move:* copy-mode flags with Step 25. *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| List filters and sort order | `list-* -f filter -O order -r` | CLI filter and sort; format filters in Step 25 *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Send keys to a client's key table | `send-keys -K -c target-client` | Inject keys into a client's key handling *Rationale for the move:* with custom key tables (Step 25). *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |

## 4. Default key bindings: prefix table (`C-b`)

Complete list from `key-bindings.c` of tmux 3.8 (the manual page lists only part of it). Keys in
the basic table of SUBAK-FR-033 are marked *basic*. A key can only work once the Step 23 engine
exists, so no key row targets a Step before 23.

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Send the prefix key to the program | `C-b C-b` → `send-prefix` | `Ctrl+B Ctrl+B` sends a literal `Ctrl+B` (*basic*) | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Rotate panes forwards | `C-b C-o` → `rotate-window` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Suspend the client | `C-b C-z` → `suspend-client` | *Rationale:* no SIGTSTP on Windows; the key stays unbound | unsupported | — | — |
| Next layout preset | `C-b Space` → `next-layout` | Same key | implemented | 25 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Break pane to a new window | `C-b !` → `break-pane` | Same key Since Step 24. | implemented | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Split top/bottom | `C-b "` → `split-window` | Same key (*basic*) | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| List paste buffers | `C-b #` → `list-buffers` | Same key Since Step 24 (the answer in the pane's place). | implemented | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| Rename the session | `C-b $` → `command-prompt -I'#S' { rename-session }` | Rename the Terminal (and its tab) (*basic*); full prompt in 24 *Partial:* `Ctrl+B $` opens the in-place rename of the tab (the prompt with Step 24). | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11) |
| Split left/right | `C-b %` → `split-window -h` | Same key (*basic*) | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Kill the window (confirm) | `C-b &` → `confirm-before kill-window` | Same key with confirmation (*basic*; SUBAK_PHILOSOPHY §6.1) *Partial:* `Ctrl+B &` asks only when the window runs more than a shell (the Host's busy rule, as `Ctrl+W`); an idle window ends at once. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11) |
| Prompt for a window index | `C-b '` → `command-prompt -pindex` | Same key *Rationale for the move:* prompt templates with Step 25. *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| New floating pane | `C-b *` → `new-pane` | *Rationale:* floating panes deferred (see `new-pane`) | deferred | post-26 | — |
| Toggle floating/tiled pane | `C-b @` → `join-pane` / `break-pane -W` | *Rationale:* floating panes deferred | deferred | post-26 | — |
| Previous session | `C-b (` → `switch-client -p` | Previous Terminal in the current client (GUI analogue: `Ctrl+Shift+Tab`) *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Next session | `C-b )` → `switch-client -n` | Next Terminal (GUI analogue: `Ctrl+Tab`) *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Rename the window | `C-b ,` → `command-prompt -I'#W' { rename-window }` | Rename the internal Window (*basic*); full prompt in 24 *Partial:* `Ctrl+B ,` renames the active internal window in the tab's place (the prompt with Step 24). | partial | 23 | `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Delete the latest paste buffer | `C-b -` → `delete-buffer` | Same key | implemented | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| Move the window (prompt) | `C-b .` → `command-prompt { move-window }` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Describe a key binding | `C-b /` → `list-keys -1N` | Same key *Rationale for the move:* with the command prompt of Step 24. *Rationale for the move:* with the help of Step 25 (`F1` lists every key now). *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Select window 0–9 | `C-b 0` … `C-b 9` → `select-window -t:=N` | Same keys (*basic*) | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Command prompt | `C-b :` → `command-prompt` | Same key | implemented | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Previously active pane | `C-b ;` → `last-pane` | Same key *Rationale for the move:* the Host keeps no previous selection yet (Step 25). Since Step 25. | implemented | 25 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Choose a paste buffer | `C-b =` → `choose-buffer -Z` | Same key *Rationale for the move:* tree modes are Step 25. *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| List key bindings | `C-b ?` → `list-keys -N` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Choose a client to detach | `C-b D` → `choose-client -Z` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Spread panes out evenly | `C-b E` → `select-layout -E` | Same key *Rationale for the move:* `select-layout -E` moved to Step 25. Since Step 25 (`Ctrl+B E`). | implemented | 25 | `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Last session | `C-b L` → `switch-client -l` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Clear the marked pane | `C-b M` → `select-pane -M` | Same key *Rationale for the move:* marked panes are Step 25. *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Set the pane title | `C-b T` → `command-prompt -I'#T' { select-pane -T }` | Same key *Rationale for the move:* pane titles are Step 25. *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Enter copy mode | `C-b [` → `copy-mode` | Same key | implemented | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Paste the latest buffer | `C-b ]` → `paste-buffer -p` | Same key *Partial:* without bracketed paste (`-p`). | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| New window | `C-b c` → `new-window` | New internal Window, not a tab (*basic*) | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Detach the client | `C-b d` → `detach-client` | Same key; processes keep running (*basic*; SUBAK-FR-015) *Step 23:* `Ctrl+B d` closes the window (a detach); the console client follows the user's prefix and `client.detach`. | implemented | 23 | `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11), `crates/subak/tests/subak_cli.rs::attach_detach_keys_follow_the_users_hotkeys_json` (2026-11-11) |
| Find a window (prompt) | `C-b f` → `command-prompt { find-window -Z }` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Show window information | `C-b i` → `display-message` | Same key *Partial:* `Ctrl+B i` shows the default `display-message` text in the banner, not tmux's exact default format. | partial | 25 | `crates/subak/src/tmuxcmd.rs::tests::step_25_commands_tokens_and_formats` (2026-11-11) |
| Last window | `C-b l` → `last-window` | Same key *Rationale for the move:* the Host keeps no previous selection yet (Step 25). Since Step 25. | implemented | 25 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Mark the pane | `C-b m` → `select-pane -m` | Same key *Rationale for the move:* marked panes are Step 25. *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Next window | `C-b n` → `next-window` | Same key (*basic*) | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Next pane | `C-b o` → `select-pane -t:.+` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Customize options | `C-b C` → `customize-mode -Z` | *Rationale:* `customize-mode` deferred | deferred | post-26 | — |
| Previous window | `C-b p` → `previous-window` | Same key (*basic*) | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Show pane numbers | `C-b q` → `display-panes` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Redraw the client | `C-b r` → `refresh-client` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Choose a session | `C-b s` → `choose-tree -Zs` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Show a clock | `C-b t` → `clock-mode` | *Rationale:* `clock-mode` deferred | deferred | post-26 | — |
| Fuzzy-switch window | `C-b Tab` → `new-pane …; switch-mode -wk` | Same key opens the Window switcher (without a floating pane) *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Fuzzy-switch session | `C-b BTab` → `new-pane …; switch-mode -sk` | Same key opens the Terminal switcher *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Choose a window | `C-b w` → `choose-tree -Zw` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Kill the pane (confirm) | `C-b x` → `confirm-before kill-pane` | Same key with confirmation (*basic*) *Partial:* `Ctrl+B x` asks only when the pane runs more than a shell (the Host's busy rule); an idle pane ends at once. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11) |
| Zoom the pane | `C-b z` → `resize-pane -Z` | Same key (*basic*) | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Swap with the previous pane | `C-b {` → `swap-pane -U` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Swap with the next pane | `C-b }` → `swap-pane -D` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Show messages | `C-b ~` → `show-messages` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Copy mode, one page up | `C-b PPage` → `copy-mode -u` | Same key | implemented | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Select pane by direction (repeatable) | `C-b Up`, `Down`, `Left`, `Right` → `select-pane -U/-D/-L/-R` | Same keys (*basic*) *Partial:* `Ctrl+B Up/Down/Left/Right` (and `Alt+Arrow`); not repeatable without the prefix yet. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Even-horizontal layout | `C-b M-1` → `select-layout even-horizontal` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Even-vertical layout | `C-b M-2` → `select-layout even-vertical` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Main-horizontal layout | `C-b M-3` → `select-layout main-horizontal` | Same key | implemented | 25 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Main-vertical layout | `C-b M-4` → `select-layout main-vertical` | Same key | implemented | 25 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Tiled layout | `C-b M-5` → `select-layout tiled` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Main-horizontal-mirrored layout | `C-b M-6` → `select-layout main-horizontal-mirrored` | Same key | implemented | 25 | `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Main-vertical-mirrored layout | `C-b M-7` → `select-layout main-vertical-mirrored` | Same key | implemented | 25 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Next window with an alert | `C-b M-n` → `next-window -a` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Rotate panes backwards | `C-b M-o` → `rotate-window -D` | Same key | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Previous window with an alert | `C-b M-p` → `previous-window -a` | Same key *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Pan the visible part of a larger window | `C-b S-Up`, `S-Down`, `S-Left`, `S-Right` → `refresh-client -U/-D/-L/-R 10` | *Rationale:* only matters when a client is smaller than its window; revisit with multi-client sizing | deferred | post-26 | — |
| Follow the cursor again | `C-b DC` → `refresh-client -c` | *Rationale:* as above | deferred | post-26 | — |
| Resize the pane by 5 (repeatable) | `C-b M-Up`, `M-Down`, `M-Left`, `M-Right` → `resize-pane … 5` | Same keys | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Resize the pane by 1 (repeatable) | `C-b C-Up`, `C-Down`, `C-Left`, `C-Right` → `resize-pane` | Same keys | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Move a floating pane (`move` table) | `C-b g` then `1`–`4`, `M-1`–`M-4`, arrows, `M-` arrows, `0`, `,`, `.` | *Rationale:* floating panes deferred | deferred | post-26 | — |
| Window menu | `C-b <` → `display-menu` (window) | *Rationale:* `display-menu` deferred | deferred | post-26 | — |
| Pane menu | `C-b >` → `display-menu` (pane) | *Rationale:* `display-menu` deferred | deferred | post-26 | — |

## 5. Default key bindings: root table (mouse)

The root table has no default keyboard bindings; all defaults are mouse events and take effect
when `mouse` is on.

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Click focuses a pane; forwarded if the app uses the mouse | `MouseDown1Pane` → `select-pane -t=; send -M` | Click focuses the Pane; mouse reporting forwarded when the program requests it. *Partial:* the client view encodes the program's mouse reports for every tracking mode and encoding, and Shift keeps the mouse for selection ([TERMINAL_EMULATOR §4.3](TERMINAL_EMULATOR.md)); the window (Step 16) and focusing split panes (Step 22) are later | partial | 15, 22 | `crates/subak-term/tests/view.rs::the_view_routes_the_mouse_to_the_program_or_to_selection_and_scrolling` (2026-11-04) |
| Drag selects text | `MouseDrag1Pane` → `copy-mode -M` (or `send -M`) | Drag selection in copy mode *Partial:* a drag selects in the window (Step 19) without entering copy mode. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Ctrl-drag creates a floating pane | `C-MouseDrag1Pane`, `C-MouseDrag1Empty` → `new-pane -M` | *Rationale:* floating panes deferred | deferred | post-26 | — |
| Alt-drag moves a floating pane | `M-MouseDrag1Pane`, `M-MouseDrag1Border` → `move-pane -M` | *Rationale:* floating panes deferred | deferred | post-26 | — |
| Wheel up scrolls history | `WheelUpPane` → `copy-mode -e` (or `send -M`) | Wheel scrolls scrollback; forwarded in the alternate screen or when the program uses the mouse. *Partial:* the client view scrolls its viewport over the Host's history, forwards the wheel to programs that track the mouse and sends cursor keys on the alternate screen when the program enables alternate scroll (1007); the window that delivers the wheel is Step 16 | partial | 13, 15 | `crates/subak-term/tests/view.rs::the_view_routes_the_mouse_to_the_program_or_to_selection_and_scrolling` (2026-11-04) |
| Middle-click pastes | `MouseDown2Pane` → `paste -p` | Middle-click pastes the latest buffer *Partial:* `terminal.middle_click = "paste"` pastes the clipboard (Step 20), not a buffer. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Double-click copies a word | `DoubleClick1Pane` → `select-word; copy-pipe-and-cancel` | Same gesture. *Partial:* word selection and its copy text in the client view (Windows Terminal's word delimiters, wide characters once); the gesture and the clipboard are the GUI's (Step 16; plain text and, on request, HTML Format since Step 19), which copies on an explicit copy rather than on the click (copy-on-select is off, Step 20 adds the option) | partial | 15, 24 | `crates/subak-term/tests/view.rs::word_and_block_selection` (2026-11-04) |
| Triple-click copies a line | `TripleClick1Pane` → `select-line; copy-pipe-and-cancel` | Same gesture. *Partial:* line selection (soft-wrapped rows as one line) and its copy text in the client view; the gesture and the clipboard are the GUI's (Steps 16, 19) | partial | 15, 24 | `crates/subak-term/tests/view.rs::copy_joins_soft_wrapped_rows_but_not_hard_line_breaks` (2026-11-04) |
| Click a border selects the pane | `MouseDown1Border` → `select-pane -t=` | Same gesture. *Rationale for the move:* in Step 22 a press on a border starts a border drag; selecting the pane with it comes with the mouse options of Step 25 *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Drag a border resizes | `MouseDrag1Border` → `resize-pane -M` | Drag the split divider: one `pane.adjust` per cell crossed | implemented | 22 | `crates/subak/tests/gui_window.rs::a_split_window_draws_every_pane_and_its_keys_and_mouse_split_focus_resize_and_zoom` (2026-11-11) |
| Click the window list selects a window | `MouseDown1Status` → `switch-client -t=` | Click a Window in the in-tab window list *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Ctrl-click the window list swaps windows | `C-MouseDown1Status` → `swap-window -t@` | Same gesture *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Wheel over the window list changes window | `WheelUpStatus`, `WheelDownStatus` → `previous-window`, `next-window` | Same gesture *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Pane control buttons (new in 3.8) | `MouseDown1Control9` (kill menu), `Control8` (zoom), `Control7` (float toggle) | *Rationale:* depend on `display-menu` and floating panes | deferred | post-26 | — |
| Session menu on the status-left | `MouseDown3StatusLeft`, `M-MouseDown3StatusLeft` → `display-menu` | *Rationale:* `display-menu` deferred | deferred | post-26 | — |
| Window menu on the window list | `MouseDown3Status`, `M-MouseDown3Status` → `display-menu` | *Rationale:* `display-menu` deferred | deferred | post-26 | — |
| Pane menu | `MouseDown3Pane`, `M-MouseDown3Pane` → `display-menu` | *Rationale:* `display-menu` deferred; right-click forwarding to mouse-aware programs is planned with `MouseDown1Pane` | deferred | post-26 | — |
| Empty-area menu | `MouseDown3Empty`, `M-MouseDown3Empty` → `display-menu` | *Rationale:* `display-menu` deferred | deferred | post-26 | — |
| Pane scrollbar | `MouseDown1ScrollbarUp`, `MouseDown1ScrollbarDown`, `MouseDrag1ScrollbarSlider` | Native GUI scrollbar over the scrollback *Rationale:* the GUI has no scrollbar yet (wheel, keys and copy mode scroll). *Step 26 validation:* follow-up F7 of #97. | deferred | post-26 | — |

## 6. Default key tables overview

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Prefix table | `prefix` (§4) | Subak prefix table behind `Ctrl+B` Since Step 23: the `prefix` leader of the hotkey contract, rebindable. | implemented | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11), `crates/subak/tests/gui_window.rs::the_prefix_table_drives_windows_and_panes_of_the_shown_terminal_only` (2026-11-11) |
| Root table | `root` (§5) | Root bindings; GUI shortcuts (`Ctrl+T` etc.) are handled before it (§23) *Partial:* the window's single-chord shortcuts (`app`/`terminal` scopes) act as the root table; the mouse root bindings are §5. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11) |
| Copy-mode tables | `copy-mode` (emacs), `copy-mode-vi` (§7) | Both tables *Partial:* one table of vi keys and arrows; `Space`, `Enter`, `q`/`Escape` are rebindable `copy-mode` actions; emacs keys later. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Floating-pane move table | `move` | *Rationale:* floating panes deferred | deferred | post-26 | — |
| Custom key tables | `bind -T name`, `switch-client -T name`, `key-table` option | User-defined tables *Rationale for the move:* one leader table in Step 23; more tables with Step 25. *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Key names | `C-`, `^`, `S-`, `M-`, `Up`, `BSpace`, `BTab`, `DC`, `End`, `Enter`, `Escape`, `F1`–`F12`, `Home`, `IC`, `NPage`/`PageDown`/`PgDn`, `PPage`/`PageUp`/`PgUp`, `Space`, `Tab` | Same names in bindings and `send-keys` *Partial:* the hotkey contract's names (`Ctrl+B`, `PageDown`, `char:%`); tmux spellings with `send-keys` (Step 24). | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11) |
| Catch-all binding | `Any` key | Same *Rationale for the move:* with custom key tables (Step 25). *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Repeatable keys | `bind -r`, `repeat-time`, `initial-repeat-time` | Same *Partial:* `repeat: true` bindings repeat for `terminal.repeat_time_ms` (default 500) without the prefix; no `initial-repeat-time`. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::repeatable_prefix_keys_work_again_without_the_prefix_for_the_repeat_time` (2026-11-11) |

## 7. Copy mode (vi and emacs)

Keys come from the `copy-mode-vi` and `copy-mode` tables in `key-bindings.c`. Every copy-mode
command (`send-keys -X <command>`) documented in tmux 3.8 is named in this table.

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Choose vi or emacs keys | `mode-keys` option; default from `VISUAL`/`EDITOR` | Same option and default *Rationale for the move:* the emacs table is Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Send a copy-mode command | `send-keys -X command`, `-N count` | Same *Rationale for the move:* with `send-keys -X`, Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Exit copy mode | vi `q`, `C-c`; emacs `q`, `Escape`, `C-[`, `C-c` → `cancel` | Same keys *Partial:* vi `q`, `Escape`, `C-c`; emacs keys later. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Cursor movement | vi `h` `j` `k` `l`, `BSpace`, `C-h`, arrows; emacs `C-b` `C-f` `C-p` `C-n`, arrows → `cursor-left`, `cursor-right`, `cursor-up`, `cursor-down`, `cursor-down-and-cancel` | Same keys *Partial:* vi `h j k l` and arrows. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Word movement | vi `w` `b` `e` `W` `B` `E`; emacs `M-f` `M-b` → `next-word`, `next-word-end`, `previous-word`, `next-space`, `next-space-end`, `previous-space` | Same keys; `word-separators` honoured *Partial:* vi `w` `b` (and `Ctrl+Left/Right`), with `terminal.word_separators`. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Line positions | vi `0` `^` `$` `Home` `End`; emacs `C-a` `C-e` `M-m` `Home` `End` → `start-of-line`, `end-of-line`, `back-to-indentation` | Same keys *Partial:* vi `0` `^` `$` `Home` `End`. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Screen positions | vi `H` `M` `L`; emacs `M-R` `M-r` `M-l` → `top-line`, `middle-line`, `bottom-line`, `cursor-centre-vertical`, `cursor-centre-horizontal` | Same keys *Rationale for the move:* copy-mode keys of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Paging | vi `C-b` `C-f` `PPage` `NPage`; emacs `M-v` `C-v` `Space` `PPage` `NPage` → `page-up`, `page-down`, `page-down-and-cancel` | Same keys *Partial:* `PPage` `NPage` (vi `C-b` is the prefix). | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Half pages | vi `C-u` `C-d`; emacs `M-Up` `M-Down` → `halfpage-up`, `halfpage-down`, `halfpage-down-and-cancel` | Same keys *Partial:* `C-u` `C-d` move a whole page. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Line scrolling | vi `C-y` `C-e` `K` `J` `C-Up` `C-Down` `z`; emacs `C-Up` `C-Down` `C-l` → `scroll-up`, `scroll-down`, `scroll-down-and-cancel`, `scroll-middle`, `scroll-top`, `scroll-bottom`, `recentre-top-bottom` | Same keys *Rationale for the move:* copy-mode keys of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Exit when scrolled to the bottom | `scroll-exit-on`, `scroll-exit-off`, `scroll-exit-toggle`; `copy-mode -e` | Same *Rationale for the move:* copy-mode options of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| History top and bottom | vi `g` `G`; emacs `M-<` `M->` → `history-top`, `history-bottom` | Same keys vi `g` `G`, `Ctrl+Home/End`. | implemented | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Paragraphs and brackets | vi `{` `}` `%`; emacs `M-{` `M-}` `C-M-f` `C-M-b` → `previous-paragraph`, `next-paragraph`, `next-matching-bracket`, `previous-matching-bracket` | Same keys *Rationale for the move:* copy-mode keys of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Jump to a character | vi and emacs `f` `F` `t` `T` `;` `,` → `jump-forward`, `jump-backward`, `jump-to-forward`, `jump-to-backward`, `jump-again`, `jump-reverse` | Same keys *Rationale for the move:* copy-mode keys of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Search | vi `/` `?` `n` `N`, `*` `#` (word under cursor); emacs `C-s` `C-r` `n` `N` → `search-forward`, `search-backward`, `search-forward-text`, `search-backward-text`, `search-forward-incremental`, `search-backward-incremental`, `search-again`, `search-reverse` | Same keys; regex and plain text; `wrap-search`. *Groundwork (Step 19):* the bounded, incremental search engine over the retained history (`subak_term::search`, ADR 0048), used today by the window's search bar (`Ctrl+Shift+F`, older/newer with wrap-around) *Partial:* `/` `?` open the search bar (Step 19), `n` `N` step through matches; `*` `#` later. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Go to a line | vi `:`; emacs `g` → `goto-line` | Same keys *Rationale for the move:* copy-mode keys of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Selection | vi `Space` `V` `o` `Escape`; emacs `C-Space` `C-g` → `begin-selection`, `select-line`, `select-word`, `other-end`, `clear-selection`, `stop-selection`, `selection-mode` | Same keys. *Groundwork (Step 19):* the view's keyboard selection (mark mode: caret, grow by cells, words, lines, pages) behind the window's `Ctrl+Shift+M` *Partial:* `Space` begins, moves extend, `Escape`/`q` clear and leave; `V`, `o` later. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Rectangle selection | vi `v` `C-v`; emacs `R` → `rectangle-toggle`, `rectangle-on`, `rectangle-off` | Same keys *Rationale for the move:* copy-mode keys of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Copy and append | vi `Enter` `C-j` `D` `A`; emacs `M-w` `C-w` `C-k` → `copy-selection`, `copy-selection-and-cancel`, `copy-selection-no-clear`, `copy-line`, `copy-line-and-cancel`, `copy-end-of-line`, `copy-end-of-line-and-cancel`, `append-selection`, `append-selection-and-cancel` | Same keys; copies to a paste buffer and the Windows clipboard (§16) *Partial:* `Enter` = copy-selection-and-cancel into a new buffer and the clipboard; `D`, `A` later. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Copy through an external command | `copy-pipe`, `copy-pipe-and-cancel`, `copy-pipe-no-clear`, `copy-pipe-line`, `copy-pipe-line-and-cancel`, `copy-pipe-end-of-line`, `copy-pipe-end-of-line-and-cancel`, `pipe`, `pipe-and-cancel`, `pipe-no-clear`; `copy-command` option | Same commands; the command runs in the profile shell *Rationale for the move:* copy-pipe runs programs: Step 25 with a security review. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Marks | vi and emacs `X`, `M-x` → `set-mark`, `jump-to-mark` | Same keys *Rationale for the move:* copy-mode keys of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Repeat count | vi `1`–`9`; emacs `M-1`–`M-9` | Same keys *Rationale for the move:* copy-mode keys of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Shell-prompt navigation (OSC 133) | emacs `M-C-Up` `M-C-Down` → `previous-prompt`, `next-prompt` (`-o`) | Same keys; needs OSC 133 marks from the shell *Rationale for the move:* shell integration is Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Line numbers | emacs `L` → `line-numbers-toggle`, `line-numbers-on`, `line-numbers-off`; `copy-mode-line-numbers` option | Same *Rationale for the move:* copy-mode options of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Position indicator | `P` → `toggle-position`; `copy-mode-position-format` | Same *Rationale for the move:* copy-mode options of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Refresh from the live pane | `r` → `refresh-now`; `refresh-on`, `refresh-off`, `refresh-toggle` | Same *Rationale for the move:* copy-mode keys of Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Mouse in copy mode | `MouseDrag1Pane` (select), `MouseDragEnd1Pane` (copy), `WheelUpPane`/`WheelDownPane` (`send -N5 -X scroll-up/scroll-down`), double/triple click, `scroll-to-mouse` | Same gestures *Rationale for the move:* the window's drag selection (Step 19) works outside copy mode; inside, Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| View mode | Output of a command run from a key binding (e.g. `list-keys`) shown read-only | Read-only view of command output *Partial:* the answers of `list-*`, `show-buffer`, `capture-pane` shown read-only in the pane's place. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Copy-mode appearance | `mode-style`, `copy-mode-*-style` options | Styles from the options engine *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |

## 8. Other pane modes

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Tree mode | `choose-tree` (`-s`, `-w`, `-Z`, `-f filter`, `-O order`, `-F`, `-G`) | Tree chooser overlay *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Chooser keys | `Enter`, `Up`/`Down`, `x`/`X` kill, `t`/`T`/`C-t` tag, `f` filter, `O` sort, `r` reverse, `v` preview, `:` command, `q` exit | Same keys in all choosers *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Client mode | `choose-client` | Client chooser *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Buffer mode | `choose-buffer` (`e` edits with `editor`) | Buffer chooser; editing deferred with `editor` *Rationale for the move:* tree modes are Step 25. *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Switch mode | `switch-mode` (fuzzy match; `Enter`, `Up`, `Down`, `Escape`) | Fuzzy switcher *Step 25 inventory:* follow-up F1 (copy mode and pane modes), #97. | deferred | post-26 | — |
| Customize mode | `customize-mode` | *Rationale:* see §2.2 | deferred | post-26 | — |
| Clock mode | `clock-mode` | *Rationale:* see §2.7 | deferred | post-26 | — |

## 9. Options

### 9.1 Families and mechanics

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Server options | `set -s`, `show -s` | Host-wide settings *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Session options | `set`, `set -g`, `show` | Per-Terminal settings with global defaults *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Window options | `set -w`, `setw`, `show -w` | Per-Window settings *Partial:* three window options (`synchronize-panes`, `monitor-activity`, `monitor-bell`), set per window; shown in Window objects (`options`). | partial | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Pane options | `set -p`, `show -p` | Per-Pane settings *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Global defaults and inheritance | `-g`; `show -A` (inherited values marked `*`) | Same inheritance chain: pane → window → session → global *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| User options | `@name` | Same; usable in formats *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Array options | `name[N]`, `set -a` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Format-expanded values | `set -F` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Set-only-if-unset, quiet, unset | `set -o`, `-q`, `-u`, `-U` (unset in all panes) | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Show modifiers | `show -v`, `-H` (hooks), `-q` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Durable configuration | tmux options live in server memory only | Subak config is human-readable, versioned, preserves unknown keys (SUBAK-FR-046); how runtime `set` persists is decided in Step 25 *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |

### 9.2 Server options

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Key sent for Backspace | `backspace` | Same *Rationale for the move:* a terminal option of Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Maximum automatic buffers | `buffer-limit` | Same *Partial:* 50, fixed. | partial | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| Clear the outer terminal on attach | `clear-on-attach` | *Rationale:* no outer terminal; the GUI redraws from Host state | unsupported | — | — |
| Unicode width overrides | `codepoint-widths`, `variation-selector-always-wide` | Same; important for Korean/CJK (KIM-NFR-013) *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Custom command aliases | `command-alias` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Command for copy-pipe | `copy-command` | Same; default is the Windows clipboard *Rationale for the move:* with copy-pipe, Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Command run by a bare `tmux` | `default-client-command` | Bare `subak` prints help (Step 12, ADR 0027 §2); an option to change that, if any, later *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| `TERM` for new panes | `default-terminal` | `TERM` given to pane processes *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Editor command | `editor` | *Rationale:* only used by buffer editing; revisit with buffer mode extras | deferred | post-26 | — |
| Escape-key delay | `escape-time` | *Rationale:* the GUI receives key events, not an escape byte stream | unsupported | — | — |
| Exit when no sessions remain | `exit-empty` | Fixed behaviour: the Host exits when it owns no Terminals and no client is attached (SUBAK-FR-002) *Step 26 validation:* fixed behaviour, with a grace period (`--idle-exit`). | implemented | 10 | `crates/subak/tests/host_ipc.rs::an_idle_host_exits_after_its_grace_period_and_not_before` (2026-11-11) |
| Exit when no clients are attached | `exit-unattached` | *Rationale:* contradicts Subak's core promise that Terminals outlive clients (SUBAK-FR-003) | unsupported | — | — |
| Extended keys to programs | `extended-keys`, `extended-keys-format` | *Rationale:* Subak sends xterm encoding or win32-input-mode (ADR 0009); CSI u extended keys revisited after Step 23 | deferred | post-26 | — |
| Focus events to programs | `focus-events` | Same. *Partial:* the client view sends `CSI I`/`CSI O` once per change when the program enables 1004 (the inbox ConPTY passes 1004 through, measured on 19045); the window's focus changes are Step 16 | partial | 15, 25 | `crates/subak-term/tests/view.rs::focus_reports_and_hyperlink_hit_test` (2026-11-04) |
| OSC 52 clipboard | `set-clipboard`, `get-clipboard` | Windows clipboard; reading is opt-in (privacy) *Rationale for the move:* clipboard access by programs needs an opt-in (privacy), Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Prompt history file | `history-file`, `prompt-history-limit` | Same, stored in the user profile *Partial:* the prompt keeps its history in memory for the window's life; no file. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Escape-sequence size limit | `input-buffer-size` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Message log size | `message-limit` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Prefix timeout | `prefix-timeout` | Same *Partial:* like tmux's default (no timeout): the prefix waits for the next key, Esc or a click cancels (HOTKEY_CONTRACT §5.3, no timer); not configurable. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through` (2026-11-11) |
| Outer-terminal capabilities | `terminal-features`, `terminal-overrides` | *Rationale:* apply to an outer terminal; the GUI renders natively. Revisit for a console client | deferred | post-26 | — |
| Theme detection and palettes | `theme`, `dark-theme-black`, `dark-theme-blue`, `dark-theme-cyan`, `dark-theme-dark-grey`, `dark-theme-green`, `dark-theme-light-grey`, `dark-theme-magenta`, `dark-theme-red`, `dark-theme-white`, `dark-theme-yellow`, `light-theme-black`, `light-theme-blue`, `light-theme-cyan`, `light-theme-dark-grey`, `light-theme-green`, `light-theme-light-grey`, `light-theme-magenta`, `light-theme-red`, `light-theme-white`, `light-theme-yellow` | *Rationale:* colours and light/dark follow the Kim design system and the OS; tmux palette options revisited later | deferred | post-26 | — |
| User-defined outer-terminal keys | `user-keys` | *Rationale:* as `terminal-overrides` | deferred | post-26 | — |

### 9.3 Session options

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Alert actions | `activity-action`, `bell-action`, `silence-action` | Same; GUI tab indicators *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Paste detection by timing | `assume-paste-time` | *Rationale:* the GUI knows when the user pastes; no timing heuristic needed | unsupported | — | — |
| First window index | `base-index` | Same; windows are numbered from 0 (tmux's default) since Step 21, the option later *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Shell and command for new panes | `default-shell`, `default-command` | Profile shell and command (SUBAK-FR-011). *Partial:* the default is `%ComSpec%`; any program with its arguments via `subak new -- <program> [args…]`, resolved on the pane's `PATH` (cmd, `powershell`, `pwsh` if installed) | partial | 11 | `crates/subak/tests/pty_shells.rs::powershell_and_cmd_run_interactively_in_independent_trees_under_the_host` (2026-11-04) |
| Size of detached sessions | `default-size` | Same *Partial:* `subak new --size <cols>x<rows>` (default 80x24); no option. *Step 26 validation:* | partial | 10 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Destroy a session with no clients | `destroy-unattached` | Opt-in option, off by default *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Behaviour when the session is destroyed | `detach-on-destroy` | What the client shows next (GUI: close the tab or switch) *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Message display time | `display-time` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Focus follows mouse | `focus-follows-mouse` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Scrollback length | `history-limit` | Same. *Partial:* per Terminal at creation, `subak new --history-limit <rows>` (0–100 000, default 2 000, plus a 4 MiB cap per pane); changing it later with an option command later (Step 25) | partial | 13 | `crates/subak/tests/scrollback_replay.rs::history_is_bounded_evicts_the_oldest_lines_and_clears_on_request` (2026-11-04) |
| Repeat timing | `repeat-time`, `initial-repeat-time` | Same *Partial:* `terminal.repeat_time_ms` in `config.toml` (0–2000, default 500); no `initial-repeat-time`. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::repeatable_prefix_keys_work_again_without_the_prefix_for_the_repeat_time` (2026-11-11) |
| Default key table | `key-table` | Same *Rationale for the move:* with custom key tables (Step 25). *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Automatic locking | `lock-after-time`, `lock-command` | *Rationale:* as `lock-client` | unsupported | — | — |
| Message and prompt look | `message-style`, `message-command-style`, `message-format`, `message-line` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Mouse support | `mouse` | Same; the GUI always receives the mouse, the option controls tmux-style handling *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Prefix keys | `prefix`, `prefix2` | Same; default `Ctrl+B` *Partial:* the `prefix` leader in `hotkeys.json` (`"leaders": {"prefix": "Ctrl+A"}`), in the window and the console client; no `prefix2`. | partial | 23 | `crates/subak/src/gui/keys.rs::tests::the_leader_follows_hotkeys_json_and_profiles_overlay_it` (2026-11-11), `crates/subak/tests/subak_cli.rs::attach_detach_keys_follow_the_users_hotkeys_json` (2026-11-11) |
| Prompt cursor | `prompt-cursor-colour`, `prompt-cursor-style`, `prompt-command-cursor-colour`, `prompt-command-cursor-style` | Same *Rationale for the move:* prompt options of Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Renumber windows on close | `renumber-windows` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Outer window title | `set-titles`, `set-titles-string` | GUI window/tab title *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Status line | `status`, `status-bg`, `status-fg`, `status-format`, `status-interval`, `status-justify`, `status-left`, `status-left-length`, `status-left-style`, `status-position`, `status-right`, `status-right-length`, `status-right-style`, `status-style` | In-tab status line (§11) *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Prompt key style | `status-keys` | vi or emacs editing in the prompt *Rationale for the move:* prompt options of Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Environment refreshed on attach | `update-environment` | Same (§15) *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Visual alerts | `visual-activity`, `visual-bell`, `visual-silence` | GUI message and tab indicator *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Word separators | `word-separators` | Same `terminal.word_separators` (Step 20) for double-click, mark mode and copy mode. | implemented | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |

### 9.4 Window options

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Size to the largest active client | `aggressive-resize` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Automatic window names | `automatic-rename`, `automatic-rename-format` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Clock appearance | `clock-mode-colour`, `clock-mode-style` | *Rationale:* `clock-mode` deferred | deferred | post-26 | — |
| Copy-mode styles | `copy-mode-current-line-number-style`, `copy-mode-current-line-style`, `copy-mode-current-match-style`, `copy-mode-line-number-style`, `copy-mode-mark-style`, `copy-mode-match-style`, `copy-mode-position-style`, `copy-mode-selection-style`, `mode-style` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Copy-mode line numbers | `copy-mode-line-numbers` | Same *Rationale for the move:* copy-mode options of Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Pane-number overlay | `display-panes-active-colour`, `display-panes-border-style`, `display-panes-colour`, `display-panes-format`, `display-panes-time` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Fill character outside a smaller window | `fill-character` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Preset layout sizes | `main-pane-height`, `main-pane-width`, `other-pane-height`, `other-pane-width` | Same *Partial:* fixed at tmux's defaults (`main-pane-width` 80, `main-pane-height` 24); the options are not settable. | partial | 25 | `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Tiled layout columns | `tiled-layout-max-columns` | Same (Step 22 tiles with as many columns as the square root of the count, rounded up) *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Menu appearance | `menu-border-lines`, `menu-border-style`, `menu-selected-style`, `menu-style` | *Rationale:* `display-menu` deferred | deferred | post-26 | — |
| Popup appearance | `popup-border-lines`, `popup-border-style`, `popup-style` | *Rationale:* `display-popup` deferred | deferred | post-26 | — |
| Copy-mode key style | `mode-keys` | Same *Rationale for the move:* the emacs table is Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Alert monitoring | `monitor-activity`, `monitor-bell`, `monitor-silence` | Same *Partial:* `monitor-activity` (off) and `monitor-bell` (on), as in tmux; no `monitor-silence`. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11), `crates/subak-term/src/screen.rs::tests::bells_count_controls_not_osc_terminators` (2026-11-11) |
| First pane index | `pane-base-index` | Same; panes are numbered from 0 (tmux's default) since Step 21, the option later *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Active-pane border indicators | `pane-border-indicators` | Same. *Partial:* the active pane's border is drawn in the accent colour (fixed; the option later) | partial | 22 | `crates/subak/tests/gui_window.rs::a_split_window_draws_every_pane_and_its_keys_and_mouse_split_focus_resize_and_zoom` (2026-11-11) |
| Pane scrollbars | `pane-scrollbars`, `pane-scrollbars-position`, `pane-scrollbars-timeout` | Native GUI scrollbar *Rationale:* no GUI scrollbar yet. *Step 26 validation:* follow-up F2 of #97. | deferred | post-26 | — |
| Per-object status styles and pane lists (new in 3.8) | `pane-status-style`, `pane-status-current-style`, `session-status-style`, `session-status-current-style`, `window-pane-status-format`, `window-pane-current-status-format` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Tree-mode appearance | `tree-mode-border-style`, `tree-mode-preview-style`, `tree-mode-selection-style` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Window size policy | `window-size` (`largest`, `smallest`, `manual`, `latest`) | Fixed `latest` first (ADR 0008); the other policies with the option *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Window list entries | `window-status-format`, `window-status-current-format`, `window-status-separator`, `window-status-style`, `window-status-current-style`, `window-status-last-style`, `window-status-activity-style`, `window-status-bell-style` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Search wrap-around | `wrap-search` | Same *Rationale for the move:* copy-mode options of Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| xterm-style modified keys | `xterm-keys` | Same *Rationale for the move:* a terminal option of Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |

### 9.5 Pane options (window or pane scope)

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Escape-sequence passthrough | `allow-passthrough` | *Rationale:* passthrough targets an outer terminal; the GUI must decide per sequence (images, notifications) | deferred | post-26 | — |
| Programs may rename the window | `allow-rename` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Programs may set the pane title | `allow-set-title` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Alternate screen | `alternate-screen` | Same *Partial:* always on (the behaviour of tmux's default); the option is not settable. *Step 26 validation:* | partial | 13 | `crates/subak-term/src/screen.rs::full_screen_program_on_the_alternate_screen` (2026-11-11) |
| Copy-mode position text | `copy-mode-position-format` | Same *Rationale for the move:* copy-mode options of Step 25. *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Cursor colour and style | `cursor-colour`, `cursor-style` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Pane borders | `pane-border-style`, `pane-active-border-style`, `pane-border-lines` | Native dividers with equivalent styles. *Partial:* one-cell gaps with a thin divider from the theme (`border_subtle`, the active pane's `accent`); styles later | partial | 22 | `crates/subak/tests/gui_window.rs::a_split_window_draws_every_pane_and_its_keys_and_mouse_split_focus_resize_and_zoom` (2026-11-11) |
| Pane status lines | `pane-border-status`, `pane-border-format` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Pane colour palette | `pane-colours` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Scrollbar style | `pane-scrollbars-style` | Same *Rationale:* no GUI scrollbar yet. *Step 26 validation:* follow-up F2 of #97. | deferred | post-26 | — |
| Keep a pane after its process exits | `remain-on-exit`, `remain-on-exit-format` | "Keep pane open on exit" (SUBAK_PHILOSOPHY §6.1) *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Scroll on clear | `scroll-on-clear` | Same *Rationale:* not verified against tmux's behaviour yet. *Step 26 validation:* follow-up F2 of #97. | deferred | post-26 | — |
| Switch-mode match style | `switch-mode-match-style` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Synchronize input | `synchronize-panes` | Same `setw synchronize-panes on|off`: interactive input and `send-keys` reach every running pane of the window (paste-buffer does not, as in tmux). | implemented | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Tree-mode preview format | `tree-mode-preview-format` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |
| Pane background/foreground | `window-style`, `window-active-style` | Same *Step 25 inventory:* follow-up F2 (the options engine (scopes, -g, user options, the other options)), #97. | deferred | post-26 | — |

## 10. Formats

### 10.1 Syntax and modifiers

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Variable expansion | `#{name}`, option names, environment names | Format engine shared by `-F`, status line and hooks *Partial:* `#{name}` and `#S #I #W #P #D #F ##` for `display-message`; unknown names expand to nothing; no option or environment names. | partial | 25 | `crates/subak/src/tmuxcmd.rs::tests::step_25_commands_tokens_and_formats` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Short aliases | `#H`, `#h`, `#D`, `#F`, `#I`, `#P`, `#S`, `#T`, `#W` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Escapes | `##`, `#,`, `#}` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Conditionals | `#{?cond,a,b}`, chained pairs | Same *Partial:* `#{?name,yes,no}` (true when not empty and not `0`); no chained pairs or comparisons. | partial | 25 | `crates/subak/src/tmuxcmd.rs::tests::step_25_commands_tokens_and_formats` (2026-11-11) |
| Comparisons | `#{==:a,b}`, `!=`, `<`, `>`, `<=`, `>=` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Boolean logic | `#{\|\|:a,b}`, `#{&&:a,b}`, `#{!:a}`, `#{!!:a}` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Pattern match | `#{m:glob,str}`, `m/r` regex, `m/i`, `m/z` fuzzy, `m/p` positions | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Search pane content | `#{C:pattern}`, `C/r`, `C/i` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Arithmetic | `#{e\|+\|:a,b}`, `-`, `*`, `/`, `m`, `%%`, comparisons, `f` float with precision | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Truncate | `#{=5:v}`, `#{=-5:v}`, `#{=/5/…:v}` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Pad | `#{p10:v}`, `#{p-10:v}` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Length and width | `#{n:v}`, `#{w:v}` | Same; width follows Unicode/CJK rules *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Repeat | `#{R:a,3}` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Time | `#{t:v}`, `t/p`, `t/r`, `t/d`, `t/f/strftime` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Client environment | `#{I/e:VAR}` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Outer-terminal interrogation | `#{I/f:feature}`, `#{I/c:capability}` | *Rationale:* no outer terminal or terminfo; GUI capabilities are fixed | unsupported | — | — |
| Basename and dirname | `#{b:v}`, `#{d:v}` | Same; Windows paths *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Quoting | `#{q:v}`, `q/h`, `q/a`, `q/s` | Same, plus quoting for the profile shell (TBD) *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Double expansion | `#{E:v}`, `#{T:v}` (with strftime) | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Loops | `#{S:…}`, `#{W:…}`, `#{P:…}`, `#{L:…}` with sort flags `/i`, `/n`, `/t`, `/r`, `/z` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Neighbour user options in window loops | `next_@name`, `prev_@name` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Option and environment loops | `#{O:…}` (`s`, `w`, `p`, `v`, `g`), `#{V:…}` (`s`, `g`, `c`) | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Name exists | `#{N:name}`, `N/w`, `N/s` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Substitution | `#{s/re/repl/:v}`, `/i`, custom delimiter | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Literal | `#{l:text}` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Animation frames | `#{A/count:frame,frame,…}` | *Rationale:* decorative motion; must first respect the system reduced-motion setting (KIM-NFR-012) | deferred | post-26 | — |
| ASCII and colour helpers | `#{a:98}`, `#{c:colour}` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Chained modifiers | `#{T;=10:status-left}` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Shell output | `#(command)` | Same, run asynchronously in the profile shell *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Inline styles | `#[fg=…,bg=…]` (§11) | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |

### 10.2 Variables

275 variables are documented in tmux 3.8. Planned as families; per-variable rows are added when
a family is scheduled in detail.

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Session variables (25) | `session_*` (e.g. `session_id`, `session_name`, `session_windows`, `session_attached`) | Terminal fields; `session_id` is the Terminal's stable ID (SUBAK-FR-010) *Partial:* `session_name`, `session_id`, `session_windows`. | partial | 25 | `crates/subak/src/tmuxcmd.rs::tests::step_25_commands_tokens_and_formats` (2026-11-11) |
| Window variables (37) | `window_*` (e.g. `window_id`, `window_index`, `window_name`, `window_flags`, `window_layout`, `window_zoomed_flag`) | Window fields *Partial:* `window_index`, `window_name`, `window_id`, `window_panes`, `window_active`, `window_flags`, `window_zoomed_flag`, `window_activity_flag`, `window_bell_flag`, `window_width`, `window_height`. | partial | 25 | `crates/subak/src/tmuxcmd.rs::tests::step_25_commands_tokens_and_formats` (2026-11-11) |
| Pane variables (62) | `pane_*` (e.g. `pane_id`, `pane_index`, `pane_title`, `pane_current_command`, `pane_current_path`, `pane_pid`, `pane_dead`, `pane_in_mode`, `pane_floating_flag`) | Pane fields; `pane_pid` is the Windows PID *Partial:* `pane_index`, `pane_id`, `pane_pid`, `pane_width`, `pane_height`, `pane_left`, `pane_top`, `pane_active`, `pane_current_command`, `pane_synchronized`, `pane_dead`. | partial | 25 | `crates/subak/src/tmuxcmd.rs::tests::step_25_commands_tokens_and_formats` (2026-11-11) |
| Client variables (25) | `client_*` (e.g. `client_name`, `client_tty`, `client_session`, `client_readonly`, `client_width`) | Client fields; `client_tty` has no Windows meaning (empty) *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Buffer variables (5) | `buffer_name`, `buffer_sample`, `buffer_size`, `buffer_created`, `buffer_full` | Same *Rationale for the move:* formats are Step 25. *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Copy, selection and search variables | `copy_*`, `selection_*`, `search_*`, `scroll_position`, `rectangle_toggle` | Same *Rationale for the move:* formats are Step 25. *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Cursor and mouse variables | `cursor_*`, `mouse_*` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| History and screen state | `history_*`, `alternate_*`, `insert_flag`, `keypad_*`, `origin_flag`, `wrap_flag`, `scroll_region_*`, `bracket_paste_flag`, `synchronized_output_flag`, `sixel_support` | Same where the renderer tracks the state *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Hook variables | `hook`, `hook_client`, `hook_pane`, `hook_session`, `hook_window`, `hook_*_name` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Key and command-list variables | `key_*`, `command`, `command_list_*` | Same, for `list-keys -F` and `list-commands -F` *Rationale for the move:* formats are Step 25. *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Option variables (customize mode) | `option_*` | *Rationale:* used by `customize-mode`, deferred | deferred | post-26 | — |
| Server and host variables | `host`, `host_short`, `pid`, `version`, `start_time`, `uid`, `user`, `config_files`, `current_file`, `server_sessions`, `next_session_id`, `line`, `loop_index`, `loop_last_flag` | Host fields; `pid` is the Host PID *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |
| Socket path | `socket_path` | *Rationale:* no tmux socket (SUBAK-SEC-010) | unsupported | — | — |
| Window neighbours | `active_window_index`, `last_window_index`, `next_window_*`, `prev_window_*` | Same *Step 25 inventory:* follow-up F3 (the format engine (modifiers, the other variables, -F)), #97. | deferred | post-26 | — |

## 11. Status line and styles

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Status line on/off and height | `status on\|off\|2…5` | Optional status line inside the Terminal tab; GUI tabs list Terminals *Partial:* the window list with flags (`0:cmd* 1:logs-!`) is in the window title when a Terminal has several windows or an alert; no drawn status line, no option. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Left and right sections | `status-left`, `status-right`, lengths and styles | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Window list | `window-status-format`, `window-status-current-format`, `window-status-separator` | Internal Window list inside the tab *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Window flags | `*` current, `-` last, `#` activity, `!` bell, `~` silence, `M` marked, `Z` zoomed | Same flags and GUI indicators *Partial:* `*`, `-`, `#`, `!`, `Z` in `#{window_flags}` and the title's window list; no `~` (silence) or `M` (marked). | partial | 25 | `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11), `crates/subak/src/tmuxcmd.rs::tests::step_25_commands_tokens_and_formats` (2026-11-11) |
| Full line formats | `status-format[]` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Refresh interval | `status-interval` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Alignment and position | `status-justify`, `status-position` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Message line | `display-message`, `message-line`, `display-time` | Same *Partial:* `display-message` in the window's banner (until a key); no `display-time` or message log. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Command prompt on the status line | `command-prompt`, `status-keys` | Prompt overlay *Partial:* the prompt is the banner row of the window. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Pane status lines | `pane-border-status`, `pane-border-format` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Colours | names, `colour0`–`colour255`, `#RRGGBB`, `default`, `terminal` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Attributes | `bold`, `dim`, `italics`, `underscore` (and double/curly/dotted/dashed), `blink`, `reverse`, `hidden`, `overline`, `strikethrough`, `noattr`, `none` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Style keywords | `fg=`, `bg=`, `us=`, `fill=`, `dim=percentage`, `width=N`, `acs` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Alignment and lists | `align=left\|centre\|right\|absolute-centre`, `list=on\|focus\|left-marker\|right-marker\|nolist` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Clickable ranges | `range=left\|right\|session\|window\|pane\|control\|user` | Clickable regions in the status line *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Style stack | `push-default`, `pop-default`, `set-default` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |
| Hyperlinks in styles | `link=uri` | Same *Step 25 inventory:* follow-up F4 (a drawn status line, styles, prompts and menus), #97. | deferred | post-26 | — |

## 12. Hooks

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| After-command hooks | `after-<command>` (e.g. `after-split-window`) | Same *Partial:* `after-new-window`, `after-split-window` (also `pane new`), `after-select-pane`, `after-select-window`; not every command. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Hooks are array options with scopes | `set-hook -a`, `-g`, `-p`, `-w`, `-u`, `hook[N]` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Run a hook now | `set-hook -R` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| User events | `set-hook -E @name` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Format subscriptions as hooks | `set-hook -B name:what:format`, `show-hooks -B` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Conditional hooks | `set-hook -T format` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Command failed | `command-error` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Window activity | `alert-activity` | Same Runs after the `window.alert` event. | implemented | 25 | `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Window bell | `alert-bell` | Same Runs after the `window.alert` event. | implemented | 25 | `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Window silence | `alert-silence` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client became latest active | `client-active` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client attached | `client-attached` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client closed | `client-closed` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client created | `client-created` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client detached | `client-detached` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client focus in | `client-focus-in` | GUI window focus *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client focus out | `client-focus-out` | GUI window focus *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client resized | `client-resized` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client switched session | `client-session-changed` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Client switched to light theme | `client-light-theme` | *Rationale:* theme options deferred (§9.2) | deferred | post-26 | — |
| Client switched to dark theme | `client-dark-theme` | *Rationale:* theme options deferred (§9.2) | deferred | post-26 | — |
| Marked pane changed | `marked-pane-changed` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane output | `pane-activity` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane bell | `pane-bell` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| OSC 133 command finished | `pane-command-finished` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| OSC 133 command started | `pane-command-started` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane created or respawned | `pane-created` | Same *Partial:* after `pane.created` (no respawn yet). | partial | 25 | `crates/subak/src/host.rs::HOOKS` |
| Pane process exited, pane kept | `pane-died` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane process exited | `pane-exited` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Pane focus in | `pane-focus-in` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane focus out | `pane-focus-out` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane mode changed | `pane-mode-changed` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane entered a mode | `pane-mode-entered` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane exited a mode | `pane-mode-exited` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane moved to another window | `pane-moved` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Prompt in a pane closed | `pane-prompt-closed` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Prompt in a pane opened | `pane-prompt-opened` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane resized | `pane-resized` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Clipboard set by OSC 52 | `pane-set-clipboard` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| OSC 133 shell prompt | `pane-shell-prompt` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Pane title changed | `pane-title-changed` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Session closed | `session-closed` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Session created | `session-created` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Session added to a group | `session-added-to-group` | *Rationale:* session groups deferred (§3) | deferred | post-26 | — |
| Session removed from a group | `session-removed-from-group` | *Rationale:* session groups deferred (§3) | deferred | post-26 | — |
| Session renamed | `session-renamed` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Session changed active window | `session-window-changed` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Window closed | `window-closed` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Window created | `window-created` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Window layout changed | `window-layout-changed` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Window linked into a session | `window-linked` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Window changed active pane | `window-pane-changed` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Window renamed | `window-renamed` | Same *Partial:* the hook name is accepted and runs after the matching Host event (ADR 0059 §5); only `after-split-window` has an end-to-end test. | partial | 25 | `crates/subak/src/host.rs::HOOKS`; `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Window resized | `window-resized` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Window unzoomed | `window-unzoomed` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Window zoomed | `window-zoomed` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |
| Window unlinked from a session | `window-unlinked` | Same *Step 25 inventory:* follow-up F5 (the other hooks and hook flags), #97. | deferred | post-26 | — |

## 13. Target syntax

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Target and source flags | `-t target`, `-s source`; client, session, window, pane target types | Same resolver for prompt, `source-file` and CLI *Partial:* `-t` and `-s` take Subak targets; tmux's client targets and relative forms are refused with a hint. | partial | 24 | `crates/subak/src/tmuxcmd.rs::tests` (2026-11-11), `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Client target | `target-client` (tty name, e.g. `/dev/ttyp1`) | Subak client ID (no tty on Windows) *Rationale for the move:* clients are not targets in Subak (Step 25 reviews `switch-client`). *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Session by ID | `$1` | Terminal ID `t-` + 12 hex (ADR 0007); the prompt also maps tmux `$n` forms *Partial:* Subak ids (`t-…`); `$1` is refused with a hint. | partial | 24 | `crates/subak/src/tmuxcmd.rs::tests` (2026-11-11) |
| Session by exact name, prefix or glob | `mysess`, `mysession`, `my*` | Same match order in the prompt; the headless CLI accepts exact names and IDs only (ADR 0007) *Partial:* exact names (case and composition ignored); no prefix or glob (never guessed). | partial | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Exact-match prefix | `=mysess` | Same *Rationale for the move:* names already match exactly. *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Default session | omitted → current, else most recently used | Same *Partial:* the context: the window's shown Terminal, `--target`, or the pane `subak command` runs in (`SUBAK_PANE_ID`). | partial | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Window target | `session:window` | `terminal:window` | implemented | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Window by index | `mysession:1` | Same | implemented | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Window by ID | `@1` | Window ID `w-` + 12 hex; tmux `@n` mapped by the prompt *Partial:* Subak ids (`w-…`); `@1` is refused. | partial | 24 | `crates/subak/src/tmuxcmd.rs::tests` (2026-11-11) |
| Window by exact name, prefix or glob | `:mywindow`, `=` exact | Same *Partial:* exact window names after the Terminal (`work:logs`). | partial | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Next free index | `session:` (empty) for `new-window`, `link-window` | Same *Rationale for the move:* refused with a hint; Step 25. *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Window tokens | `{start}` `^`, `{end}` `$`, `{last}` `!`, `{next}` `+`, `{previous}` `-`, `{current}` `@` | Same *Rationale for the move:* relative targets are refused (ADR 0058); Step 25 with the selection history. After a Terminal: `work:{last}`, `work:!`, `work:+`, `work:{end}`, `work:` (the active window). A bare token stays refused: Subak never guesses the Terminal (ADR 0059 §3). | implemented | 25 | `crates/subak/src/session.rs::tests::targets_resolve_names_ids_and_indexes_exactly` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Offsets | `+2`, `-1` (e.g. `select-window -t:+2`) | Same *Rationale for the move:* relative targets are refused (ADR 0058); Step 25. `work:+2`, `work:-1`, wrapping. | implemented | 25 | `crates/subak/src/session.rs::tests::targets_resolve_names_ids_and_indexes_exactly` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Pane target | `session:window.pane` | `terminal:window.pane` | implemented | 24 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Pane by ID | `%1` | Pane ID `p-` + 12 hex; tmux `%n` mapped by the prompt *Partial:* Subak ids (`p-…`); `%1` is refused. | partial | 24 | `crates/subak/src/tmuxcmd.rs::tests` (2026-11-11) |
| Pane by index | `.1` | Same *Rationale for the move:* a pane index needs its Terminal and window (`work:0.1`). *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Pane relative tokens | `{last}` `!`, `{next}` `+`, `{previous}` `-`, `{active}` `@` | Same *Rationale for the move:* relative targets are refused (ADR 0058); Step 25. `work:.{last}`, `work:1.!`, `work:.+`, `work:.{active}`. | implemented | 25 | `crates/subak/src/session.rs::tests::targets_resolve_names_ids_and_indexes_exactly` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Pane position tokens | `{top}`, `{bottom}`, `{left}`, `{right}`, `{top-left}`, `{top-right}`, `{bottom-left}`, `{bottom-right}` | Same *Rationale for the move:* Step 25. | implemented | 25 | `crates/subak/src/session.rs::tests::targets_resolve_names_ids_and_indexes_exactly` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Pane neighbour tokens | `{up-of}`, `{down-of}`, `{left-of}`, `{right-of}` | Same *Rationale for the move:* directions of `select-pane -L/-R/-U/-D` cover it; tokens with Step 25. | implemented | 25 | `crates/subak/src/session.rs::tests::targets_resolve_names_ids_and_indexes_exactly` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Marked pane | `{marked}` `~` | Same *Rationale for the move:* marked panes are Step 25. *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Mouse target | `{mouse}` `=` | Same *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Best-guess resolution of unqualified targets | e.g. `-t 1` as pane, window or session | Same heuristics *Rationale:* Subak never guesses a target (ADR 0058); an ambiguous name is `ambiguous_target`. | unsupported | — | `crates/subak/src/tmuxcmd.rs::tests` (2026-11-11) |
| IDs unique for the object's life | `$`, `@`, `%` IDs unique per server | Stronger: random IDs, never reused, kept across Host restarts and restore (SUBAK-FR-010, ADR 0007) *Step 26 validation:* | implemented | 10 | `crates/subak/src/session.rs::names_are_unique_ignoring_case_and_ids_are_never_reused` (2026-11-11), `crates/subak/tests/session_lifecycle.rs::names_are_unique_ignoring_case_and_composition_and_renames_keep_ids` (2026-11-11) |
| Pane ID in the pane environment | `TMUX_PANE` | `SUBAK_PANE_ID=p-…` and `SUBAK_TERMINAL_ID=t-…` in every pane (ADR 0010 §1); an override cannot change them | implemented | 11 | `crates/subak/tests/pty_shells.rs::explicit_programs_get_their_arguments_exactly_and_report_exit_codes` (2026-11-04) |

## 14. Mouse

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Enable mouse handling | `mouse` option | Same *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Mouse events as keys | `MouseDown1`–`3`, `MouseUp1`–`3`, `MouseDrag1`–`3`, `MouseDragEnd1`–`3`, `WheelUp`, `WheelDown`, `SecondClick1`–`3`, `DoubleClick1`–`3`, `TripleClick1`–`3` | Bindable mouse events *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Event locations | `Pane`, `Border`, `Status`, `StatusLeft`, `StatusRight`, `StatusDefault`, `ScrollbarSlider`, `ScrollbarUp`, `ScrollbarDown`, `Empty`, `ControlN` | Same suffixes *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Modifiers on mouse keys | `C-`, `M-`, `S-` (e.g. `M-MouseDown3Pane`) | Same *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Forward a mouse event to the program | `send-keys -M`, `mouse_any_flag` | Mouse reporting passed through ConPTY *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Mouse target token | `{mouse}`, `=` | Same (§13) *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |
| Focus follows mouse | `focus-follows-mouse` | Same *Step 25 inventory:* follow-up F7 (key tables, more prefix keys and the mouse), #97. | deferred | post-26 | — |

## 15. Environment

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Global environment copied at server start | global environment | *Differs on purpose:* every new pane gets the user's logon environment built fresh (`CreateEnvironmentBlock`), never the server's or a launcher's, so a variable set with `setx` reaches the next pane ([PTY_PROCESSES §3](PTY_PROCESSES.md)) | implemented | 11 | `crates/subak/tests/pty_shells.rs::each_new_pane_gets_a_fresh_logon_environment` (2026-11-04) (manual), `crates/subak/tests/pty_shells.rs::the_pane_environment_keeps_the_users_locations_and_secrets_never_leak` (2026-11-04) |
| Session environment | per-session environment | Per-Terminal environment from the profile (SUBAK-FR-011). *Partial:* per-pane overrides given at creation (`env` of `terminal.create`; names shown, values never); profiles with Step 33 | partial | 11 | `crates/subak/tests/pty_shells.rs::the_pane_environment_keeps_the_users_locations_and_secrets_never_leak` (2026-11-04) |
| Merge for new processes | session overrides global | Overrides (set or unset, case-insensitive names) on top of the logon environment, then the Host's `SUBAK_*` variables | implemented | 11 | `crates/subak/src/win/mod.rs::tests::overrides_replace_remove_and_add_case_insensitively` (2026-11-04) |
| Refresh from the client on attach | `update-environment` (e.g. `DISPLAY`, `SSH_AUTH_SOCK`) | Same *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Hidden variables | `set-environment -h`, `%hidden` | Hidden variables; variables marked secret are not persisted (SUBAK-SEC-011) *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Set, unset, remove, format value | `set-environment -u`, `-r`, `-F`, `-g` | Same *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Show in shell syntax | `show-environment -s` | Bourne syntax as in tmux; PowerShell syntax TBD *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| In-tmux marker variable | `TMUX` | `SUBAK_PANE_ID` / `SUBAK_TERMINAL_ID` mark a Subak pane; no socket path | implemented | 11 | `crates/subak/tests/pty_shells.rs::explicit_programs_get_their_arguments_exactly_and_report_exit_codes` (2026-11-04) |
| Terminal type in panes | `TERM` (`default-terminal`) | Same *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| vi keys from the editor | `VISUAL`, `EDITOR` | Same default for `mode-keys`/`status-keys` *Rationale for the move:* the emacs table is Step 25. *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Home directory | `HOME` | Windows user profile directory *Step 26 validation:* panes get the user's logon environment and start in the profile directory. | implemented | 10 | `crates/subak/tests/pty_shells.rs::the_pane_environment_keeps_the_users_locations_and_secrets_never_leak` (2026-11-11) |
| Locale for output | `LC_CTYPE` | Always UTF-8 (KIM-NFR-013) *Step 26 validation:* UTF-8 end to end, Korean wide glyphs included. | implemented | 12 | `crates/subak/tests/pty_shells.rs::ctrl_c_and_unicode_reach_programs_in_cmd_and_powershell` (2026-11-11), `crates/subak/tests/terminal_render.rs::cmd_renders_korean_wide_glyphs_and_follows_a_resize` (2026-11-11) |
| Locale for times | `LC_TIME` | Windows locale for time formats *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Working directory | `PWD` | Profile or invoking directory *Step 26 validation:* the profile directory by default, `--cwd` otherwise. | implemented | 10 | `crates/subak/tests/pty_shells.rs::the_pane_environment_keeps_the_users_locations_and_secrets_never_leak` (2026-11-11) |
| Default shell | `SHELL` | Profile shell; Windows has no `SHELL` *Partial:* the default profile of `config.toml` or `cmd.exe`; PowerShell and cmd tested. *Step 26 validation:* | partial | 10 | `crates/subak/tests/pty_shells.rs::powershell_and_cmd_run_interactively_in_independent_trees_under_the_host` (2026-11-11) |
| Socket directory | `TMUX_TMPDIR` | *Rationale:* no tmux socket (SUBAK-SEC-010) | unsupported | — | — |

## 16. Buffers and clipboard

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Named and automatic buffers | `buffer0001`…, explicit names via `set-buffer -n`, `load-buffer -b` | Same `bufferN` and names from `-b`. | implemented | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| Automatic buffer limit | `buffer-limit` (explicit names exempt) | Same 50; named buffers are exempt. | implemented | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| Default buffer | most recent automatic buffer | Same | implemented | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| Bracketed paste | `paste-buffer -p` | Same *Rationale for the move:* paste-buffer -p is Step 25. *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Line-ending handling on paste | `paste-buffer -r`, `-s separator` | Same; CRLF handling for Windows programs TBD *Partial:* line ends become carriage returns (tmux without `-r`); `-r`, `-s` later. | partial | 24 | `crates/subak/tests/tmux_commands.rs::paste_buffers_live_in_the_host_and_paste_as_input` (2026-11-11) |
| System clipboard | `set-clipboard`, `set-buffer -w`, `load-buffer -w`, OSC 52 | Windows clipboard *Partial:* copy mode copies into a buffer and the Windows clipboard; `set-clipboard`, `-w`, OSC 52 later. | partial | 24 | `crates/subak/tests/gui_window.rs::the_command_prompt_and_copy_mode_work_on_the_shown_terminal` (2026-11-11) |
| Clipboard read by programs | `get-clipboard` | Opt-in only (privacy) *Rationale for the move:* opt-in only, Step 25. *Step 25 inventory:* follow-up F9 (environment, buffer files and the clipboard), #97. | deferred | post-26 | — |
| Per-pane history buffer | `history-limit` | Scrollback. *Partial:* the Host keeps each pane's history in memory, bounded and evicted oldest first ([SCROLLBACK §1](SCROLLBACK.md)); copying from it into buffers with copy mode later (Step 24) | partial | 13 | `crates/subak/tests/scrollback_replay.rs::reattach_after_shell_output_and_scrolling_reproduces_history_and_screen` (2026-11-04) |

## 17. Configuration file and parsing syntax

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Load a user config at server start | `~/.tmux.conf`, `$XDG_CONFIG_HOME/tmux/tmux.conf`, `~/.config/tmux/tmux.conf` | *Rationale:* Subak's own config (SUBAK-FR-046) is primary; loading a tmux file automatically needs an ADR (opt-in import is the likely form) | deferred | post-26 | — |
| System-wide config | `@SYSCONFDIR@/tmux.conf` | *Rationale:* no system-wide tmux file on Windows; per-user installs (KIM-SEC-004) | unsupported | — | — |
| Load a tmux-syntax file on demand | `source-file path`, glob patterns | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Parse only, quiet, verbose, format path | `source-file -n`, `-q`, `-v`, `-F` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Reload binding | `bind R source-file …` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Commands end at newline or `;` | `neww ; splitw` | Same *Partial:* `;` separates commands (Step 24); a newline inside one command line is not a separator yet. | partial | 25 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Comments | `#` to end of line | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Line continuation | trailing `\` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Quoting | `'…'`, `"…"` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Command lists in braces | `{ … }` (nestable, multi-line) | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Replacements | `$VAR`, `~`, `~user`, `\uXXXX`, `\e`, `\r`, `\n`, `\t`, `\\` | Same; `~` is the Windows profile directory *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Conditional parsing | `%if`, `%elif`, `%else`, `%endif` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Environment assignment | `NAME=value`, `%hidden NAME=value` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Error reporting | config errors, `show-messages`, `%config-error` | Clear errors; invalid config falls back to defaults (SUBAK-FR-046) *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |

## 18. Command chaining and scripting

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Command sequences | `cmd1 ; cmd2`; stop at the first error | Same Since Step 24: a sequence stops at the first error and says how many ran. | implemented | 25 | `crates/subak/tests/tmux_commands.rs::command_lines_split_move_send_and_list_with_tmux_semantics` (2026-11-11) |
| Separator escaping from a shell | `\;`, `';'` | Same; PowerShell and cmd need their own quoting (`';'`) *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Command queue semantics | `if-shell`/`confirm-before` insert commands after themselves | Same ordering *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Conditional on a shell command | `if-shell 'cmd' { … } { … }`, `-b` | Same; condition runs in the profile shell *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Conditional on a format | `if-shell -F '#{…}'` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Run a shell command | `run-shell`, `-b`, `-d`, `-c`, `-t` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Run a tmux command after a delay | `run-shell -C`, `run -d 0.3` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Synchronize scripts | `wait-for`, `-S`, `-L`, `-U` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Command aliases | `command-alias[N] name=value` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Format arguments | `-F` on `display-message`, `list-*`, `new-*`, `set-option` | Same *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Placeholders in prompts | `%%`, `%1`…`%9`, `%%%` | Same *Rationale for the move:* prompt templates with Step 25. *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |
| Shell for commands | `/bin/sh -c` (or `default-shell`) | Profile shell (PowerShell, cmd, Git Bash, WSL); quoting differs. *Differs:* `subak new` runs the program with its arguments directly, never through a shell; scripts need their interpreter named (Step 11, [PTY_PROCESSES §2](PTY_PROCESSES.md)) *Step 25 inventory:* follow-up F8 (configuration files and scripting (source-file, if-shell, run-shell, wait-for)), #97. | deferred | post-26 | — |

## 19. Control mode

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Text protocol client | `tmux -C`, `tmux -CC` | *Rationale:* deferred; Subak's versioned IPC (KIM-NFR-014), `--json` CLI and the planned MCP server (SUBAK-FR-062) serve integrators first | deferred | post-26 | — |
| Output blocks | `%begin`, `%end`, `%error` | *Rationale:* as above | deferred | post-26 | — |
| Output notifications | `%output`, `%extended-output`, `%pause`, `%continue` | *Rationale:* as above | deferred | post-26 | — |
| Structure notifications | `%layout-change`, `%window-add`, `%window-close`, `%window-renamed`, `%window-pane-changed`, `%unlinked-window-add`, `%unlinked-window-close`, `%unlinked-window-renamed`, `%pane-mode-changed` | *Rationale:* as above | deferred | post-26 | — |
| Session and client notifications | `%sessions-changed`, `%session-changed`, `%session-renamed`, `%session-window-changed`, `%client-session-changed`, `%client-detached`, `%exit` | *Rationale:* as above | deferred | post-26 | — |
| Other notifications | `%message`, `%config-error`, `%paste-buffer-changed`, `%paste-buffer-deleted`, `%subscription-changed` | *Rationale:* as above | deferred | post-26 | — |
| Control client size and flow control | `refresh-client -C WxH`, `-A pane:on\|off\|pause\|continue`, `-f no-output,pause-after=N,wait-exit` | *Rationale:* as above | deferred | post-26 | — |
| Format subscriptions | `refresh-client -B name:what:format` | *Rationale:* as above | deferred | post-26 | — |

## 20. Multiple clients and access control

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Several clients on one session | multiple `attach-session` | GUI, CLI and agents attached together (SUBAK-FR-004). *Partial:* several CLI and IPC clients attach to one Terminal at once; the GUI later | partial | 12 | `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04) |
| List clients | `list-clients` | `subak host status`. *Partial:* `subak status` / `host status` with client ids | partial | 12 | `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04) |
| Window size with several clients | `window-size`, `aggressive-resize` | Same policies *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Client does not affect size | `ignore-size` client flag | Same *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Read-only clients | `attach-session -r`, `read-only` flag, `switch-client -r` | Same; input ignored except detach/switch. *Partial:* `subak attach --read-only` (IPC `interactive: false`); the flag on a running client and `switch-client -r` later | partial | 12, 25 | `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04) |
| Per-client active pane | `active-pane` client flag | *Rationale:* complicates the shared object model; revisit after Step 25 | deferred | post-26 | — |
| Keep client when its session is destroyed | `no-detach-on-destroy` client flag | Same *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Switch sessions | `switch-client -t`, `-l`, `-n`, `-p`, `-Z` | Same *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Choose a client | `choose-client` | Same *Step 25 inventory:* follow-up F6 (clients, sessions and targets (switch-client, kill-server, -f)), #97. | deferred | post-26 | — |
| Session groups | `new-session -t` | *Rationale:* see §3 | deferred | post-26 | — |
| Multi-user access | `server-access -a`, `-d`, `-r`, `-w`, `-g`, `-l` | *Rationale:* the Host is per-user; IPC is same-user only (SUBAK-SEC-010, SUBAK-SEC-012) | unsupported | — | — |
| Socket file permissions | socket mode bits | *Rationale:* no tmux socket; access control is the IPC's same-user restriction | unsupported | — | — |
| Locking | `lock-client`, `lock-session`, `lock-server` | *Rationale:* see §2.1 | unsupported | — | — |
| Suspending | `suspend-client` | *Rationale:* see §2.1 | unsupported | — | — |

## 21. Layouts, names, alerts and pane lifecycle

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Even-horizontal layout | `even-horizontal` | Same: `subak window layout <window> even-horizontal` / `window.select_layout` | implemented | 22 | `crates/subak/tests/pane_layout.rs::rotate_and_basic_layouts_rearrange_without_ending_anything` (2026-11-11) |
| Even-vertical layout | `even-vertical` | Same: `subak window layout <window> even-vertical` / `window.select_layout` | implemented | 22 | `crates/subak/tests/pane_layout.rs::rotate_and_basic_layouts_rearrange_without_ending_anything` (2026-11-11) |
| Tiled layout | `tiled` | Same: `subak window layout <window> tiled` / `window.select_layout` | implemented | 22 | `crates/subak/tests/pane_layout.rs::rotate_and_basic_layouts_rearrange_without_ending_anything` (2026-11-11) |
| Main-horizontal layout | `main-horizontal` | Same | implemented | 25 | `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Main-horizontal-mirrored layout | `main-horizontal-mirrored` | Same | implemented | 25 | `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Main-vertical layout | `main-vertical` | Same | implemented | 25 | `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Main-vertical-mirrored layout | `main-vertical-mirrored` | Same | implemented | 25 | `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11), `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Spread panes evenly | `select-layout -E` | Same `select-layout -E`, `subak window layout <w> spread`. | implemented | 25 | `crates/subak/src/layout.rs::tests::main_presets_cycle_and_spread` (2026-11-11) |
| Undo the last layout change | `select-layout -o` | Same *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Layout strings | `window_layout` format, `select-layout <string>` | Same string format, so layouts can be copied from tmux *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Zoom a pane | `resize-pane -Z`, `Z` flag | Same: `subak pane zoom`, `pane.zoom`, `Ctrl+Shift+Z` in the window; the zoomed pane fills the window, the others keep their sizes unseen; `zoomed` in window objects | implemented | 22 | `crates/subak/tests/pane_layout.rs::splits_resize_zoom_swap_and_close_keep_processes_and_real_pty_sizes` (2026-11-11), `crates/subak/tests/gui_window.rs::a_split_window_draws_every_pane_and_its_keys_and_mouse_split_focus_resize_and_zoom` (2026-11-11) |
| Session names | `new-session -s`, `rename-session` | Terminal names (SUBAK-FR-010) *Step 26 validation:* | implemented | 10 | `crates/subak/tests/session_lifecycle.rs::names_are_unique_ignoring_case_and_composition_and_renames_keep_ids` (2026-11-11), `crates/subak/tests/session_lifecycle.rs::concurrent_clients_create_and_rename_without_duplicates_or_orphans` (2026-11-11) |
| Window names | `new-window -n`, `rename-window`, `automatic-rename`, `\033k…\033\\` | Same. *Partial:* `subak window new --name`, `subak window rename`; a window is named after its first program until renamed (`automatic_name`); following the foreground program and `\033k…\033\\` later | partial | 21 | `crates/subak/tests/windows_panes.rs::a_terminal_holds_windows_with_independent_pane_processes` (2026-11-04) |
| Pane titles | `select-pane -T`, OSC 2, `allow-set-title` | Same *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Marked pane | `select-pane -m`, `-M`, `{marked}` | Same *Rationale for the move:* marked panes are Step 25. *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Disable/enable pane input | `select-pane -d`, `-e` | Same *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Synchronized input | `synchronize-panes` | Same | implemented | 25 | `crates/subak/tests/tmux_advanced.rs::layouts_sync_tokens_and_formats_act_on_real_panes` (2026-11-11) |
| Linked windows | `link-window`, `unlink-window` | Same *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Activity, bell and silence alerts | `monitor-*`, `visual-*`, `*-action`, window flags | Alerts on the in-tab window list and the GUI tab *Partial:* activity and bell flag a window that is not shown, once until it is shown, with a `window.alert` event and the title's window list; no silence, `visual-*` or `*-action` options. | partial | 25 | `crates/subak/tests/tmux_advanced.rs::alerts_and_hooks_follow_host_events` (2026-11-11) |
| Dead panes | `remain-on-exit`, `pane_dead`, `respawn-pane` | Same *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Floating and modal panes (new in 3.8) | `new-pane`, `move-pane -P`, `-O` modal, z-order | *Rationale:* see `new-pane` (§2.2) | deferred | post-26 | — |
| Shell integration marks | OSC 133 (`next-prompt`, `pane-shell-prompt`) | Same *Rationale for the move:* shell integration is Step 25. *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |
| Hyperlinks | OSC 8, `clear-history -H` | Clickable links in the GUI *Step 25 inventory:* follow-up F10 (more window and pane operations and layouts (respawn, move-window, layout strings)), #97. | deferred | post-26 | — |

## 22. Terminal integration and exit messages

| tmux feature | tmux reference | Subak equivalent (planned) | Status | Target Step | Evidence |
| --- | --- | --- | --- | --- | --- |
| Outer-terminal terminfo extensions | `AX`, `Bidi`, `Cs`/`Cr`, `Cmg`/`Clmg`/`Dsmg`/`Enmg`, `Dsbp`/`Enbp`, `Dseks`/`Eneks`, `Dsesc`/`Enesc`, `Dsfcs`/`Enfcs`, `Hls`, `Nobr`, `Rect`, `Smol`, `Smulx`, `Setulc`/`Setulc1`/`ol`, `Ss`/`Se`, `Spb`, `Swd`, `Sxl`, `Sync`, `Tc`, `Ms`, `XT` | *Rationale:* describe an outer terminal; the GUI renders natively. Revisit for a console client | deferred | post-26 | — |
| Pane terminal emulation | VT/xterm sequences inside panes | ConPTY output rendered by the GUI (SUBAK-FR-016). *Partial:* the Host models every pane's terminal (xterm subset: screens, colours, modes, regions) and reproduces it on attach ([SCROLLBACK §2](SCROLLBACK.md)); the GUI renderer later (Steps 15–16) | partial | 13 | `crates/subak/tests/scrollback_replay.rs::reattach_during_and_after_a_full_screen_program` (2026-11-04) |
| Client exit messages | `[detached (from session …)]`, `[detached and SIGHUP]`, `[lost tty]`, `[terminated]`, `[too far behind]`, `[exited]`, `[server exited]`, `[server exited unexpectedly]` | Equivalent CLI messages and exit codes; no SIGHUP or tty variants. *Partial:* `[subak: detached …]`, `[… detached from `<name>` by another client …]`, `[… the program … ended (exit code n) …]`, `[… the Host stopped (reason)]` (exit 0) and `host_disconnected` (exit 6); `[too far behind]` does not end a client since Step 13: it is resynced (`pane.resync`, a fresh snapshot) | partial | 12, 13 | `crates/subak/tests/subak_cli.rs::a_user_creates_attaches_detaches_lists_renames_and_kills_with_the_cli_only` (2026-11-04) |

## 23. Subak-specific differences

- **GUI tabs are Terminals, not tmux windows.** One GUI tab = one Terminal = one tmux session.
  `Ctrl+B c` creates an internal Window *inside* the current tab; internal Windows are listed in
  the tab (status line, Step 25), never as extra GUI tabs.
- **GUI-level shortcuts sit in front of the prefix table.** `Ctrl+T` (new Terminal),
  `Ctrl+W` (close = delete the Terminal, confirmed when busy) and `Ctrl+Tab` /
  `Ctrl+Shift+Tab` (next/previous tab) are Subak GUI shortcuts with no tmux binding
  (SUBAK-FR-033). Their nearest tmux analogues are `new-session`, `kill-session` and
  `switch-client -n`/`-p`. All are rebindable (SUBAK-FR-034).
- **Shadowed keys pass through via the prefix.** `Ctrl+B Ctrl+W`, `Ctrl+B Ctrl+T` (and
  `Ctrl+B Ctrl+Tab`) send the literal shadowed key to the program (readline, Vim), as decided in
  ADR 0009 (SUBAK-FR-034). tmux leaves these prefix keys unbound, so no tmux binding is lost.
  `Ctrl+B Ctrl+B` sends a literal `Ctrl+B`, exactly like tmux `send-prefix`.
- **The Host is not a tmux server.** There is no socket or protocol compatibility: no `-L`/`-S`,
  no `TMUX_TMPDIR`, no socket path in the marker variable, and the real `tmux` binary cannot
  attach to a Subak Host (or the reverse). tmux muscle memory carries over through the key
  tables; tmux scripts carry over through tmux-syntax commands (prompt, `source-file`, CLI form
  TBD).
- **Persistence goes further than tmux.** tmux sessions die with the server. The Subak Host
  persists structure and offers restore after a reboot, with new processes marked `restored`
  (SUBAK-FR-040…043). tmux has no built-in equivalent.
- **IDs are stronger.** tmux `$n`/`@n`/`%n` are unique for one server's life; Subak IDs are
  stable and never reused (SUBAK-FR-010).
- **Windows, not POSIX.** No SIGHUP, SIGTSTP or tty names; panes run under ConPTY with
  Job Objects (SUBAK-FR-016). Shell commands (`if-shell`, `run-shell`, `#()`, `copy-pipe`) run in
  the profile shell, so quoting differs from `/bin/sh`.
- **Closing vs killing is the same as tmux for panes and windows** (`Ctrl+B x`, `Ctrl+B &` kill
  processes), but closing the GUI is always a detach (SUBAK-FR-015).
- **Killing the last window or pane never ends the Terminal** (Step 21, ADR 0054). tmux's
  `kill-window` or `kill-pane` of a session's last one ends the session; Subak refuses it
  (`invalid_state`, `last_window`/`last_pane`) because only `subak kill` (or the GUI's guarded
  `Ctrl+W`) ends a Terminal. Kills from the CLI always ask (or need `--yes`) and act on the
  confirmed id; the Host refuses a busy pane without `force`.
- **The selection is shared, per Terminal** (Step 21), as tmux's current window is per session;
  every client sees the same active window and pane and the GUI follows it. A `subak attach` stays
  on the pane it attached to (tmux would switch the client).
- **Subak-only features** such as the Claude indicator (SUBAK-FR-050) have no tmux row.

## 24. Unreleased changes on tmux master (not counted)

Seen at `master` commit `a73616d781f2` (`tmux.1` revision 1.1195, 2026-10-29) and **not** in 3.8.
They get rows only once released:

- `display-popup` (`popup`) and the `popup-style`, `popup-border-style`, `popup-border-lines`
  options are removed from the manual page (floating panes take over that role).
- New options: `image-size-limit`, `image-total-count-limit`, `image-total-size-limit`, `utmp`,
  `side-status`, `side-status-format`, `side-status-style`, `side-status-width`,
  `copy-mode-keep-column`, `window-default-command`; a new `nl` list entry.
- The list of layout names is restructured in the manual page.

## 25. Related decisions and open inputs

Rows follow the Step 03 decision records in `docs/adr/`: ADR 0007 (IDs, names, targets),
ADR 0008 (attach, replay, size arbitration) and ADR 0009 (GUI keyboard dispatch and
pass-through). Still open: the CLI contract (Step 07) and the Kim design system (Step 04).
Rows that say "TBD" are updated when those land.

## 26. How evidence is recorded

- **Format.** An Evidence cell lists one or more automated tests and the date they first
  passed, e.g. `e2e/tests/step21_windows.rs::new_window_creates_internal_window (2027-01-15)` or
  a unit/integration test path plus test name. Dates are ISO 8601.
- **`implemented` needs automated evidence** that exercises the tmux-visible behaviour of the
  whole row (command, flags named in the row, key, or option effect). A manual check alone is
  never enough; it can support `partial` at most, and must say *manual* with steps.
- **`partial`** names what is covered in the Subak column (e.g. "`-h`, `-v` only").
- **Traceability.** Tests cite SUBAK-FR-060 and the tmux reference of the row in their doc
  comment (see the e2e conventions in [e2e/LEDGER.md](../e2e/LEDGER.md)).
- **Same-change updates.** Changing a Status updates Evidence and §27 in the same commit.
- **Reference upgrades.** Moving to a newer tmux release bumps the matrix version, adds new rows
  as `planned` (or `deferred`/`unsupported` with a rationale), and marks rows for removed tmux
  features. Step 26 re-validates every row and publishes the compatibility statement.
- **Checked by the build (Step 26).** `cargo xtask docs-check` (part of `cargo xtask ci`) fails
  when a row has a wrong shape, an unknown Status or Target, an `implemented`/`partial` row cites
  no evidence or evidence that does not exist (a file, or a `fn`/`const` it names), a
  `deferred`/`unsupported` row has no rationale, a `planned` row targets a finished Step, or §27
  does not count the rows (`xtask/src/tmux.rs`).
- **The journey suite.** `crates/subak/tests/tmux_journeys.rs` runs one long-running program
  through nested splits, resizes, zoom, swaps, every layout, break/join, renames, synchronized
  input, a hook, two watching clients and detaching all of them; `tmux_advanced.rs`,
  `tmux_commands.rs`, `pane_layout.rs`, `windows_panes.rs` and the `gui_window.rs` prompt,
  copy-mode and prefix tests cover the rest; the e2e scenarios `step21`–`step26` run the real
  binaries daily.

## 27. Summary counts

Computed from every matrix row in §1–§22 (matrix version 1); §24 is not counted. Recount
whenever a row or Status changes.

| Status | Rows |
| --- | --- |
| `planned` | 0 |
| `partial` | 140 |
| `implemented` | 88 |
| `deferred` | 352 |
| `unsupported` | 26 |
| **Total** | **606** |

| Section | planned | partial | implemented | deferred | unsupported | Total |
| --- | --- | --- | --- | --- | --- | --- |
| §1 Invocation | 0 | 3 | 5 | 4 | 5 | 17 |
| §2 Commands | 0 | 45 | 8 | 34 | 5 | 92 |
| §3 Notable command flags | 0 | 9 | 1 | 9 | 1 | 20 |
| §4 Prefix table | 0 | 7 | 35 | 29 | 1 | 72 |
| §5 Root table (mouse) | 0 | 6 | 1 | 12 | 0 | 19 |
| §6 Key tables overview | 0 | 4 | 1 | 3 | 0 | 8 |
| §7 Copy mode | 0 | 10 | 1 | 18 | 0 | 29 |
| §8 Other pane modes | 0 | 0 | 0 | 7 | 0 | 7 |
| §9 Options | 0 | 15 | 3 | 72 | 5 | 95 |
| §10 Formats | 0 | 5 | 0 | 37 | 2 | 44 |
| §11 Status line and styles | 0 | 4 | 0 | 13 | 0 | 17 |
| §12 Hooks | 0 | 12 | 2 | 43 | 0 | 57 |
| §13 Target syntax | 0 | 7 | 10 | 6 | 1 | 24 |
| §14 Mouse | 0 | 0 | 0 | 7 | 0 | 7 |
| §15 Environment | 0 | 2 | 6 | 7 | 1 | 16 |
| §16 Buffers and clipboard | 0 | 3 | 3 | 2 | 0 | 8 |
| §17 Configuration file and parsing | 0 | 1 | 0 | 12 | 1 | 14 |
| §18 Command chaining and scripting | 0 | 0 | 1 | 11 | 0 | 12 |
| §19 Control mode | 0 | 0 | 0 | 8 | 0 | 8 |
| §20 Multiple clients and access control | 0 | 3 | 0 | 7 | 4 | 14 |
| §21 Layouts, names, alerts, pane lifecycle | 0 | 2 | 11 | 10 | 0 | 23 |
| §22 Terminal integration and exit messages | 0 | 2 | 0 | 1 | 0 | 3 |
| **Total** | **0** | **140** | **88** | **352** | **26** | **606** |

Coverage checks for matrix version 1: all 92 commands and their aliases, all 184 options in
`options-table.c`, all 51 named hooks and all 99 copy-mode commands of tmux 3.8 appear in the
tables above.
