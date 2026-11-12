//! Step 76 (DDALGAK-FR-040…042, ADR 0077): the test playground's simulation — what one input
//! event would do, as a `ddalgak-simulation-v1` trace (docs/DDALGAK_RULE_MODEL.md §11).
//!
//! A pure function of a rules document (saved, or an unsaved draft), a context the user chose
//! (profile, mode, engine state, focus kind, app, IME), the event, and — for the pointer
//! previews — the playground's simulated desktop and pointer. **Nothing is injected, opened or
//! changed** by this module: `simulate` only describes; in `live` the playground itself types the
//! effects into its own test field and opens a program or website only for an effect the user
//! confirmed (`confirm`), which this trace then reports as performed.
//!
//! Selection follows §7.1: stepping aside (secure desktop; an elevated window, a password field or
//! an IME composition for rules that inject), paused (only engine-control rules), candidates
//! (trigger, modes with their parent chain, `when`), precedence (nearest mode, `when.apps`,
//! `when.window_title`, `priority`, file order). Personal content is summarized, never echoed:
//! typed text as counts, URLs as scheme and host, programs as file names.

use crate::model::{
    Action, Document, EngineOp, IfRunning, InjectedPolicy, ModeOp, Profile, Rule, TriggerOn,
};
use kim_hotkeys::keys::Sequence;
use serde_json::{Value, json};

/// What the playground simulates the pointer on: one 1920×1080 monitor at the origin.
pub const DESKTOP: (i32, i32, i32, i32) = (0, 0, 1920, 1080);

/// `simulate` (the default) or `live`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Simulate,
    Live,
}

/// The context and event of one test.
#[derive(Debug, Clone)]
pub struct Input {
    pub kind: Kind,
    /// The profile to test (`None`: the document's active profile).
    pub profile: Option<String>,
    /// `None` for an unsaved draft.
    pub revision: Option<u64>,
    pub mode: Option<String>,
    pub paused: bool,
    /// `normal`, `password_field`, `elevated_window` or `secure_desktop`.
    pub focus: String,
    pub app_exe: Option<String>,
    pub window_title: Option<String>,
    pub ime_composing: bool,
    pub keys: String,
    /// `press`, `release`, `hold` or `repeat`.
    pub on: String,
    pub injected: bool,
    /// The playground pointer, in the simulated desktop.
    pub pointer: (i32, i32),
    /// Live: the step numbers whose launch the user confirmed.
    pub confirmed: Vec<usize>,
}

impl Default for Input {
    fn default() -> Input {
        Input {
            kind: Kind::Simulate,
            profile: None,
            revision: None,
            mode: None,
            paused: false,
            focus: "normal".into(),
            app_exe: None,
            window_title: None,
            ime_composing: false,
            keys: String::new(),
            on: "press".into(),
            injected: false,
            pointer: (DESKTOP.2 / 2, DESKTOP.3 / 2),
            confirmed: Vec::new(),
        }
    }
}

/// The canonical text of a trigger (Ddalgak's syntax: lock keys, bare side modifiers, mouse
/// buttons) or, for output keys a trigger may not use (`Win+D`), of the hotkey contract.
fn canonical(keys: &str) -> Option<String> {
    crate::trigger::Trigger::parse(keys)
        .map(|t| t.to_string())
        .ok()
        .or_else(|| Sequence::parse_input(keys).ok().map(|s| s.to_string()))
}

/// The mode, then its parent, then the parent's parent… (Step 72 layers).
fn chain(p: &Profile, mode: &str) -> Vec<String> {
    let mut out = vec![mode.to_string()];
    let mut cur = mode.to_string();
    while let Some(parent) = p
        .modes
        .iter()
        .find(|m| m.id == cur)
        .and_then(|m| m.parent.clone())
    {
        if out.contains(&parent) {
            break;
        }
        out.push(parent.clone());
        cur = parent;
    }
    out
}

/// Whether a step injects input (stepping aside applies to it).
fn injects(a: &Action) -> bool {
    matches!(
        a,
        Action::Remap(_)
            | Action::SendKeys(_)
            | Action::TypeText(_)
            | Action::MouseMove(_)
            | Action::MouseScroll(_)
            | Action::MouseClick(_)
    )
}

/// The rule's compiled `when` (`Ok(None)` without one), exactly as the engine compiles it.
fn cond_of(r: &Rule) -> Result<Option<crate::context::Cond>, String> {
    r.when
        .as_ref()
        .map(|w| crate::context::Cond::compile(&serde_json::to_value(w).unwrap_or_default()))
        .transpose()
}

/// Whether `when` holds in this context: the engine's matcher against the chosen app (an exe
/// name or a full path) and title (Step 77).
fn condition_holds(r: &Rule, i: &Input) -> bool {
    let fg = crate::context::FixedForeground {
        app: i.app_exe.clone(),
        title: i.window_title.clone(),
    };
    match cond_of(r) {
        Ok(None) => true,
        Ok(Some(c)) => c.holds(&fg, &mut crate::context::Probe::default()),
        Err(_) => false,
    }
}

/// Whether the trigger matches the event's keys and kind.
fn trigger_matches(r: &Rule, keys: &str, on: &str) -> bool {
    let Some(k) = r.trigger.keys.as_deref().and_then(canonical) else {
        return false;
    };
    if k != keys {
        return false;
    }
    match on {
        "repeat" => r.trigger.on() == TriggerOn::Press && r.trigger.repeat == Some(true),
        "press" => r.trigger.on() == TriggerOn::Press,
        "release" => r.trigger.on() == TriggerOn::Release,
        "hold" => r.trigger.on() == TriggerOn::Hold,
        _ => false,
    }
}

/// One step's effect: (type, summary, preview, injected events if performed live).
fn effect(
    a: &Action,
    trigger: &str,
    i: &Input,
    mode: &str,
    profile: &Profile,
) -> (String, String, Value, u64) {
    match a {
        Action::Remap(r) => {
            let p = crate::remap::preview(trigger, &r.to, r.tap.as_deref(), false);
            let holds = p["holds"].as_str().unwrap_or("?").to_string();
            let s = match p["tap"].as_str() {
                Some(t) => {
                    format!("{holds} is held while the key is held; a quick tap alone sends {t}.")
                }
                None => format!("The key acts as {holds}."),
            };
            ("remap".into(), s, p, 2)
        }
        Action::Block(_) => ("block".into(), "The key does nothing.".into(), json!({}), 0),
        Action::SendKeys(s) => {
            let keys = canonical(&s.keys).unwrap_or_else(|| s.keys.clone());
            let n: u64 = keys
                .split(' ')
                .map(|c| 2 * c.split('+').count() as u64)
                .sum();
            (
                "send_keys".into(),
                format!("Sends {keys}."),
                json!({"keys": keys}),
                n,
            )
        }
        Action::TypeText(t) => {
            let method = t.method.map_or("unicode", |m| m.as_str());
            let p = crate::text::preview(&t.text, t.variables == Some(true), method);
            let chars = p["characters"].as_u64().unwrap_or(0);
            let lines = p["lines"].as_u64().unwrap_or(1);
            (
                "type_text".into(),
                format!(
                    "Types {chars} character{} on {lines} line{}.",
                    if chars == 1 { "" } else { "s" },
                    if lines == 1 { "" } else { "s" }
                ),
                p,
                2 * chars,
            )
        }
        Action::OpenProgram(o) => {
            let raw = o
                .path
                .clone()
                .or_else(|| o.app_id.clone())
                .unwrap_or_default();
            let name = if o.path.is_some() {
                crate::launch::program_name(&raw).to_string()
            } else {
                raw.split('!').next().unwrap_or(&raw).to_string()
            };
            let found = o.path.as_ref().is_some_and(|p| {
                crate::launch::expand(p, &crate::launch::process_var).is_some_and(|x| x.is_file())
            });
            let mut p = json!({"program": name, "found": found, "arguments": o.args.as_ref().map_or(0, Vec::len)});
            if let Some(r) = o.if_running {
                p["if_running"] = json!(match r {
                    IfRunning::StartNew => "start_new",
                    IfRunning::Activate => "activate",
                });
            }
            ("open_program".into(), format!("Opens {name}."), p, 0)
        }
        Action::OpenUrl(u) => {
            let p = crate::launch::url_preview(&u.url).unwrap_or_else(|| json!({}));
            let s = format!(
                "Opens a {} page on {}.",
                p["scheme"].as_str().unwrap_or("web"),
                p["host"].as_str().unwrap_or("?")
            );
            ("open_url".into(), s, p, 0)
        }
        Action::MouseMove(_) | Action::MouseScroll(_) | Action::MouseClick(_) => {
            // The engine's own mouse code against the simulated desktop (never the real pointer).
            let (px, py) = i.pointer;
            let p = crate::mouse::compile_step(&a.to_value())
                .ok()
                .and_then(|c| {
                    crate::mouse::preview(
                        &c,
                        &crate::mouse::Layout::single(DESKTOP.2, DESKTOP.3),
                        crate::mouse::Point::new(px, py),
                        Some(crate::mouse::WindowArea {
                            origin: crate::mouse::Point::new(100, 100),
                            dpi: 96,
                        }),
                    )
                    .ok()
                })
                .unwrap_or_else(|| json!({}));
            let ty = a.kind().to_string();
            let s = match ty.as_str() {
                "mouse_move" => format!("The pointer moves to {}, {}.", p["to"]["x"], p["to"]["y"]),
                "mouse_scroll" => format!(
                    "Scrolls {} by {}.",
                    p["direction"].as_str().unwrap_or("?"),
                    p["notches"]
                ),
                _ => format!(
                    "{} the {} button.",
                    p["op"].as_str().unwrap_or("click"),
                    p["button"].as_str().unwrap_or("?")
                ),
            };
            (ty, s, p, if a.kind() == "mouse_click" { 2 } else { 1 })
        }
        Action::Mode(m) => {
            let to = match m.op {
                ModeOp::Default | ModeOp::Previous => profile.default_mode.clone(),
                ModeOp::Toggle => {
                    let target = m.mode.clone().unwrap_or_default();
                    if mode == target {
                        profile.default_mode.clone()
                    } else {
                        target
                    }
                }
                ModeOp::Cycle => {
                    let list = m.modes.clone().unwrap_or_default();
                    let at = list
                        .iter()
                        .position(|x| x == mode)
                        .map_or(0, |k| (k + 1) % list.len().max(1));
                    list.get(at).cloned().unwrap_or_else(|| mode.to_string())
                }
                ModeOp::Set | ModeOp::Hold => m.mode.clone().unwrap_or_else(|| mode.to_string()),
            };
            let mut p = json!({"from": mode, "to": to});
            if m.op == ModeOp::Hold {
                p["until_release"] = json!(true);
            }
            (
                "mode".into(),
                format!("The mode changes from {mode} to {to}."),
                p,
                0,
            )
        }
        Action::Engine(e) => {
            let from = if i.paused { "paused" } else { "running" };
            let to = match e.op {
                EngineOp::Pause => "paused",
                EngineOp::Resume => "running",
                EngineOp::TogglePause => {
                    if i.paused {
                        "running"
                    } else {
                        "paused"
                    }
                }
                EngineOp::Disable => "off",
            };
            (
                "engine".into(),
                format!("The engine goes from {from} to {to}."),
                json!({"from": from, "to": to}),
                0,
            )
        }
        Action::Delay(d) => (
            "delay".into(),
            format!("Waits {} ms.", d.ms),
            json!({"ms": d.ms.clamp(1, 10_000)}),
            0,
        ),
        // Step 79: window, clipboard, notification and tool steps (never their personal content).
        Action::Window(w) => {
            let target = w.app.as_ref().map_or_else(
                || "active window".to_string(),
                |m| {
                    m.exe.clone().unwrap_or_else(|| {
                        crate::launch::program_name(m.path.as_deref().unwrap_or("")).to_string()
                    })
                },
            );
            let mut p = json!({"op": w.op.as_str(), "target": target});
            if w.op == crate::model::WindowOp::Move {
                let mut to = serde_json::Map::new();
                for (k, v) in [
                    ("x", w.x.map(i64::from)),
                    ("y", w.y.map(i64::from)),
                    ("width", w.width.map(i64::from)),
                    ("height", w.height.map(i64::from)),
                ] {
                    if let Some(v) = v {
                        to.insert(k.into(), json!(v));
                    }
                }
                p["to"] = Value::Object(to);
            }
            if w.op == crate::model::WindowOp::Topmost {
                p["on"] = json!(w.on != Some(false));
            }
            let s = format!(
                "{} {}.",
                match w.op.as_str() {
                    "activate" => "Brings to the front:",
                    "minimize" => "Minimizes",
                    "maximize" => "Maximizes",
                    "restore" => "Restores",
                    "move" => "Moves",
                    _ => "Keeps on top:",
                },
                p["target"].as_str().unwrap_or("")
            );
            ("window".into(), s, p, 0)
        }
        Action::Clipboard(c) => {
            let mut p = json!({"op": c.op.as_str()});
            if let Some(t) = &c.text {
                p["characters"] = json!(t.chars().count().max(1));
            }
            if let Some(t) = c.to {
                p["to"] = json!(t.as_str());
            }
            let s = match c.op.as_str() {
                "set" => format!("Puts {} characters on the clipboard.", p["characters"]),
                "clear" => "Empties the clipboard.".into(),
                _ => format!(
                    "Changes the clipboard text ({}).",
                    c.to.map_or("", |t| t.as_str())
                ),
            };
            ("clipboard".into(), s, p, 0)
        }
        Action::Notify(n) => {
            let p = json!({"title_characters": n.title.chars().count().max(1), "body_characters": n.body.as_ref().map_or(0, |b| b.chars().count())});
            ("notify".into(), "Shows a notification.".into(), p, 0)
        }
        // #102: the program's file name, the argument count and the timeout (never the
        // arguments, which may hold secrets).
        // #101: block markers and variables (a variable's value is never shown).
        Action::Repeat(r) => (
            "repeat".into(),
            format!("Repeats the steps up to its end {} times.", r.times),
            json!({"times": r.times}),
            0,
        ),
        Action::If(c) => {
            let holds = crate::actions::IfCond::compile(&Value::Object(c.when.clone()))
                .ok()
                .and_then(|c| match (&c.window, &c.var) {
                    (Some(w), None) => {
                        let fg = crate::context::FixedForeground {
                            app: i.app_exe.clone(),
                            title: i.window_title.clone(),
                        };
                        Some(w.holds(&fg, &mut crate::context::Probe::default()))
                    }
                    _ => None,
                });
            let summary = match holds {
                Some(true) => "Its condition holds here: the steps up to `else` or `end` run.",
                Some(false) => "Its condition does not hold here: the steps after `else` run.",
                None => "Its condition depends on a variable set while the macro runs.",
            };
            ("if".into(), summary.into(), json!({"holds": holds}), 0)
        }
        Action::Else(_) => ("else".into(), "Otherwise:".into(), json!({}), 0),
        Action::End(_) => ("end".into(), "Ends the block.".into(), json!({}), 0),
        Action::SetVar(v) => (
            "set_var".into(),
            format!("Sets the variable {}.", v.name),
            json!({"name": v.name, "op": if v.add.is_some() { "add" } else { "set" }}),
            0,
        ),
        // #102: which rule a timer starts, and when.
        Action::Timer(t) => {
            let p = json!({"rule": t.rule, "after_ms": t.after_ms});
            (
                "timer".into(),
                format!("Starts rule {} once after {} ms.", t.rule, t.after_ms),
                p,
                0,
            )
        }
        Action::RunCommand(c) => {
            let name = crate::launch::program_name(&c.program).to_string();
            let found = crate::launch::expand(&c.program, &crate::launch::process_var)
                .is_some_and(|p| p.is_file());
            let p = json!({"program": name, "found": found, "arguments": c.args.as_ref().map_or(0, Vec::len),
                "timeout_ms": c.timeout_ms.unwrap_or(60_000)});
            (
                "run_command".into(),
                format!("Runs {name} and waits for it."),
                p,
                0,
            )
        }
        Action::Tool(t) => {
            let names: Vec<String> = t
                .args
                .as_ref()
                .and_then(Value::as_object)
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            let p = json!({"capability": t.capability, "arguments": names});
            (
                "tool".into(),
                format!("Asks another Kim tool for {}.", t.capability),
                p,
                0,
            )
        }
        Action::Future { kind, .. } => (
            kind.clone(),
            "Not supported by this Ddalgak.".into(),
            json!({}),
            0,
        ),
    }
}

/// The simulation of one event against `doc` (a `ddalgak-simulation-v1` document).
pub fn simulate(doc: &Document, i: &Input) -> Result<Value, String> {
    let pid = i
        .profile
        .clone()
        .unwrap_or_else(|| doc.active_profile.clone());
    let profile = doc
        .profiles
        .iter()
        .find(|p| p.id == pid)
        .ok_or_else(|| format!("no profile `{pid}`"))?;
    let mode = i
        .mode
        .clone()
        .unwrap_or_else(|| profile.default_mode.clone());
    if !profile.modes.iter().any(|m| m.id == mode) {
        return Err(format!("profile `{pid}` has no mode `{mode}`"));
    }
    let keys = canonical(&i.keys).ok_or_else(|| format!("`{}` is not a key or chord", i.keys))?;
    let chain = chain(profile, &mode);
    // Candidates: enabled rules whose trigger matches, with their precedence key.
    struct C<'a> {
        rule: &'a Rule,
        order: usize,
        mode_rank: Option<usize>,
        unscoped: bool,
    }
    let mut cands: Vec<C<'_>> = Vec::new();
    for (order, e) in profile.rules.iter().enumerate() {
        let Some(r) = e.rule() else { continue };
        if !r.enabled || !trigger_matches(r, &keys, &i.on) {
            continue;
        }
        let (mode_rank, unscoped) = match &r.modes {
            None => (Some(usize::MAX), true),
            Some(ms) => (chain.iter().position(|c| ms.contains(c)), false),
        };
        cands.push(C {
            rule: r,
            order,
            mode_rank,
            unscoped,
        });
    }
    cands.sort_by(|a, b| {
        let key = |c: &C<'_>| {
            let w = c.rule.when.as_ref();
            (
                c.mode_rank.unwrap_or(usize::MAX - 1).min(if c.unscoped {
                    usize::MAX
                } else {
                    usize::MAX - 1
                }),
                !w.is_some_and(|w| w.apps.is_some()),
                !w.is_some_and(|w| w.window_title.is_some()),
                -c.rule.priority(),
                c.order,
            )
        };
        key(a).cmp(&key(b))
    });
    let mut candidates = Vec::new();
    let mut winner: Option<&Rule> = None;
    let mut skip: Option<(&'static str, &Rule)> = None;
    for c in &cands {
        let r = c.rule;
        let entry = |result: &str, reason: Option<String>| {
            let mut v = json!({"rule": r.id, "result": result});
            if let Some(x) = reason {
                v["reason"] = json!(x.chars().take(200).collect::<String>());
            }
            v
        };
        if winner.is_some() || skip.is_some() {
            if c.mode_rank.is_some() && condition_holds(r, i) {
                candidates.push(entry("lower_precedence", None));
            } else if c.mode_rank.is_none() {
                candidates.push(entry("mode_mismatch", None));
            } else {
                candidates.push(entry("condition_false", None));
            }
            continue;
        }
        if c.mode_rank.is_none() {
            candidates.push(entry("mode_mismatch", Some(format!("not in mode {mode}"))));
            continue;
        }
        if cond_of(r).is_err() {
            candidates.push(entry(
                "inactive",
                Some("a condition this Ddalgak does not know".into()),
            ));
            skip = Some(("condition_unsupported", r));
            continue;
        }
        if !condition_holds(r, i) {
            candidates.push(entry("condition_false", None));
            continue;
        }
        if r.steps.iter().any(|s| matches!(s, Action::Future { .. })) {
            candidates.push(entry(
                "inactive",
                Some("a step this Ddalgak does not support".into()),
            ));
            skip = Some(("step_unsupported", r));
            continue;
        }
        let inject = r.steps.iter().any(injects);
        let engine_only = r.steps.iter().all(|s| matches!(s, Action::Engine(_)));
        let step_aside = match i.focus.as_str() {
            "secure_desktop" => Some("skipped_secure_desktop"),
            "elevated_window" if inject => Some("skipped_elevated_target"),
            "password_field" if inject => Some("skipped_password_field"),
            _ if i.ime_composing && inject => Some("skipped_ime_composition"),
            _ => None,
        };
        let code = step_aside
            .or_else(|| (i.paused && !engine_only).then_some("skipped_paused"))
            .or_else(|| {
                (i.injected && r.trigger.injected != Some(InjectedPolicy::Accept))
                    .then_some("skipped_injected_input")
            })
            .or_else(|| {
                // A launch grant counts only for exactly the targets it was given for (§6.2).
                let v = r.to_value();
                r.permissions_needed()
                    .into_iter()
                    .any(|p| match r.grant(p) {
                        None => true,
                        Some(g) => crate::validate::fingerprint_for(p, &v)
                            .is_some_and(|f| g.fingerprint.as_deref() != Some(f.as_str())),
                    })
                    .then_some("consent_missing")
            });
        match code {
            Some(code) => {
                candidates.push(entry("inactive", Some(code.replace('_', " "))));
                skip = Some((code, r));
            }
            None => {
                candidates.push(entry("winner", None));
                winner = Some(r);
            }
        }
    }
    let mut effects = Vec::new();
    let mut injected = 0u64;
    let mut opened = Vec::new();
    // #101: how a macro unrolls (its `times` and `repeat` blocks), as a count of steps.
    let mut unrolled = None;
    let decision = if let Some(r) = winner {
        for (n, s) in r.steps.iter().enumerate().take(32) {
            let (ty, summary, preview, events) = effect(s, &keys, i, &mode, profile);
            let mut e = json!({"step": n, "type": ty, "performed": false, "summary": summary, "preview": preview});
            if i.kind == Kind::Live {
                match s {
                    Action::OpenProgram(_) | Action::OpenUrl(_) => {
                        if i.confirmed.contains(&n) {
                            e["performed"] = json!(true);
                            e["confirmed"] = json!(true);
                            opened.push(match s {
                                Action::OpenUrl(_) => format!(
                                    "{}://{}",
                                    e["preview"]["scheme"].as_str().unwrap_or(""),
                                    e["preview"]["host"].as_str().unwrap_or("")
                                ),
                                _ => e["preview"]["program"].as_str().unwrap_or("").to_string(),
                            });
                        } else {
                            e["needs_confirmation"] = json!(true);
                        }
                    }
                    // Into the playground's own test field only.
                    Action::Remap(_) | Action::SendKeys(_) | Action::TypeText(_) => {
                        e["performed"] = json!(true);
                        injected += events;
                    }
                    // The real pointer, mode and engine are never touched: simulated only.
                    _ => {}
                }
            }
            effects.push(e);
        }
        let times = u64::from(r.times.unwrap_or(1));
        let blocks = r
            .steps
            .iter()
            .any(|s| matches!(s, Action::Repeat(_) | Action::If(_)));
        if times > 1 || blocks {
            let steps: Vec<Value> = r.steps.iter().map(Action::to_value).collect();
            let modes = crate::modes::ModeSet::from_profile(profile);
            if let Ok(n) = crate::rules::check_macro(&steps, times, &modes) {
                unrolled = Some(json!({"times": times, "steps": n}));
            }
        }
        json!({"outcome": "fired", "code": "ok", "rule": r.id,
            "key_reaches_app": r.trigger.pass_through == Some(true), "candidates": candidates})
    } else if let Some((code, r)) = skip {
        json!({"outcome": "skipped", "code": code, "rule": r.id, "key_reaches_app": true, "candidates": candidates})
    } else {
        json!({"outcome": "no_match", "code": "no_match", "rule": null, "key_reaches_app": true, "candidates": candidates})
    };
    let mut out = json!({
        "schema": "ddalgak.simulation", "schema_version": 1,
        "kind": if i.kind == Kind::Live { "live" } else { "simulate" },
        "profile": pid, "revision": i.revision,
        "context": {"mode": mode, "engine": if i.paused { "paused" } else { "running" }, "focus": i.focus,
            "app": i.app_exe.as_ref().map(|e| {
                // A full path is matched as given but printed as its file name (privacy).
                let mut a = json!({"exe": crate::launch::program_name(e)});
                if let Some(t) = &i.window_title { a["window_title"] = json!(t.chars().take(200).collect::<String>()); }
                a
            }),
            "ime_composing": i.ime_composing},
        "event": {"keys": keys, "on": i.on, "source": if i.injected { "injected_by_other_program" } else { "physical" }},
        "decision": decision, "effects": effects, "injected_events": injected, "opened": opened,
    });
    if i.kind == Kind::Live {
        out["live_target"] = json!("playground");
    }
    if let Some(u) = unrolled {
        out["unrolled"] = u;
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
