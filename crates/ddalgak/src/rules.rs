//! The engine's view of `rules.json` (docs/DDALGAK_RULE_MODEL.md §2, §7, §9;
//! `DDALGAK-FR-006`, `DDALGAK-FR-009`, `DDALGAK-FR-017`, `DDALGAK-FR-021`).
//!
//! The file is read and checked by the rule store ([`crate::store`], [`crate::validate`]); this
//! module turns the result into what the engine runs and what `status` shows:
//!
//! - A file that is not usable (not JSON, not `ddalgak.rules`, a schema violation outside the
//!   rules, an unknown active profile or default mode) is **not loaded** (`rules_invalid`); a
//!   newer `schema_version` is not run either (`rules_from_newer_version`). The engine cannot be
//!   turned on with it, and a running engine keeps its previous rules.
//! - Of the active profile's enabled rules, every rule with an error or an `*_unsupported` info
//!   is **inactive** with that code; the others keep working.
//! - Of the remaining rules, those whose steps are all `engine` steps (§5.11), a `remap` (§5.1)
//!   and `block` (§5.2; Step 68), and since Step 69 a single `type_text` (§5.4) or `send_keys`
//!   (§5.3) step, with a keys or a hotstring trigger (§3.4), and since Step 70 rules made only of
//!   `open_program` (§5.5) and `open_url` (§5.6) steps with a keys trigger, and since Step 71
//!   rules made only of `mouse_move`, `mouse_scroll` and `mouse_click` steps (§5.7–§5.9; a held
//!   button or a move while held as the only step), and since Step 72 a single `mode` step (§5.10),
//!   become bindings. Every other action type (`delay`), a typing or mode step among other steps,
//!   and launches or mouse steps mixed with other steps are inactive with `step_unsupported` until
//!   ordered multi-step runs (Step 78).
//! - Modes (Step 72, [`crate::modes`]): every valid rule of the active profile becomes a binding
//!   with the bit set of the modes it is limited to; the table compiles one precedence list per
//!   mode. The engine starts in the profile's `default_mode`; `other_modes` counts the rules that
//!   do not work in it (they work after a mode switch).
//! - Precedence (§7.1): in the active mode, rules of that mode first, then of its parent and the
//!   parent's parents, then rules without `modes`; within each, higher `priority`, then file
//!   order. The bindings here are in priority and file order; [`crate::table::Table`] ranks them
//!   per mode.

use crate::hotstring;
use crate::model::{FloatingIndicator, ModeIndicator, Rule};
use crate::modes::{ModeAction, ModeSet};
use crate::remap::Output;
use crate::run::{KeysSpec, TextSpec};
use crate::store::{self, MIGRATIONS};
use crate::table::{Action, Binding, EngineOp, Injected, On, RemapSpec, SeqPlan, Stage};
use crate::trigger::{TKey, Trigger};
use crate::validate::Context;
use serde_json::{Map, Value, json};
use std::path::Path;
use std::sync::Arc;

pub use crate::store::FileState;

/// An enabled rule that does not run, with the code of DDALGAK_RULE_MODEL §8.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Inactive {
    pub rule: String,
    pub code: &'static str,
    pub reason: String,
}

/// The engine's view of `rules.json`.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub state: FileState,
    pub revision: Option<u64>,
    pub profile: String,
    /// The default mode (the mode the engine starts in).
    pub mode: String,
    /// The active profile's modes (Step 72).
    pub modes: ModeSet,
    /// `settings.mode_indicator` (Step 72).
    pub mode_indicator: Option<ModeIndicator>,
    /// `settings.floating_indicator` (Step 73).
    pub floating_indicator: Option<FloatingIndicator>,
    /// The active profile is enabled (Step 73; `enabled: false` keeps all its rules inactive).
    pub profile_enabled: bool,
    pub engine_on_start: bool,
    pub max_runs_per_second: u32,
    /// Active bindings (every mode) in priority and file order; the table ranks them per mode.
    pub bindings: Vec<Binding>,
    pub inactive: Vec<Inactive>,
    pub disabled: usize,
    /// Active rules that do not work in the default mode (only after a mode switch).
    pub other_modes: usize,
    /// Rules in the active profile.
    pub total: usize,
    /// A hash of the file's bytes: a reload applies only real changes.
    pub hash: u64,
    /// Errors and warnings in the whole document (every profile).
    pub errors: usize,
    pub warnings: usize,
}

impl Loaded {
    fn empty(state: FileState, revision: Option<u64>, hash: u64) -> Loaded {
        Loaded {
            state,
            revision,
            profile: "default".into(),
            mode: "base".into(),
            modes: ModeSet::single(),
            mode_indicator: None,
            floating_indicator: None,
            profile_enabled: true,
            engine_on_start: false,
            max_runs_per_second: 20,
            bindings: Vec::new(),
            inactive: Vec::new(),
            disabled: 0,
            other_modes: 0,
            total: 0,
            hash,
            errors: 0,
            warnings: 0,
        }
    }
}

/// The step types this version runs (Step 66: `engine`; Step 68: `remap`, `block`; Step 69:
/// `send_keys`, `type_text`; Step 70: `open_program`, `open_url`; Step 71: `mouse_move`,
/// `mouse_scroll`, `mouse_click`; Step 72: `mode`; Step 78: `delay`). A rule made of `engine` and
/// `block` steps is an engine-control rule; a `remap`, a `block` among other steps, a held button,
/// a mode hold and a move while held are always the rule's only step (schema `holds_trigger`);
/// any other mix, a delay or `times` above 1 is a macro (Step 78, ADR 0079).
pub const RUNNABLE_STEPS: [&str; 23] = [
    "engine",
    "remap",
    "block",
    "send_keys",
    "type_text",
    "open_program",
    "open_url",
    "mouse_move",
    "mouse_scroll",
    "mouse_click",
    "mode",
    "delay",
    "window",
    "clipboard",
    "notify",
    "tool",
    "run_command",
    "timer",
    "repeat",
    "if",
    "else",
    "end",
    "set_var",
];

const HOTSTRING_STEPS: &str =
    "a hotstring runs one `type_text` or `send_keys` step; a macro needs a key trigger";

/// Step 73: why a rule of a disabled profile does not run.
pub const PROFILE_DISABLED: &str = "the profile is disabled; `ddalgak enable --profile <id>` turns it on again (the rule keeps its own setting)";
/// Step 73: why a rule limited to disabled modes does not run.
pub const MODE_DISABLED: &str = "every mode the rule is limited to is disabled; `ddalgak enable --mode <id>` turns one on again";

fn is_launch(kind: &str) -> bool {
    matches!(kind, "open_program" | "open_url")
}

fn is_mouse(kind: &str) -> bool {
    matches!(kind, "mouse_move" | "mouse_scroll" | "mouse_click")
}

/// A combination of runnable steps this version does not run yet (`step_unsupported`): the step
/// index and why.
pub fn later_step(rule: &Rule) -> Option<(usize, String)> {
    use crate::model::Action as A;
    let typing = |a: &A| matches!(a, A::TypeText(_) | A::SendKeys(_));
    if rule.trigger.hotstring.is_some() {
        if let Some(i) = rule.steps.iter().position(|s| !typing(s)) {
            return Some((i, HOTSTRING_STEPS.into()));
        }
        if rule.steps.len() > 1 {
            return Some((1, HOTSTRING_STEPS.into()));
        }
    }
    // Step 78: every other mix of steps runs as a macro (ADR 0079).
    None
}

/// Reads and interprets the rules file.
pub fn load(path: &Path) -> Loaded {
    load_with(path, &Context::default())
}

/// Reads and interprets the rules file with what the checks know beyond it (Ddalgak's own
/// shortcuts).
pub fn load_with(path: &Path, ctx: &Context) -> Loaded {
    from_store(&store::read_path(path, ctx, MIGRATIONS))
}

/// Interprets the bytes of a rules file.
pub fn parse(bytes: &[u8]) -> Loaded {
    from_store(&store::interpret(bytes, &Context::default(), MIGRATIONS))
}

const RULE_FIELDS: &[&str] = &[
    "id",
    "title",
    "description",
    "enabled",
    "trigger",
    "modes",
    "when",
    "priority",
    "steps",
    "on_error",
    "times",
    "consent",
    "created_at",
    "updated_at",
];
const TRIGGER_FIELDS: &[&str] = &[
    "keys",
    "hotstring",
    "on",
    "hold_ms",
    "repeat",
    "pass_through",
    "injected",
    "schedule",
];

const HOTSTRING_FIELDS: &[&str] = &[
    "abbreviation",
    "case",
    "inside_words",
    "immediate",
    "end_chars",
    "omit_end",
    "erase",
];

/// Compiles `trigger.hotstring` (§3.4).
fn compile_hotstring(h: &Value) -> Result<hotstring::Spec, (&'static str, String)> {
    let o = h.as_object().ok_or((
        "trigger_invalid",
        "`trigger.hotstring` must be an object".to_string(),
    ))?;
    if let Some(f) = unknown_field(o, HOTSTRING_FIELDS) {
        return Err(("rule_unsupported", format!("unknown hotstring field `{f}`")));
    }
    let bad = |what: &str| {
        (
            "trigger_invalid",
            format!("`trigger.hotstring.{what}` has a wrong type or value"),
        )
    };
    let abbreviation = o
        .get("abbreviation")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("abbreviation"))?;
    let len = abbreviation.chars().count();
    if !(1..=hotstring::MAX_ABBREVIATION).contains(&len)
        || abbreviation
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(bad("abbreviation"));
    }
    let flag = |k: &str, default: bool| -> Result<bool, (&'static str, String)> {
        match o.get(k) {
            None => Ok(default),
            Some(v) => v.as_bool().ok_or_else(|| bad(k)),
        }
    };
    let mut spec = hotstring::Spec::new(abbreviation);
    spec.case = match o
        .get("case")
        .map(|v| v.as_str().and_then(hotstring::Case::parse))
    {
        None => hotstring::Case::Conform,
        Some(Some(c)) => c,
        Some(None) => return Err(bad("case")),
    };
    spec.inside_words = flag("inside_words", false)?;
    spec.immediate = flag("immediate", false)?;
    spec.omit_end = flag("omit_end", false)?;
    spec.erase = flag("erase", true)?;
    if let Some(e) = o.get("end_chars") {
        let e = e
            .as_str()
            .filter(|e| !e.is_empty())
            .ok_or_else(|| bad("end_chars"))?;
        spec.end_chars = e.chars().collect();
    }
    Ok(spec)
}

/// Compiles a typing step (`type_text`, `send_keys`).
/// Step 78: whether these steps, run `times` times, form a macro the engine accepts (the
/// editor's check before saving); `Ok` is the number of stages.
pub fn check_macro(steps: &[Value], times: u64, modes: &ModeSet) -> Result<usize, String> {
    if steps.is_empty() || steps.len() > 32 {
        return Err("a macro has 1–32 steps".into());
    }
    if !(1..=100).contains(&times) {
        return Err("repeat 1–100 times".into());
    }
    let n = compile_stages(steps, modes).map_err(|(_, e)| e)?.len()
        * usize::try_from(times).unwrap_or(1);
    if n > crate::table::MAX_STAGES {
        return Err(format!(
            "{n} steps in all; at most {}",
            crate::table::MAX_STAGES
        ));
    }
    Ok(n)
}

/// Step 78: a macro's steps as stages, each compiled exactly as when it runs alone.
fn compile_stages(steps: &[Value], modes: &ModeSet) -> Result<Vec<Stage>, (&'static str, String)> {
    // #101: `repeat` … `end` and `if` … `else` … `end` are block markers, nested at most
    // `MAX_DEPTH` deep; the tree is unrolled into stages with relative jumps.
    let mut stack: Vec<Frame> = vec![Frame::default()];
    let mut set: Vec<&str> = Vec::new();
    let mut used: Vec<(usize, String)> = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        let ty = s.get("type").and_then(Value::as_str).unwrap_or_default();
        let bad = |e: String| ("step_invalid", format!("step {i}: {e}"));
        match ty {
            "repeat" | "if" => {
                if stack.len() > MAX_DEPTH {
                    return Err(bad(format!("blocks nest at most {MAX_DEPTH} deep")));
                }
                let kind = if ty == "repeat" {
                    let n = s
                        .get("times")
                        .and_then(Value::as_u64)
                        .filter(|n| (1..=100).contains(n))
                        .ok_or_else(|| bad("`times` is 1–100".into()))?;
                    Block::Repeat(u32::try_from(n).unwrap_or(1))
                } else {
                    let when = s.get("when").cloned().unwrap_or(Value::Null);
                    let c = crate::actions::IfCond::compile(&when).map_err(bad)?;
                    if let Some((name, _)) = &c.var {
                        used.push((i, name.clone()));
                    }
                    Block::If(c)
                };
                stack.push(Frame {
                    block: Some(kind),
                    ..Frame::default()
                });
                continue;
            }
            "else" => {
                match stack.last_mut() {
                    Some(f) if matches!(f.block, Some(Block::If(_))) && f.otherwise.is_none() => {
                        f.otherwise = Some(Vec::new());
                    }
                    _ => return Err(bad("`else` belongs right inside an `if`".into())),
                }
                continue;
            }
            "end" => {
                if stack.len() == 1 {
                    return Err(bad("`end` without a `repeat` or `if`".into()));
                }
                let f = stack.pop().expect("more than the root");
                let node = match f.block.expect("an open block") {
                    Block::Repeat(n) => Node::Repeat(n, f.body),
                    Block::If(c) => Node::If(Arc::new(c), f.body, f.otherwise.unwrap_or_default()),
                };
                stack.last_mut().expect("root").push(node);
                continue;
            }
            "set_var" => {
                let v = crate::actions::SetVarSpec::compile(s).map_err(bad)?;
                if !set.contains(&v.name()) {
                    set.push(s["name"].as_str().unwrap_or_default());
                }
                stack
                    .last_mut()
                    .expect("root")
                    .push(Node::Leaf(Stage::SetVar(Arc::new(v))));
                continue;
            }
            "type_text" if s["variables"] == true => {
                let text = s["text"].as_str().unwrap_or_default();
                for p in crate::text::parse(text).unwrap_or_default() {
                    if let crate::text::Piece::User(u) = p {
                        used.push((i, u.to_string()));
                    }
                }
            }
            _ => {}
        }
        let leaf = match ty {
            "type_text" | "send_keys" => match compile_run(s, ty)? {
                Action::Type(t) => Stage::Text(t),
                Action::Keys(k) => Stage::Keys(k),
                _ => return Err(bad("not a typing step".into())),
            },
            "delay" => {
                let ms = s
                    .get("ms")
                    .and_then(Value::as_u64)
                    .filter(|m| (1..=10_000).contains(m));
                Stage::Wait(
                    u32::try_from(ms.ok_or_else(|| bad("`ms` is 1–10000".into()))?).unwrap_or(1),
                )
            }
            "mouse_move" | "mouse_scroll" | "mouse_click" => {
                match crate::mouse::compile_step(s).map_err(bad)? {
                    crate::mouse::Compiled::Run(m) => {
                        Stage::Mouse(Arc::new(crate::mouse::MousePlan {
                            steps: vec![m],
                            stop_on_error: true,
                        }))
                    }
                    _ => {
                        return Err(bad(
                            "a held button or a move while held is the rule's only step".into(),
                        ));
                    }
                }
            }
            "open_program" | "open_url" => Stage::Launch(Arc::new(crate::launch::Plan {
                targets: vec![crate::launch::compile_step(s).map_err(bad)?],
                stop_on_error: true,
            })),
            "engine" => Stage::Engine(
                s.get("op")
                    .and_then(Value::as_str)
                    .and_then(EngineOp::parse)
                    .ok_or_else(|| bad("unknown engine `op`".into()))?,
            ),
            "mode" => {
                let a = ModeAction::compile(s, modes).map_err(|e| {
                    let code = if e.starts_with("no mode") {
                        "unknown_mode"
                    } else {
                        "step_invalid"
                    };
                    (code, format!("step {i}: {e}"))
                })?;
                if a.holds() {
                    return Err(bad("a mode hold is the rule's only step".into()));
                }
                if a.targets_disabled(modes) {
                    return Err((
                        "mode_disabled",
                        format!("step {i}: the mode it switches to is disabled"),
                    ));
                }
                Stage::Mode(a)
            }
            "remap" | "block" => {
                return Err(bad(format!("a `{ty}` is the rule's only step")));
            }
            // Step 79 (ADR 0080).
            "window" => Stage::Window(Arc::new(compile_window(s).map_err(bad)?)),
            "clipboard" => Stage::Clip(Arc::new(compile_clipboard(s).map_err(bad)?)),
            "notify" => {
                let title = s
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                let body = s.get("body").and_then(Value::as_str).unwrap_or("").trim();
                if !(1..=63).contains(&title.chars().count()) || body.chars().count() > 255 {
                    return Err(bad(
                        "a notification has a title of 1–63 characters and a body of at most 255"
                            .into(),
                    ));
                }
                Stage::Notify(Arc::new(crate::actions::NotifySpec {
                    title: title.to_string(),
                    body: body.to_string(),
                }))
            }
            // #102: a local command, waited for; hidden; its output is never read.
            "run_command" => Stage::Command(Arc::new(compile_command(s).map_err(bad)?)),
            // #102: a one-shot timer for another rule.
            "timer" => Stage::Timer(Arc::new(
                crate::actions::TimerSpec::compile(s).map_err(bad)?,
            )),
            "tool" => {
                let cap = s.get("capability").and_then(Value::as_str).unwrap_or("");
                let ok = cap.len() <= 64
                    && cap.split('.').count() >= 2
                    && cap.split('.').all(|p| {
                        p.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                            && p.chars()
                                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    });
                if !ok {
                    return Err(bad("`capability` is a name like `pin.list`".into()));
                }
                let args = s.get("args").cloned().unwrap_or_else(|| json!({}));
                if !args.is_object() {
                    return Err(bad("`args` is an object".into()));
                }
                Stage::Tool(Arc::new(crate::actions::ToolSpec {
                    capability: cap.to_string(),
                    args,
                }))
            }
            _ => {
                return Err((
                    "step_unsupported",
                    format!("`{ty}` steps are not implemented in this version yet"),
                ));
            }
        };
        stack.last_mut().expect("root").push(Node::Leaf(leaf));
    }
    if stack.len() > 1 {
        return Err(("step_invalid", "a `repeat` or `if` has no `end`".into()));
    }
    if set.len() > crate::actions::MAX_VARS {
        return Err((
            "step_invalid",
            format!("at most {} variables in one rule", crate::actions::MAX_VARS),
        ));
    }
    if let Some((i, name)) = used.iter().find(|(_, n)| !set.contains(&n.as_str())) {
        return Err((
            "step_invalid",
            format!("step {i}: the variable `{name}` is never set (`set_var`)"),
        ));
    }
    let root = stack.pop().expect("root").body;
    let size = nodes_size(&root);
    if size > crate::table::MAX_STAGES {
        return Err((
            "step_invalid",
            format!(
                "the steps with their repeats make {size} steps; at most {}",
                crate::table::MAX_STAGES
            ),
        ));
    }
    let mut out = Vec::with_capacity(size);
    emit(&root, &mut out);
    Ok(out)
}

/// #101: how deep `repeat` and `if` blocks may nest.
pub const MAX_DEPTH: usize = 3;

enum Block {
    Repeat(u32),
    If(crate::actions::IfCond),
}

/// One open block while the steps are read: its body, and an `if`'s `else` branch once seen.
#[derive(Default)]
struct Frame {
    block: Option<Block>,
    body: Vec<Node>,
    otherwise: Option<Vec<Node>>,
}

impl Frame {
    fn push(&mut self, n: Node) {
        match &mut self.otherwise {
            Some(o) => o.push(n),
            None => self.body.push(n),
        }
    }
}

enum Node {
    Leaf(Stage),
    Repeat(u32, Vec<Node>),
    If(Arc<crate::actions::IfCond>, Vec<Node>, Vec<Node>),
}

/// The number of stages `nodes` unroll to (saturating, so a huge repeat is refused, not built).
fn nodes_size(nodes: &[Node]) -> usize {
    nodes.iter().fold(0usize, |acc, n| {
        acc.saturating_add(match n {
            Node::Leaf(_) => 1,
            Node::Repeat(times, body) => nodes_size(body).saturating_mul(*times as usize),
            Node::If(_, then, other) => {
                let o = nodes_size(other);
                1usize
                    .saturating_add(nodes_size(then))
                    .saturating_add(if o > 0 { o.saturating_add(1) } else { 0 })
            }
        })
    })
}

/// Unrolls repeats and lays out branches: `Branch { skip }` passes over the `then` stages (and
/// the `Skip` after them) when the condition fails; `Skip(n)` passes over the `else` stages.
fn emit(nodes: &[Node], out: &mut Vec<Stage>) {
    for n in nodes {
        match n {
            Node::Leaf(s) => out.push(s.clone()),
            Node::Repeat(times, body) => {
                for _ in 0..*times {
                    emit(body, out);
                }
            }
            Node::If(cond, then, other) => {
                let mut t = Vec::new();
                emit(then, &mut t);
                let mut e = Vec::new();
                emit(other, &mut e);
                let skip = t.len() + usize::from(!e.is_empty());
                out.push(Stage::Branch(Arc::new(crate::actions::BranchSpec {
                    cond: (**cond).clone(),
                    skip: u16::try_from(skip).unwrap_or(u16::MAX),
                })));
                out.extend(t);
                if !e.is_empty() {
                    out.push(Stage::Skip(u16::try_from(e.len()).unwrap_or(u16::MAX)));
                    out.extend(e);
                }
            }
        }
    }
}

/// Step 79: a `window` step.
fn compile_window(s: &Value) -> Result<crate::actions::WindowSpec, String> {
    use crate::model::WindowOp;
    let op: WindowOp = serde_json::from_value(s.get("op").cloned().unwrap_or(Value::Null))
        .map_err(|_| "unknown window `op`".to_string())?;
    let app = match s.get("app") {
        None => None,
        Some(m) => {
            let c = crate::context::Cond::compile(&json!({ "apps": [m] }))?;
            c.apps.into_iter().next()
        }
    };
    let int = |k: &str| s.get(k).and_then(Value::as_i64);
    let pos = match (int("x"), int("y")) {
        (Some(x), Some(y)) => Some((
            i32::try_from(x).map_err(|_| "x is too large")?,
            i32::try_from(y).map_err(|_| "y is too large")?,
        )),
        (None, None) => None,
        _ => return Err("give both `x` and `y`".into()),
    };
    let size = match (int("width"), int("height")) {
        (Some(w), Some(h)) if (50..=32767).contains(&w) && (50..=32767).contains(&h) => Some((
            u32::try_from(w).unwrap_or(50),
            u32::try_from(h).unwrap_or(50),
        )),
        (None, None) => None,
        _ => return Err("give both `width` and `height` (50–32767)".into()),
    };
    if op == WindowOp::Move && pos.is_none() && size.is_none() {
        return Err("a move needs a position or a size".into());
    }
    if op == WindowOp::Activate && app.is_none() {
        return Err("`activate` names the program (`app`)".into());
    }
    Ok(crate::actions::WindowSpec {
        op,
        app,
        pos,
        size,
        on: s.get("on").and_then(Value::as_bool) != Some(false),
    })
}

/// #102: a `run_command` step: an absolute program path (known-folder tokens expanded), up to 32
/// arguments, an optional absolute working folder, a timeout of 100 ms–10 min (default 60 s).
pub(crate) fn compile_command(s: &Value) -> Result<crate::actions::CommandSpec, String> {
    let raw = s.get("program").and_then(Value::as_str).unwrap_or("");
    let program = crate::launch::expand(raw, &crate::launch::process_var)
        .filter(|p| p.is_absolute())
        .ok_or("`program` is a full path to a program (or starts with a known folder)")?;
    let args: Vec<String> = match s.get("args") {
        None => Vec::new(),
        Some(a) => a
            .as_array()
            .filter(|a| a.len() <= 32)
            .ok_or("`args` lists at most 32 texts")?
            .iter()
            .map(|x| {
                x.as_str()
                    .map(str::to_string)
                    .filter(|t| t.len() <= 4096 && !t.contains('\0'))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or("every argument is a text of at most 4096 characters")?,
    };
    let working_dir = match s.get("working_dir").and_then(Value::as_str) {
        None => None,
        Some(d) => Some(
            crate::launch::expand(d, &crate::launch::process_var)
                .filter(|p| p.is_absolute())
                .ok_or("`working_dir` is a full path")?,
        ),
    };
    let timeout_ms = s
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(60_000);
    if !(100..=600_000).contains(&timeout_ms) {
        return Err("`timeout_ms` is 100–600000".into());
    }
    Ok(crate::actions::CommandSpec {
        program,
        args,
        working_dir,
        timeout_ms: u32::try_from(timeout_ms).unwrap_or(60_000),
    })
}

/// Step 79: a `clipboard` step.
fn compile_clipboard(s: &Value) -> Result<crate::actions::ClipSpec, String> {
    use crate::actions::ClipSpec;
    match s.get("op").and_then(Value::as_str) {
        Some("set") => {
            let t = s.get("text").and_then(Value::as_str).unwrap_or("");
            if !(1..=20_000).contains(&t.chars().count()) {
                return Err("`set` takes a text of 1–20000 characters".into());
            }
            Ok(ClipSpec::Set(t.to_string()))
        }
        Some("clear") => Ok(ClipSpec::Clear),
        Some("transform") => serde_json::from_value(s.get("to").cloned().unwrap_or(Value::Null))
            .map(ClipSpec::Transform)
            .map_err(|_| "`transform` takes `to`: upper, lower, trim or plain".into()),
        _ => Err("unknown clipboard `op`".into()),
    }
}

fn compile_run(s: &Value, ty: &str) -> Result<Action, (&'static str, String)> {
    if ty == "type_text" {
        let text = s.get("text").and_then(Value::as_str).ok_or((
            "step_invalid",
            "a `type_text` step needs `text`".to_string(),
        ))?;
        return Ok(Action::Type(Arc::new(TextSpec {
            text: text.to_string(),
            paste: s.get("method").and_then(Value::as_str) == Some("paste"),
            key_delay_ms: s
                .get("key_delay_ms")
                .and_then(Value::as_u64)
                .map_or(0, |d| u32::try_from(d.min(200)).unwrap_or(0)),
            variables: s.get("variables").and_then(Value::as_bool) == Some(true),
        })));
    }
    let keys = s.get("keys").and_then(Value::as_str).ok_or((
        "step_invalid",
        "a `send_keys` step needs `keys`".to_string(),
    ))?;
    let chords = keys
        .split(' ')
        .map(|k| match Output::parse(k) {
            Ok(o) if matches!(o.key, Some(crate::inject::OutCode::Button(_))) => Err((
                "step_invalid",
                format!("`{k}`: send_keys presses keys, not mouse buttons"),
            )),
            Ok(o) => Ok(o),
            Err(e) => Err(("step_invalid", e)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Action::Keys(Arc::new(KeysSpec {
        chords,
        step_delay_ms: s
            .get("step_delay_ms")
            .and_then(Value::as_u64)
            .map_or(0, |d| u32::try_from(d.min(1000)).unwrap_or(0)),
    })))
}

struct Candidate {
    binding: Binding,
    priority: i64,
    order: usize,
}

/// The engine's interpretation of a checked document.
pub fn from_store(l: &store::Loaded) -> Loaded {
    let (Some(doc), true) = (&l.doc, l.state.usable()) else {
        return Loaded::empty(l.state.clone(), l.revision, l.hash);
    };
    let settings = doc.settings.as_ref();
    let pi = doc
        .profile_index(&doc.active_profile)
        .expect("a usable document has its active profile");
    let profile = &doc.profiles[pi];
    let modes = ModeSet::from_profile(profile);
    let base = modes.default_mode();
    let mut out = Loaded {
        state: l.state.clone(),
        revision: doc.revision.or(l.revision),
        profile: profile.id.clone(),
        mode: profile.default_mode.clone(),
        modes: modes.clone(),
        mode_indicator: settings.and_then(|s| s.mode_indicator),
        floating_indicator: settings.and_then(|s| s.floating_indicator),
        engine_on_start: settings.and_then(|s| s.engine_on_start) == Some(crate::model::OnOff::On),
        max_runs_per_second: settings
            .and_then(|s| s.max_runs_per_second)
            .map_or(20, |n| n.clamp(1, 100)),
        total: profile.rules.len(),
        errors: l.report.count(crate::validate::Severity::Error),
        warnings: l.report.count(crate::validate::Severity::Warning),
        ..Loaded::empty(l.state.clone(), l.revision, l.hash)
    };
    out.profile_enabled = profile.is_enabled();
    let mut candidates: Vec<Candidate> = Vec::new();
    for (ri, entry) in profile.rules.iter().enumerate() {
        let id = entry.id().unwrap_or_default().to_string();
        if entry.enabled() != Some(true) {
            out.disabled += 1;
            continue;
        }
        if let Some(p) = l.report.inactive((pi, ri)) {
            out.inactive.push(Inactive {
                rule: id,
                code: p.code,
                reason: p.message.clone(),
            });
            continue;
        }
        // Step 73: the user disabled the whole profile; its rules keep their own `enabled`.
        if !out.profile_enabled {
            out.inactive.push(Inactive {
                rule: id,
                code: "profile_disabled",
                reason: PROFILE_DISABLED.into(),
            });
            continue;
        }
        let Some(rule) = entry.rule() else {
            continue; // a raw rule always has a schema problem (above)
        };
        let mask = match &rule.modes {
            None => 0,
            Some(m) => match modes.mask(m) {
                // Step 73: a rule limited to disabled modes only does not run.
                Some(mask) if modes.enabled_mask(mask) == 0 => {
                    out.inactive.push(Inactive {
                        rule: id,
                        code: "mode_disabled",
                        reason: MODE_DISABLED.into(),
                    });
                    continue;
                }
                Some(mask) => modes.enabled_mask(mask),
                None => {
                    out.inactive.push(Inactive {
                        rule: id,
                        code: "unknown_mode",
                        reason: "the rule names a mode that does not exist in this profile".into(),
                    });
                    continue;
                }
            },
        };
        let Some(map) = rule.to_value().as_object().cloned() else {
            continue;
        };
        match compile(&map, &modes) {
            Ok((mut binding, priority)) => {
                binding.modes = mask;
                if modes.rank(base, mask).is_none() {
                    out.other_modes += 1;
                }
                candidates.push(Candidate {
                    binding,
                    priority,
                    order: ri,
                });
            }
            Err((code, reason)) => out.inactive.push(Inactive {
                rule: id,
                code,
                reason,
            }),
        }
    }
    // §7.1 step 6 after the mode rank (which the table applies with a stable sort): rules with
    // `when.apps`, then with `when.window_title`, then higher priority, then file order.
    candidates.sort_by(|a, b| {
        let key = |c: &Candidate| crate::context::Cond::rank(c.binding.when.as_deref());
        key(a)
            .cmp(&key(b))
            .then(b.priority.cmp(&a.priority))
            .then(a.order.cmp(&b.order))
    });
    out.bindings = candidates.into_iter().map(|c| c.binding).collect();
    out
}

impl Loaded {
    /// The compiled table of these rules for the hook.
    pub fn table(&self) -> crate::table::Table {
        crate::table::Table::compile_modes(
            self.bindings.clone(),
            self.modes.clone(),
            self.max_runs_per_second,
        )
    }
}

fn unknown_field(o: &Map<String, Value>, known: &[&str]) -> Option<String> {
    o.keys().find(|k| !known.contains(&k.as_str())).cloned()
}

/// Compiles one checked rule into a binding; `Err` keeps it inactive. The validation already
/// refused everything below; these checks are the last line of defense ("never broader").
fn compile(
    r: &Map<String, Value>,
    modes: &ModeSet,
) -> Result<(Binding, i64), (&'static str, String)> {
    if let Some(f) = unknown_field(r, RULE_FIELDS) {
        return Err(("rule_unsupported", format!("unknown field `{f}`")));
    }
    // Step 77: conditions on the focused app and window (§7.2); anything unknown stays
    // inactive, never unconditional.
    let when = r
        .get("when")
        .map(crate::context::Cond::compile)
        .transpose()
        .map_err(|e| ("condition_unsupported", e))?
        .map(Arc::new);
    let priority = r.get("priority").and_then(Value::as_i64).unwrap_or(0);
    let t = r
        .get("trigger")
        .and_then(Value::as_object)
        .ok_or(("trigger_invalid", "`trigger` must be an object".to_string()))?;
    if let Some(f) = unknown_field(t, TRIGGER_FIELDS) {
        return Err(("rule_unsupported", format!("unknown trigger field `{f}`")));
    }
    let hotstring = t.get("hotstring").map(compile_hotstring).transpose()?;
    // Step 79: a schedule fires by the clock; its rule is a macro the worker times.
    let schedule = t
        .get("schedule")
        .map(crate::actions::ScheduleSpec::compile)
        .transpose()
        .map_err(|e| ("trigger_invalid", e))?
        .map(Arc::new);
    let trigger = match (t.get("keys"), &hotstring) {
        _ if schedule.is_some() => {
            if t.contains_key("keys") || hotstring.is_some() {
                return Err((
                    "trigger_invalid",
                    "a schedule has no `keys` or `hotstring`".into(),
                ));
            }
            Trigger::none()
        }
        (Some(k), None) => {
            let k = k
                .as_str()
                .ok_or(("trigger_invalid", "`trigger.keys` must be text".to_string()))?;
            Trigger::parse(k).map_err(|e| (e.code(), e.to_string()))?
        }
        (None, Some(_)) => Trigger::none(),
        _ => {
            return Err((
                "trigger_invalid",
                "a trigger has `keys` or a `hotstring`, not both".into(),
            ));
        }
    };
    let bad = |k: &str| {
        (
            "trigger_invalid",
            format!("`trigger.{k}` has a wrong type or value"),
        )
    };
    let on = match t.get("on").and_then(Value::as_str) {
        None | Some("press") => On::Press,
        Some("release") => On::Release,
        Some("hold") => {
            let ms = t.get("hold_ms").and_then(Value::as_u64).unwrap_or(500);
            On::Hold(u32::try_from(ms).unwrap_or(500))
        }
        Some(_) => return Err(bad("on")),
    };
    let repeat = t.get("repeat").and_then(Value::as_bool).unwrap_or(false);
    let pass_through = t
        .get("pass_through")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let injected = match t.get("injected").and_then(Value::as_str) {
        None | Some("ignore") => Injected::Ignore,
        Some("accept") => Injected::Accept,
        Some(_) => return Err(bad("injected")),
    };
    let steps = r
        .get("steps")
        .and_then(Value::as_array)
        .ok_or(("step_invalid", "`steps` must hold 1–32 steps".to_string()))?;
    let is_remap = steps.first().and_then(|s| s.get("type")) == Some(&json!("remap"));
    let kinds: Vec<&str> = steps
        .iter()
        .map(|s| s.get("type").and_then(Value::as_str).unwrap_or_default())
        .collect();
    let typing = |k: &&str| matches!(*k, "type_text" | "send_keys");
    if hotstring.is_some() && (kinds.len() != 1 || !kinds.iter().all(typing)) {
        return Err(("step_unsupported", HOTSTRING_STEPS.into()));
    }
    // Step 78 (ADR 0079): mixed steps, a delay or `times` above 1 make the rule a macro. Steps
    // of one kind keep their own run (launches together, mouse steps together, engine steps).
    let times = r.get("times").and_then(Value::as_u64).unwrap_or(1);
    if !(1..=100).contains(&times) {
        return Err(("step_invalid", "`times` is 1–100".into()));
    }
    let same = |f: fn(&str) -> bool| kinds.iter().all(|k| f(k));
    // Step 79: window, clipboard, notification and tool steps, and every scheduled rule, run as
    // macro stages on the worker.
    let macro_run = times > 1
        || schedule.is_some()
        || kinds.iter().any(|k| {
            matches!(
                *k,
                "delay"
                    | "window"
                    | "clipboard"
                    | "notify"
                    | "tool"
                    | "run_command"
                    | "timer"
                    | "repeat"
                    | "if"
                    | "set_var"
            )
        })
        || (kinds.len() > 1
            && !same(is_launch)
            && !same(is_mouse)
            && !kinds.iter().all(|k| *k == "engine" || *k == "block"));
    if macro_run && hotstring.is_some() {
        return Err(("step_unsupported", HOTSTRING_STEPS.into()));
    }
    if let Some(c) = trigger.single()
        && matches!(c.key, TKey::Modifier(_))
        && on != On::Release
        && !is_remap
    {
        return Err((
            "trigger_invalid",
            "a bare modifier fires a non-remap rule only with `on: release`".into(),
        ));
    }
    if macro_run {
        let continue_on_error = r.get("on_error").and_then(Value::as_str) == Some("continue");
        let once = compile_stages(steps, modes)?;
        // #102: a timer never restarts its own rule (that would be a hidden periodic loop).
        let own = r.get("id").and_then(Value::as_str).unwrap_or_default();
        if once
            .iter()
            .any(|st| matches!(st, Stage::Timer(t) if t.rule == own))
        {
            return Err(("step_invalid", "a timer cannot start its own rule".into()));
        }
        let total = once
            .len()
            .saturating_mul(usize::try_from(times).unwrap_or(1));
        if total > crate::table::MAX_STAGES {
            return Err((
                "step_invalid",
                format!(
                    "the steps × `times` make {total} steps; at most {}",
                    crate::table::MAX_STAGES
                ),
            ));
        }
        let stages: Vec<Stage> = std::iter::repeat_n(once, usize::try_from(times).unwrap_or(1))
            .flatten()
            .collect();
        let injects = stages.iter().any(Stage::injects);
        if schedule.is_some() && injects {
            return Err((
                "step_invalid",
                "a scheduled rule never types, presses keys or moves the pointer (nobody is at the keys when it fires)".into(),
            ));
        }
        return Ok((
            Binding {
                rule: r["id"].as_str().unwrap_or_default().to_string(),
                trigger,
                on,
                repeat,
                pass_through,
                injected,
                widen: false,
                injects,
                action: Action::Sequence(Arc::new(SeqPlan {
                    stages,
                    continue_on_error,
                })),
                hotstring: None,
                modes: 0,
                when,
                schedule,
            },
            priority,
        ));
    }
    let mut ops = Vec::new();
    let mut remap = None;
    let mut run = None;
    let mut targets = Vec::new();
    let mut mouse = Vec::new();
    let mut held = None;
    let mut mode = None;
    // Steps that last while the trigger is held: the rule's only step, on a single chord pressed
    // without repeat or pass-through (schema `holds_trigger`; checked again here).
    let holds_ok =
        steps.len() == 1 && !trigger.is_sequence() && on == On::Press && !repeat && !pass_through;
    for (i, s) in steps.iter().enumerate() {
        let ty = s.get("type").and_then(Value::as_str).unwrap_or_default();
        match ty {
            "mouse_move" | "mouse_scroll" | "mouse_click" => {
                match crate::mouse::compile_step(s)
                    .map_err(|e| ("step_invalid", format!("step {i}: {e}")))?
                {
                    crate::mouse::Compiled::Run(m) => mouse.push(m),
                    h if holds_ok => held = Some(h),
                    _ => {
                        return Err((
                            "step_invalid",
                            "a held button or a move while held is the rule's only step, on a single chord pressed without repeat or pass-through"
                                .into(),
                        ));
                    }
                }
            }
            "open_program" | "open_url" => targets.push(
                crate::launch::compile_step(s)
                    .map_err(|e| ("step_invalid", format!("step {i}: {e}")))?,
            ),
            "engine" => {
                let op = s
                    .get("op")
                    .and_then(Value::as_str)
                    .and_then(EngineOp::parse)
                    .ok_or(("step_invalid", format!("step {i}: unknown engine `op`")))?;
                ops.push(op);
            }
            "block" => {}
            "mode" => {
                let a = ModeAction::compile(s, modes).map_err(|e| {
                    let code = if e.starts_with("no mode") {
                        "unknown_mode"
                    } else {
                        "step_invalid"
                    };
                    (code, format!("step {i}: {e}"))
                })?;
                if a.holds() && !holds_ok {
                    return Err((
                        "step_invalid",
                        "a mode hold is the rule's only step, on a single chord pressed without repeat or pass-through"
                            .into(),
                    ));
                }
                // Step 73: nothing switches to a disabled mode.
                if a.targets_disabled(modes) {
                    return Err((
                        "mode_disabled",
                        format!("step {i}: the mode it switches to is disabled"),
                    ));
                }
                mode = Some(a);
            }
            "type_text" | "send_keys" => run = Some(compile_run(s, ty)?),
            "remap" => {
                if steps.len() != 1 || trigger.is_sequence() || on != On::Press || repeat {
                    return Err((
                        "step_invalid",
                        "a remap is the rule's only step, on a single chord pressed without repeat"
                            .into(),
                    ));
                }
                let out = |field: &str| -> Result<Option<Output>, (&'static str, String)> {
                    s.get(field)
                        .and_then(Value::as_str)
                        .map(|t| Output::parse(t).map_err(|e| ("step_invalid", e)))
                        .transpose()
                };
                let to = out("to")?.ok_or(("step_invalid", "a remap needs `to`".to_string()))?;
                let tap = out("tap")?;
                if tap.is_some() && pass_through {
                    return Err((
                        "step_invalid",
                        "a dual-role remap (`tap`) cannot pass the original key through".into(),
                    ));
                }
                let tap_ms = s.get("tap_ms").and_then(Value::as_u64).unwrap_or(200);
                remap = Some(RemapSpec {
                    to,
                    tap,
                    tap_ms: u32::try_from(tap_ms).unwrap_or(200).clamp(100, 1000),
                });
            }
            _ => {
                return Err((
                    "step_unsupported",
                    format!("`{ty}` steps are not implemented in this version yet"),
                ));
            }
        }
    }
    // Remaps and blocks are wildcards: a plain trigger matches with modifiers held (§3.2).
    let plain = trigger.single().is_some_and(|c| c.mods.is_empty());
    let (action, widen, injects) = match (remap, run, mode) {
        // A mode switch injects nothing and matches exactly (Step 72).
        (_, _, Some(m)) => (Action::Mode(m), false, false),
        // Mouse steps inject; they match exactly (no widening, §3.2).
        _ if !mouse.is_empty() => (
            Action::Mouse(Arc::new(crate::mouse::MousePlan {
                steps: mouse,
                stop_on_error: r.get("on_error").and_then(Value::as_str) != Some("continue"),
            })),
            false,
            true,
        ),
        _ if held.is_some() => match held {
            Some(crate::mouse::Compiled::Hold { button, at }) => (
                Action::MouseHold {
                    spec: RemapSpec {
                        to: Output::button(button),
                        tap: None,
                        tap_ms: 200,
                    },
                    at,
                },
                false,
                true,
            ),
            Some(crate::mouse::Compiled::Motion(m)) => (Action::Motion(m), false, true),
            _ => return Err(("step_invalid", "not a held mouse step".into())),
        },
        // Launches inject nothing: they still run while an elevated window has focus (§3).
        _ if !targets.is_empty() => (
            Action::Launch(Arc::new(crate::launch::Plan {
                targets,
                stop_on_error: r.get("on_error").and_then(Value::as_str) != Some("continue"),
            })),
            false,
            false,
        ),
        (Some(spec), _, None) => (Action::Remap(spec), plain, true),
        (None, Some(a), None) => (a, false, true),
        (None, None, None) if ops.is_empty() => (Action::Block, plain, false),
        (None, None, None) => (Action::Engine(ops), false, false),
    };
    Ok((
        Binding {
            rule: r["id"].as_str().unwrap_or_default().to_string(),
            trigger,
            on,
            repeat,
            pass_through,
            injected,
            widen,
            injects,
            action,
            hotstring: hotstring.map(Arc::new),
            modes: 0,
            when,
            schedule: None,
        },
        priority,
    ))
}

/// `rules` in status output: counts, codes and rule IDs (never rule content).
pub fn to_json(l: &Loaded) -> Value {
    json!({
        "file": l.state.as_str(),
        "problem": l.state.problem(),
        "revision": l.revision,
        "engine_on_start": if l.engine_on_start { "on" } else { "off" },
        "total": l.total,
        "active": l.bindings.len(),
        "active_rules": l.bindings.iter().map(|b| b.rule.clone()).collect::<Vec<_>>(),
        "disabled": l.disabled,
        "other_modes": l.other_modes,
        "inactive": l.inactive.iter().map(|i| json!({ "rule": i.rule, "code": i.code, "reason": i.reason })).collect::<Vec<_>>(),
    })
}

/// Whether a `mode` step of `rule` switches only to disabled modes (Step 73, `mode_disabled`).
fn switches_to_disabled(rule: &Rule, modes: &ModeSet) -> bool {
    let v = rule.to_value();
    v["steps"].as_array().into_iter().flatten().any(|s| {
        s["type"] == "mode"
            && ModeAction::compile(s, modes).is_ok_and(|a| a.targets_disabled(modes))
    })
}

/// How one rule stands with this file and this version (`rule list`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleState {
    pub profile: String,
    pub id: String,
    pub position: usize,
    /// `active`, `disabled`, `inactive`, `other_mode` or `other_profile`.
    pub state: &'static str,
    pub code: Option<&'static str>,
    pub reason: Option<String>,
}

/// The state of every rule of a usable document, in document order.
pub fn rule_states(l: &store::Loaded) -> Vec<RuleState> {
    let Some(doc) = &l.doc else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (pi, p) in doc.profiles.iter().enumerate() {
        let active_profile = p.id == doc.active_profile;
        let modes = ModeSet::from_profile(p);
        let base = modes.default_mode();
        for (ri, e) in p.rules.iter().enumerate() {
            let problem = l.report.inactive((pi, ri));
            let (state, code, reason) = if e.enabled() != Some(true) {
                ("disabled", None, None)
            } else if let Some(pr) = problem {
                ("inactive", Some(pr.code), Some(pr.message.clone()))
            } else if !active_profile {
                ("other_profile", None, None)
            } else if !p.is_enabled() {
                // Step 73: the whole profile is disabled.
                (
                    "inactive",
                    Some("profile_disabled"),
                    Some(PROFILE_DISABLED.into()),
                )
            } else if e
                .rule()
                .and_then(|r| r.modes.as_ref())
                .and_then(|m| modes.mask(m))
                .is_some_and(|k| modes.enabled_mask(k) == 0)
            {
                (
                    "inactive",
                    Some("mode_disabled"),
                    Some(MODE_DISABLED.into()),
                )
            } else if e.rule().is_some_and(|r| switches_to_disabled(r, &modes)) {
                (
                    "inactive",
                    Some("mode_disabled"),
                    Some("the mode it switches to is disabled".into()),
                )
            } else if e
                .rule()
                .and_then(|r| r.modes.as_ref())
                .is_some_and(|m| modes.mask(m).is_none_or(|k| modes.rank(base, k).is_none()))
            {
                // Works only after a switch to one of its modes (Step 72).
                ("other_mode", None, None)
            } else {
                ("active", None, None)
            };
            out.push(RuleState {
                profile: p.id.clone(),
                id: e.id().unwrap_or_default().to_string(),
                position: ri,
                state,
                code,
                reason,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[allow(clippy::needless_pass_by_value)]
    fn doc(rules: Value) -> Vec<u8> {
        json!({
            "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1,
            "revision": 3, "active_profile": "default",
            "settings": { "engine_on_start": "on", "max_runs_per_second": 5 },
            "profiles": [ { "id": "default", "title": "Everyday", "default_mode": "base",
                "modes": [ { "id": "base", "title": "Base" }, { "id": "nav", "title": "Nav" } ],
                "rules": rules } ]
        })
        .to_string()
        .into_bytes()
    }

    fn engine_rule(id: &str, keys: &str, op: &str) -> Value {
        json!({ "id": id, "title": id, "enabled": true, "trigger": { "keys": keys },
                "steps": [ { "type": "engine", "op": op } ] })
    }

    #[test]
    fn disabled_profiles_and_modes_keep_their_rules_from_running() {
        // Step 73: per-scope enable/disable (`profile_disabled`, `mode_disabled`).
        let rules = json!([
            engine_rule("everywhere", "F13", "pause"),
            { "id": "nav-only", "title": "n", "enabled": true, "modes": ["nav"],
              "trigger": { "keys": "F14" }, "steps": [ { "type": "engine", "op": "pause" } ] },
            { "id": "both", "title": "b", "enabled": true, "modes": ["base", "nav"],
              "trigger": { "keys": "F15" }, "steps": [ { "type": "engine", "op": "pause" } ] },
            { "id": "to-nav", "title": "t", "enabled": true, "trigger": { "keys": "F16" },
              "steps": [ { "type": "mode", "op": "set", "mode": "nav" } ] },
            { "id": "back", "title": "p", "enabled": true, "trigger": { "keys": "F17" },
              "steps": [ { "type": "mode", "op": "default" } ] },
            { "id": "off", "title": "o", "enabled": false, "trigger": { "keys": "F18" },
              "steps": [ { "type": "engine", "op": "pause" } ] }
        ]);
        let with = |profile_enabled: Option<bool>, nav_enabled: Option<bool>| {
            let mut d: Value = serde_json::from_slice(&doc(rules.clone())).unwrap();
            if let Some(e) = profile_enabled {
                d["profiles"][0]["enabled"] = json!(e);
            }
            if let Some(e) = nav_enabled {
                d["profiles"][0]["modes"][1]["enabled"] = json!(e);
            }
            let bytes = d.to_string().into_bytes();
            let l = parse(&bytes);
            let states = rule_states(&store::interpret(&bytes, &Context::default(), MIGRATIONS));
            (l, states)
        };
        let code = |l: &Loaded, id: &str| l.inactive.iter().find(|i| i.rule == id).map(|i| i.code);
        // Everything enabled (absent fields mean enabled).
        let (l, _) = with(None, None);
        assert!(l.profile_enabled);
        assert_eq!(l.bindings.len(), 5);
        // A disabled mode: its own rules and switches to it are inactive; a rule that also works
        // elsewhere keeps working there; the profile's other rules are untouched.
        let (l, states) = with(None, Some(false));
        assert_eq!(code(&l, "nav-only"), Some("mode_disabled"));
        assert_eq!(code(&l, "to-nav"), Some("mode_disabled"));
        let both = l.bindings.iter().find(|b| b.rule == "both").unwrap();
        assert_eq!(both.modes, 0b01, "only in base now");
        assert!(l.bindings.iter().any(|b| b.rule == "back"));
        assert!(!l.modes.is_enabled(l.modes.index("nav").unwrap()));
        let st = |id: &str| states.iter().find(|s| s.id == id).unwrap();
        assert_eq!(
            (st("nav-only").state, st("nav-only").code),
            ("inactive", Some("mode_disabled"))
        );
        assert_eq!(
            (st("to-nav").state, st("to-nav").code),
            ("inactive", Some("mode_disabled"))
        );
        assert_eq!(st("both").state, "active");
        // A disabled profile: no rule runs; every rule keeps its own `enabled`.
        let (l, states) = with(Some(false), None);
        assert!(!l.profile_enabled);
        assert!(l.bindings.is_empty());
        assert_eq!(
            l.disabled, 1,
            "the disabled rule stays disabled, not inactive"
        );
        assert_eq!(l.inactive.len(), 5);
        assert!(l.inactive.iter().all(|i| i.code == "profile_disabled"));
        assert!(
            states
                .iter()
                .filter(|s| s.id != "off")
                .all(|s| s.code == Some("profile_disabled"))
        );
        // `enabled: true` written out is the same as absent.
        let (l, _) = with(Some(true), Some(true));
        assert_eq!(l.bindings.len(), 5);
    }

    #[test]
    fn the_scopes_example_loads_as_documented() {
        let bytes =
            std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
                "../../docs/contracts/examples/ddalgak-rules-v1/16-enable-disable-scopes.json",
            ))
            .unwrap();
        let l = parse(&bytes);
        assert_eq!(l.state, FileState::Loaded);
        assert_eq!(l.floating_indicator, Some(FloatingIndicator::Paused));
        assert!(l.profile_enabled, "work is enabled; travel is not active");
        let codes: Vec<(&str, &str)> = l
            .inactive
            .iter()
            .map(|i| (i.rule.as_str(), i.code))
            .collect();
        assert_eq!(
            codes,
            [
                ("game-toggle", "mode_disabled"),
                ("game-no-win", "mode_disabled")
            ]
        );
        assert_eq!(l.disabled, 1);
        assert_eq!(l.bindings.len(), 1);
        // Switched to the disabled travel profile, nothing runs.
        let mut d: Value = serde_json::from_slice(&bytes).unwrap();
        d["active_profile"] = json!("travel");
        let l = parse(d.to_string().as_bytes());
        assert!(!l.profile_enabled && l.bindings.is_empty());
        assert_eq!(l.inactive[0].code, "profile_disabled");
    }

    #[test]
    fn the_default_mode_is_never_disabled() {
        let mut d: Value = serde_json::from_slice(&doc(json!([]))).unwrap();
        d["profiles"][0]["modes"][0]["enabled"] = json!(false);
        let bytes = d.to_string().into_bytes();
        let l = parse(&bytes);
        assert!(
            l.modes.is_enabled(l.modes.default_mode()),
            "ignored there (never broader)"
        );
        let s = store::interpret(&bytes, &Context::default(), MIGRATIONS);
        assert!(
            s.report
                .all()
                .any(|p| p.code == "mode_invalid" && p.message.contains("enabled: false")),
            "and reported"
        );
    }

    /// A launch rule with `launch-programs` consent for exactly its targets.
    #[allow(clippy::needless_pass_by_value)]
    fn launch_rule(id: &str, keys: &str, steps: Value) -> Value {
        let mut r = json!({ "id": id, "title": id, "enabled": true, "trigger": { "keys": keys },
                            "steps": steps });
        let fp = crate::validate::launch_fingerprint(&r);
        r["consent"] = json!({ "launch-programs": {
            "granted_at": "2026-11-04T09:00:00Z", "via": "cli", "fingerprint": fp } });
        r
    }

    #[test]
    fn launch_rules_compile_and_changed_or_unconsented_targets_stay_inactive() {
        use crate::launch::{Show, Target};
        // The target changed after consent (Step 65's fingerprint check): inactive.
        let mut stale = launch_rule(
            "stale",
            "F15",
            json!([{ "type": "open_url", "url": "https://example.com/" }]),
        );
        stale["steps"][0]["url"] = json!("https://other.example/");
        // An environment value changed after consent: inactive as well.
        let mut env_changed = launch_rule(
            "env-changed",
            "F20",
            json!([{ "type": "open_program", "path": r"C:\Tools\t.exe",
                     "env": [{ "name": "MODE", "value": "safe" }] }]),
        );
        env_changed["steps"][0]["env"][0]["value"] = json!("unsafe");
        // An imported rule: disabled and without consent, it never runs.
        let imported = json!({ "id": "imported", "title": "imported", "enabled": false,
            "trigger": { "keys": "F16" },
            "steps": [{ "type": "open_program", "path": r"D:\Tools\tool.exe" }] });
        let l = parse(&doc(json!([
            launch_rule(
                "notepad",
                "F13",
                json!([{ "type": "open_program", "path": r"%SystemRoot%\System32\notepad.exe",
                         "args": ["a b"], "window": "minimized", "if_running": "activate",
                         "env": [{ "name": "K", "value": "v" }] }])
            ),
            launch_rule(
                "two",
                "F14",
                json!([{ "type": "open_url", "url": "https://github.com/" },
                       { "type": "open_program", "app_id": "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App" }])
            ),
            stale,
            imported,
            launch_rule(
                "mixed",
                "F17",
                json!([{ "type": "open_url", "url": "https://github.com/" }, { "type": "delay", "ms": 100 }])
            ),
            launch_rule(
                "colon",
                "F18",
                json!([{ "type": "open_program", "path": r"C:\notes.txt:hidden.exe" }])
            ),
            launch_rule(
                "dup-env",
                "F19",
                json!([{ "type": "open_program", "path": r"C:\x.exe",
                         "env": [{ "name": "A", "value": "1" }, { "name": "a", "value": "2" }] }])
            ),
            env_changed,
        ])));
        let active: Vec<&str> = l.bindings.iter().map(|b| b.rule.as_str()).collect();
        // Step 78: a launch with a delay is a macro.
        assert_eq!(active, ["notepad", "two", "mixed"]);
        assert!(matches!(l.bindings[2].action, Action::Sequence(_)));
        let Action::Launch(p) = &l.bindings[0].action else {
            panic!("{:?}", l.bindings[0].action)
        };
        let Target::Program(s) = &p.targets[0] else {
            panic!("{p:?}")
        };
        assert_eq!((s.show, s.activate), (Show::Minimized, true));
        assert_eq!(s.env, [("K".to_string(), "v".to_string())]);
        assert!(
            !l.bindings[0].injects && !l.bindings[0].widen,
            "launches inject nothing"
        );
        let Action::Launch(p) = &l.bindings[1].action else {
            panic!()
        };
        assert_eq!(p.targets.len(), 2);
        assert!(p.stop_on_error);
        let codes: Vec<(&str, &str)> = l
            .inactive
            .iter()
            .map(|i| (i.rule.as_str(), i.code))
            .collect();
        assert_eq!(
            codes,
            [
                ("stale", "consent_stale"),
                ("colon", "step_invalid"),
                ("dup-env", "step_invalid"),
                ("env-changed", "consent_stale"),
            ]
        );
        assert_eq!(l.disabled, 1);
        for i in &l.inactive {
            assert!(
                !i.reason.contains("notes.txt") && !i.reason.contains("other.example"),
                "reasons never quote targets: {i:?}"
            );
        }
    }

    #[test]
    fn missing_invalid_and_newer_files() {
        let missing = load(Path::new("Z:/does/not/exist/rules.json"));
        assert_eq!(missing.state, FileState::Missing);
        assert!(missing.state.usable());
        assert_eq!(
            (missing.profile.as_str(), missing.mode.as_str()),
            ("default", "base")
        );
        assert_eq!(parse(b"{not json").state.as_str(), "invalid");
        assert!(!parse(b"[]").state.usable());
        let newer = json!({"schema": "ddalgak.rules", "schema_version": 2}).to_string();
        assert_eq!(parse(newer.as_bytes()).state, FileState::NewerVersion(2));
        let wrong = json!({"schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1,
            "active_profile": "x",
            "profiles": [{"id": "default", "title": "D", "default_mode": "base",
                          "modes": [{"id": "base", "title": "B"}], "rules": []}]})
        .to_string();
        let l = parse(wrong.as_bytes());
        assert!(
            matches!(&l.state, FileState::Invalid(w) if w.contains("active_profile")),
            "{:?}",
            l.state
        );
        // A schema violation outside the rules (no modes) is a whole-file problem.
        let no_modes = json!({"schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1,
            "active_profile": "default",
            "profiles": [{"id": "default", "title": "D", "default_mode": "base", "modes": [], "rules": []}]})
        .to_string();
        assert!(!parse(no_modes.as_bytes()).state.usable());
    }

    #[test]
    fn engine_control_rules_load_and_others_stay_inactive_with_reasons() {
        let l = parse(&doc(json!([
            engine_rule("pause-key", "Ctrl+Alt+Shift+F13", "toggle_pause"),
            { "id": "caps", "title": "c", "enabled": true, "trigger": { "keys": "CapsLock" },
              "steps": [ { "type": "remap", "to": "LCtrl" } ],
              "consent": { "input-injection": { "granted_at": "2026-11-04T09:00:00Z", "via": "gui" } } },
            { "id": "future", "title": "f", "enabled": true, "trigger": { "keys": "F14" },
              "steps": [ { "type": "teleport" } ] },
            { "id": "cond", "title": "c", "enabled": true, "trigger": { "keys": "F15" },
              "when": { "apps": [ { "exe": "Code.exe" } ], "battery": "charging" },
              "steps": [ { "type": "engine", "op": "pause" } ] },
            { "id": "newer-field", "title": "n", "enabled": true, "trigger": { "keys": "F16", "only_on_tuesdays": true },
              "steps": [ { "type": "engine", "op": "pause" } ] },
            { "id": "off", "title": "o", "enabled": false, "trigger": { "keys": "F17" },
              "steps": [ { "type": "engine", "op": "pause" } ] },
            { "id": "reserved", "title": "r", "enabled": true, "trigger": { "keys": "Win+L" },
              "steps": [ { "type": "engine", "op": "pause" } ] },
            { "id": "nav-only", "title": "n", "enabled": true, "modes": ["nav"], "trigger": { "keys": "F18" },
              "steps": [ { "type": "engine", "op": "pause" } ] },
            engine_rule("dup", "Ctrl+Alt+Shift+F13", "toggle_pause"),
            engine_rule("pause-key", "F19", "pause")
        ])));
        assert_eq!(l.state, FileState::Loaded);
        assert_eq!(l.revision, Some(3));
        assert!(l.engine_on_start);
        assert_eq!(l.max_runs_per_second, 5);
        assert_eq!(l.total, 10);
        assert_eq!(l.disabled, 1);
        assert_eq!(
            l.other_modes, 1,
            "nav-only works only after a switch to nav"
        );
        assert_eq!(l.bindings.len(), 3);
        assert_eq!(l.bindings[0].rule, "pause-key");
        assert_eq!(
            l.bindings[0].action,
            Action::Engine(vec![EngineOp::TogglePause])
        );
        assert_eq!(l.bindings[2].rule, "nav-only");
        assert_eq!(l.bindings[2].modes, 0b10, "limited to the second mode");
        assert_eq!(
            l.table().order(0).len(),
            2,
            "the base mode's list leaves nav-only out"
        );
        assert_eq!(l.bindings[1].rule, "caps");
        assert!(matches!(l.bindings[1].action, Action::Remap(_)));
        assert!(l.bindings[1].widen && l.bindings[1].injects);
        let codes: Vec<(&str, &str)> = l
            .inactive
            .iter()
            .map(|i| (i.rule.as_str(), i.code))
            .collect();
        assert_eq!(
            codes,
            [
                ("future", "step_unsupported"),
                ("cond", "condition_unsupported"),
                ("newer-field", "rule_unsupported"),
                ("reserved", "trigger_reserved"),
                ("dup", "duplicate_trigger"),
                ("pause-key", "duplicate_rule_id"),
            ]
        );
        assert!(
            l.inactive[0].reason.contains("newer"),
            "{:?}",
            l.inactive[0]
        );
    }

    #[test]
    fn mouse_rules_compile_and_mixed_or_misplaced_holds_stay_inactive() {
        use crate::inject::Button;
        use crate::mouse::{MotionKind, MouseStep, Unit};
        let r = |id: &str, keys: &str, steps: Value| {
            json!({ "id": id, "title": id, "enabled": true, "trigger": { "keys": keys },
                    "steps": steps,
                    "consent": { "input-injection": { "granted_at": "2026-11-04T09:00:00Z", "via": "cli" } } })
        };
        let mut repeat_hold = r(
            "repeat-hold",
            "F20",
            json!([{"type":"mouse_click","button":"left","op":"hold"}]),
        );
        repeat_hold["trigger"]["repeat"] = json!(true);
        let l = parse(&doc(json!([
            r(
                "plan",
                "F13",
                json!([{"type":"mouse_move","mode":"absolute","to":{"x":1,"y":2}},
                       {"type":"mouse_click","button":"left","op":"double_click"}])
            ),
            r(
                "drag",
                "Ctrl+F14",
                json!([{"type":"mouse_click","button":"x2","op":"hold"}])
            ),
            r(
                "glide",
                "F15",
                json!([{"type":"mouse_move","mode":"relative","dx":1,"dy":0,"while_held":{"speed":600}}])
            ),
            r(
                "cells",
                "F16",
                json!([{"type":"mouse_move","mode":"relative","dx":1,"dy":0,"unit":"grid",
                        "grid":{"columns":8,"rows":8},"while_held":{"repeat_ms":100}}])
            ),
            r(
                "wheel",
                "F17",
                json!([{"type":"mouse_scroll","direction":"right","notches":3}])
            ),
            r(
                "mixed",
                "F18",
                json!([{"type":"mouse_scroll","direction":"down"},{"type":"engine","op":"pause"}])
            ),
            r(
                "grid-glide",
                "F19",
                json!([{"type":"mouse_move","mode":"relative","dx":1,"dy":0,"unit":"grid",
                        "grid":{"columns":8,"rows":8},"while_held":{"speed":600}}])
            ),
            repeat_hold,
        ])));
        let active: Vec<&str> = l.bindings.iter().map(|b| b.rule.as_str()).collect();
        // Step 78: a mouse step with an engine step is a macro.
        assert_eq!(active, ["plan", "drag", "glide", "cells", "wheel", "mixed"]);
        assert!(matches!(l.bindings[5].action, Action::Sequence(_)));
        assert!(l.bindings.iter().all(|b| b.injects && !b.widen));
        let Action::Mouse(p) = &l.bindings[0].action else {
            panic!("{:?}", l.bindings[0].action)
        };
        assert_eq!(p.steps.len(), 2);
        assert!(p.stop_on_error);
        assert!(matches!(
            p.steps[1],
            MouseStep::Click {
                button: Button::Left,
                double: true,
                at: None
            }
        ));
        let Action::MouseHold { spec, at: None } = &l.bindings[1].action else {
            panic!("{:?}", l.bindings[1].action)
        };
        assert_eq!(spec.to, Output::button(Button::X2));
        assert!(matches!(
            l.bindings[2].action,
            Action::Motion(m) if matches!(m.kind, MotionKind::Continuous { speed: 600, acceleration: 0 })
        ));
        assert!(matches!(
            l.bindings[3].action,
            Action::Motion(m) if matches!(m.unit, Unit::Grid(_)) && matches!(m.kind, MotionKind::Stepped { repeat_ms: 100, .. })
        ));
        // The compiled table knows which bindings hold a button or a motion.
        let t = crate::table::Table::compile(l.bindings.clone(), 20);
        assert!(t.remap(1).is_some() && t.remap(2).is_none());
        let codes: Vec<(&str, &str)> = l
            .inactive
            .iter()
            .map(|i| (i.rule.as_str(), i.code))
            .collect();
        assert_eq!(
            codes,
            [
                ("grid-glide", "step_invalid"),
                ("repeat-hold", "step_invalid"),
            ]
        );
    }

    /// Step 79: window, clipboard, notification and tool steps load as macro stages; a schedule
    /// loads without keys; a scheduled rule that would inject is refused.
    #[test]
    fn step79_steps_and_schedules_load() {
        let mut sched = engine_rule("tick", "F13", "pause");
        sched["trigger"] = json!({"schedule": {"every_minutes": 5}});
        sched["steps"] = json!([{"type": "notify", "title": "Stretch"}]);
        let mut typing = sched.clone();
        typing["id"] = json!("typing");
        typing["steps"] = json!([{"type": "type_text", "text": "x"}]);
        let mut win = engine_rule("win", "F14", "pause");
        win["steps"] =
            json!([{"type": "window", "op": "move", "x": 0, "y": 0, "width": 800, "height": 600}]);
        let mut clip = engine_rule("clip", "F15", "pause");
        clip["steps"] = json!([{"type": "clipboard", "op": "transform", "to": "plain"}]);
        let mut tool = engine_rule("tool", "F16", "pause");
        tool["steps"] = json!([{"type": "tool", "capability": "pin.list"}]);
        let mut bad = engine_rule("bad", "F17", "pause");
        bad["steps"] = json!([{"type": "window", "op": "activate"}]);
        // Without consent, window and clipboard steps stay inactive.
        let mut no_consent = win.clone();
        no_consent["id"] = json!("no-consent");
        let grant = json!({"granted_at": "2026-11-04T09:00:00Z", "via": "gui"});
        win["consent"] = json!({"window-control": grant});
        clip["consent"] = json!({"clipboard": grant});
        let l = parse(&doc(json!([
            sched, typing, win, clip, tool, bad, no_consent
        ])));
        let active: Vec<&str> = l.bindings.iter().map(|b| b.rule.as_str()).collect();
        assert_eq!(active, ["tick", "win", "clip", "tool"]);
        assert!(l.bindings[0].schedule.is_some() && l.bindings[0].trigger.steps().is_empty());
        assert!(
            l.bindings
                .iter()
                .all(|b| matches!(b.action, Action::Sequence(_)) && !b.injects)
        );
        let codes: Vec<(&str, &str)> = l
            .inactive
            .iter()
            .map(|i| (i.rule.as_str(), i.code))
            .collect();
        assert_eq!(
            codes,
            [
                ("typing", "step_invalid"),
                ("bad", "step_invalid"),
                ("no-consent", "consent_missing")
            ]
        );
    }

    /// Step 77 (§7.1 step 6): inside a mode rank, rules with `when.apps` come first, then rules
    /// with `when.window_title`, then priority and file order; a mode-scoped rule still beats an
    /// app rule without modes.
    #[test]
    fn conditions_rank_after_the_mode_and_before_priority() {
        let mut title = engine_rule("title", "F13", "pause");
        title["when"] = json!({"window_title": {"contains": "kim"}});
        let mut app = engine_rule("app", "F13", "pause");
        app["when"] = json!({"apps": [{"exe": "Code.exe"}]});
        app["priority"] = json!(-100);
        let mut plain = engine_rule("plain", "F13", "pause");
        plain["priority"] = json!(100);
        let mut scoped = engine_rule("scoped", "F13", "pause");
        scoped["modes"] = json!(["base"]);
        let l = parse(&doc(json!([plain, title, app, scoped])));
        assert!(l.inactive.is_empty(), "{:?}", l.inactive);
        let t = l.table();
        let base: Vec<&str> = t
            .order(0)
            .iter()
            .map(|i| t.binding(usize::from(*i)).unwrap().rule.as_str())
            .collect();
        assert_eq!(base, ["scoped", "app", "title", "plain"]);
        assert!(t.compiled[usize::from(t.order(0)[1])].cond);
    }

    #[test]
    fn precedence_is_mode_scope_then_priority_then_order() {
        let mut low = engine_rule("low", "F13", "pause");
        low["priority"] = json!(-5);
        let mut high = engine_rule("high", "F14", "pause");
        high["priority"] = json!(10);
        let mut scoped = engine_rule("scoped", "F15", "pause");
        scoped["modes"] = json!(["base"]);
        let l = parse(&doc(json!([
            low,
            engine_rule("plain", "F16", "pause"),
            high,
            scoped
        ])));
        // The loader keeps priority and file order; the table ranks per mode, mode-scoped first.
        let order: Vec<&str> = l.bindings.iter().map(|b| b.rule.as_str()).collect();
        assert_eq!(order, ["high", "plain", "scoped", "low"]);
        let t = l.table();
        let base: Vec<&str> = t
            .order(0)
            .iter()
            .map(|i| t.binding(usize::from(*i)).unwrap().rule.as_str())
            .collect();
        assert_eq!(base, ["scoped", "high", "plain", "low"]);
        let nav: Vec<&str> = t
            .order(1)
            .iter()
            .map(|i| t.binding(usize::from(*i)).unwrap().rule.as_str())
            .collect();
        assert_eq!(
            nav,
            ["high", "plain", "low"],
            "a base-only rule is not in nav"
        );
    }

    #[test]
    fn mode_rules_compile_with_their_masks_and_combinations_wait_for_step_78() {
        let l = load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/contracts/examples/ddalgak-rules-v1/15-mode-layers.json"),
        );
        assert_eq!((l.errors, l.inactive.len()), (0, 0), "{:?}", l.inactive);
        assert_eq!(l.modes.len(), 6);
        let b = |id: &str| l.bindings.iter().find(|b| b.rule == id).unwrap();
        assert!(matches!(&b("hold-nav").action, Action::Mode(a) if a.holds()));
        assert!(
            !b("hold-nav").injects && !b("hold-nav").widen,
            "modes inject nothing"
        );
        assert_eq!(b("hold-symbols").modes, 1 << l.modes.index("nav").unwrap());
        assert!(matches!(&b("cycle").action, Action::Mode(ModeAction::Cycle(m)) if m.len() == 2));
        // Rules that need a switch first: hold-symbols, home, nav-h, symbols-h, accent-e, numpad-j.
        assert_eq!(l.other_modes, 6);
        // The table ranks the nearest layer first: in Symbols, H types `#`, in Navigation it is
        // Left; in Base, H is neither.
        let t = l.table();
        let first_h = |m: &str| {
            t.order(l.modes.index(m).unwrap())
                .iter()
                .map(|i| t.binding(usize::from(*i)).unwrap().rule.as_str())
                .find(|r| r.ends_with("-h"))
        };
        assert_eq!(first_h("symbols"), Some("symbols-h"));
        assert_eq!(first_h("nav"), Some("nav-h"));
        assert_eq!(first_h("base"), None);
        assert_eq!(
            first_h("gaming"),
            None,
            "an exclusive mode keeps no other mode's rules"
        );
        // Step 78: a mode step with other steps is a macro; an unknown target is inactive.
        let l = parse(&doc(json!([
            { "id": "two", "title": "two", "enabled": true, "trigger": { "keys": "F13" },
              "steps": [ { "type": "mode", "op": "set", "mode": "nav" },
                         { "type": "engine", "op": "pause" } ] },
            { "id": "ghost", "title": "ghost", "enabled": true, "trigger": { "keys": "F14" },
              "steps": [ { "type": "mode", "op": "set", "mode": "ghost" } ] },
            { "id": "ok", "title": "ok", "enabled": true, "trigger": { "keys": "F15" },
              "steps": [ { "type": "mode", "op": "toggle", "mode": "nav" } ] }
        ])));
        let codes: Vec<(&str, &str)> = l
            .inactive
            .iter()
            .map(|i| (i.rule.as_str(), i.code))
            .collect();
        assert_eq!(codes, [("ghost", "unknown_mode")]);
        assert_eq!(l.bindings.len(), 2);
        let two = l.bindings.iter().find(|b| b.rule == "two").unwrap();
        assert!(two.action.mode_at(Some(0)).is_some() && two.action.mode_at(Some(1)).is_none());
    }

    #[test]
    fn trigger_options_are_checked() {
        let mut r = engine_rule("tap", "RAlt", "toggle_pause");
        r["trigger"] = json!({ "keys": "RAlt", "on": "release" });
        let mut bad_mod = engine_rule("badmod", "RCtrl", "pause");
        bad_mod["trigger"] = json!({ "keys": "RCtrl" });
        let mut hold = engine_rule("hold", "F13", "pause");
        hold["trigger"] =
            json!({ "keys": "F13", "on": "hold", "hold_ms": 800, "injected": "accept" });
        let mut bad_hold = engine_rule("badhold", "F14", "pause");
        bad_hold["trigger"] = json!({ "keys": "F14", "on": "hold", "hold_ms": 50 });
        let mut seq_release = engine_rule("seqrel", "F15 F16", "pause");
        seq_release["trigger"] = json!({ "keys": "F15 F16", "on": "release" });
        let l = parse(&doc(json!([r, bad_mod, hold, bad_hold, seq_release])));
        let active: Vec<&str> = l.bindings.iter().map(|b| b.rule.as_str()).collect();
        assert_eq!(active, ["tap", "hold"]);
        assert_eq!(l.bindings[1].on, On::Hold(800));
        assert_eq!(l.bindings[1].injected, Injected::Accept);
        assert!(
            l.inactive.iter().all(|i| i.code == "trigger_invalid"),
            "{:?}",
            l.inactive
        );
    }

    #[test]
    fn the_repository_examples_load_without_running_anything_unsupported() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/contracts/examples/ddalgak-rules-v1");
        let mut seen = 0;
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "json") {
                continue;
            }
            seen += 1;
            let l = load(&p);
            assert!(
                l.state.usable() || matches!(l.state, FileState::NewerVersion(_)),
                "{}: {:?}",
                p.display(),
                l.state
            );
            assert_eq!(l.errors, 0, "{}", p.display());
            for b in &l.bindings {
                assert!(
                    matches!(
                        b.action,
                        Action::Engine(_)
                            | Action::Remap(_)
                            | Action::Block
                            | Action::Type(_)
                            | Action::Keys(_)
                            | Action::Launch(_)
                            | Action::Mode(_)
                            | Action::Mouse(_)
                            | Action::MouseHold { .. }
                            | Action::Motion(_)
                            | Action::Sequence(_)
                    ),
                    "{}: {}",
                    p.display(),
                    b.rule
                );
            }
        }
        assert!(seen >= 10);
    }

    #[test]
    fn rule_states_name_every_rule() {
        let mut nav = engine_rule("nav-only", "F18", "pause");
        nav["modes"] = json!(["nav"]);
        let mut off = engine_rule("off", "F17", "pause");
        off["enabled"] = json!(false);
        let bytes = doc(json!([
            engine_rule("ra", "F13", "pause"),
            nav,
            off,
            engine_rule("rb", "F13", "pause")
        ]));
        let l = store::interpret(&bytes, &Context::default(), MIGRATIONS);
        let states = rule_states(&l);
        let s: Vec<(&str, &str)> = states.iter().map(|r| (r.id.as_str(), r.state)).collect();
        assert_eq!(
            s,
            [
                ("ra", "active"),
                ("nav-only", "other_mode"),
                ("off", "disabled"),
                ("rb", "inactive")
            ]
        );
    }

    #[allow(clippy::needless_pass_by_value)]
    fn typing_rule(id: &str, trigger: Value, steps: Value) -> Value {
        json!({ "id": id, "title": id, "enabled": true, "trigger": trigger, "steps": steps,
                "consent": { "input-injection": { "granted_at": "2026-11-04T09:00:00Z", "via": "cli" },
                             "clipboard": { "granted_at": "2026-11-04T09:00:00Z", "via": "cli" } } })
    }

    #[test]
    fn typing_rules_and_hotstrings_compile_and_combinations_wait_for_step_78() {
        let l = parse(&doc(json!([
            typing_rule(
                "sign",
                json!({ "keys": "Ctrl+Shift+F2" }),
                json!([{ "type": "type_text", "text": "감사합니다\n", "method": "paste", "key_delay_ms": 5 }])
            ),
            typing_rule(
                "btw",
                json!({ "hotstring": { "abbreviation": "btw", "case": "sensitive",
                    "inside_words": true, "end_chars": " .", "omit_end": true, "erase": false } }),
                json!([{ "type": "type_text", "text": "by the way" }])
            ),
            typing_rule(
                "ac",
                json!({ "hotstring": { "abbreviation": ";ac", "immediate": true } }),
                json!([{ "type": "send_keys", "keys": "Ctrl+A Ctrl+C", "step_delay_ms": 40 }])
            ),
            typing_rule(
                "two",
                json!({ "keys": "F14" }),
                json!([{ "type": "type_text", "text": "a" }, { "type": "send_keys", "keys": "Enter" }])
            ),
            typing_rule(
                "hs-engine",
                json!({ "hotstring": { "abbreviation": ";p" } }),
                json!([{ "type": "engine", "op": "pause" }])
            ),
            typing_rule(
                "future-hs",
                json!({ "hotstring": { "abbreviation": ";f", "fuzzy": true } }),
                json!([{ "type": "type_text", "text": "f" }])
            )
        ])));
        let active: Vec<&str> = l.bindings.iter().map(|b| b.rule.as_str()).collect();
        // Step 78: two typing steps are a macro.
        assert_eq!(active, ["sign", "btw", "ac", "two"]);
        let Action::Type(t) = &l.bindings[0].action else {
            panic!("{:?}", l.bindings[0].action)
        };
        assert!(t.paste && t.key_delay_ms == 5 && !t.variables);
        assert!(l.bindings[0].injects && !l.bindings[0].widen && l.bindings[0].hotstring.is_none());
        let h = l.bindings[1].hotstring.as_ref().unwrap();
        assert_eq!(h.abbreviation, ['b', 't', 'w']);
        assert_eq!(h.case, hotstring::Case::Sensitive);
        assert!(h.inside_words && h.omit_end && !h.erase && !h.immediate);
        assert_eq!(h.end_chars, [' ', '.']);
        assert!(l.bindings[1].trigger.steps().is_empty());
        let Action::Keys(k) = &l.bindings[2].action else {
            panic!()
        };
        assert_eq!((k.chords.len(), k.step_delay_ms), (2, 40));
        assert!(l.bindings[2].hotstring.as_ref().unwrap().immediate);
        let codes: Vec<(&str, &str)> = l
            .inactive
            .iter()
            .map(|i| (i.rule.as_str(), i.code))
            .collect();
        assert_eq!(
            codes,
            [
                ("hs-engine", "step_unsupported"),
                ("future-hs", "rule_unsupported")
            ]
        );
        assert!(
            l.inactive[0].reason.contains("macro needs a key trigger"),
            "{:?}",
            l.inactive[0]
        );
        // The compiled table knows its hotstrings and bounds the core's buffer by them.
        let table = crate::table::Table::compile(l.bindings.clone(), 20);
        assert!(table.has_hotstrings());
        assert_eq!(table.longest_hotstring, 3);
    }

    #[test]
    fn duplicate_hotstrings_and_slow_typing_are_semantic_errors() {
        let tt = |t: &str| json!([{ "type": "type_text", "text": t }]);
        let mut sensitive = typing_rule(
            "btw-exact",
            json!({ "hotstring": { "abbreviation": "BTW", "case": "sensitive" } }),
            tt("BY THE WAY"),
        );
        sensitive["priority"] = json!(0);
        let l = parse(&doc(json!([
            typing_rule(
                "btw",
                json!({ "hotstring": { "abbreviation": "btw" } }),
                tt("by the way")
            ),
            typing_rule(
                "btw2",
                json!({ "hotstring": { "abbreviation": "Btw" } }),
                tt("x")
            ),
            sensitive,
            typing_rule(
                "slow",
                json!({ "keys": "F13" }),
                json!([{ "type": "type_text", "text": "y".repeat(400), "key_delay_ms": 200 }])
            )
        ])));
        let codes: Vec<(&str, &str)> = l
            .inactive
            .iter()
            .map(|i| (i.rule.as_str(), i.code))
            .collect();
        assert_eq!(
            codes,
            [("btw2", "duplicate_trigger"), ("slow", "run_too_long")],
            "a case-sensitive abbreviation is its own trigger"
        );
    }

    #[test]
    fn loading_and_validating_1000_rules_is_fast_enough() {
        // 1000 distinct triggers (two-chord sequences over F13–F24 with modifiers), as a large
        // but sensible file would have.
        let rules: Vec<Value> = (0..1000)
            .map(|i| {
                let keys = format!(
                    "{}F{} F{}",
                    [
                        "",
                        "Ctrl+",
                        "Alt+",
                        "Shift+",
                        "Ctrl+Shift+",
                        "Ctrl+Alt+",
                        "Alt+Shift+"
                    ][i % 7],
                    13 + (i / 7) % 12,
                    13 + (i / 84) % 12
                );
                engine_rule(&format!("r-{i}"), &keys, "pause")
            })
            .collect();
        let bytes = doc(Value::Array(rules));
        let t = std::time::Instant::now();
        let l = parse(&bytes);
        let took = t.elapsed();
        assert_eq!(l.total, 1000);
        eprintln!(
            "load + validate 1000 rules: {} ms ({} active)",
            took.as_millis(),
            l.bindings.len()
        );
        // DDALGAK-NFR-004 (< 100 ms with the hook) is measured on release builds; a debug build
        // is several times slower.
        assert!(took < std::time::Duration::from_secs(20), "{took:?}");
    }

    /// #102: a `run_command` step loads with `run-commands` consent bound to its exact commands;
    /// without it, or after the command changed, the rule stays inactive; relative programs,
    /// too many arguments and bad timeouts are invalid.
    #[test]
    fn run_command_needs_consent_for_the_exact_command() {
        let mut cmd = engine_rule("cmd", "F13", "pause");
        cmd["steps"] = json!([{"type": "run_command", "program": "C:\\Tools\\backup.exe",
            "args": ["--all"], "timeout_ms": 5000}]);
        let fp =
            crate::validate::fingerprint_for(crate::model::Permission::RunCommands, &cmd).unwrap();
        let mut ok = cmd.clone();
        ok["consent"] = json!({"run-commands": {"granted_at": "2026-11-12T09:00:00Z", "via": "cli",
            "fingerprint": fp}});
        let mut stale = ok.clone();
        stale["id"] = json!("stale");
        stale["trigger"] = json!({"keys": "F14"});
        stale["steps"][0]["args"] = json!(["--everything"]);
        let mut none = cmd.clone();
        none["id"] = json!("none");
        none["trigger"] = json!({"keys": "F15"});
        let mut rel = ok.clone();
        rel["id"] = json!("rel");
        rel["trigger"] = json!({"keys": "F16"});
        rel["steps"][0]["program"] = json!("backup.exe");
        let mut slow = ok.clone();
        slow["id"] = json!("slow");
        slow["trigger"] = json!({"keys": "F17"});
        slow["steps"][0]["timeout_ms"] = json!(50);
        let l = parse(&doc(json!([ok, stale, none, rel, slow])));
        let active: Vec<&str> = l.bindings.iter().map(|b| b.rule.as_str()).collect();
        assert_eq!(active, ["cmd"]);
        assert!(!l.bindings[0].injects);
        let codes: Vec<(&str, &str)> = l
            .inactive
            .iter()
            .map(|i| (i.rule.as_str(), i.code))
            .collect();
        assert!(
            codes.contains(&("stale", "consent_stale"))
                || codes.contains(&("stale", "consent_missing")),
            "{codes:?}"
        );
        assert!(codes.contains(&("none", "consent_missing")), "{codes:?}");
        assert!(codes.iter().any(|c| c.0 == "rel"), "{codes:?}");
        for bad in [
            json!({"program": "backup.exe"}),
            json!({"program": r"C:\x.exe", "args": vec!["a"; 33]}),
            json!({"program": r"C:\x.exe", "working_dir": "rel"}),
            json!({"program": r"C:\x.exe", "timeout_ms": 700_000}),
        ] {
            assert!(compile_command(&bad).is_err(), "{bad}");
        }
        let c = compile_command(&json!({"program": r"C:\x.exe"})).unwrap();
        assert_eq!((c.args.len(), c.timeout_ms), (0, 60_000));
        assert!(codes.contains(&("slow", "step_invalid")), "{codes:?}");
    }

    /// #102: a timer loads as a stage; a timer for its own rule is refused; the schedule's
    /// `missed: run_once` loads; the validator reports a timer for a rule that does not exist.
    #[test]
    fn timers_load_and_never_restart_their_own_rule() {
        let mut arm = engine_rule("arm", "F13", "pause");
        arm["steps"] = json!([{"type": "timer", "rule": "later", "after_ms": 600_000}]);
        let mut later = engine_rule("later", "F14", "pause");
        later["steps"] = json!([{"type": "notify", "title": "Later"}]);
        let mut own = engine_rule("own", "F15", "pause");
        own["steps"] = json!([{"type": "timer", "rule": "own", "after_ms": 1000}]);
        let mut wake = engine_rule("wake", "F16", "pause");
        wake["trigger"] = json!({"schedule": {"every_minutes": 5, "missed": "run_once"}});
        wake["steps"] = json!([{"type": "notify", "title": "Wake"}]);
        let d = doc(json!([arm, later, own, wake]));
        let l = parse(&d);
        let active: Vec<&str> = l.bindings.iter().map(|b| b.rule.as_str()).collect();
        assert_eq!(active, ["arm", "later", "wake"], "{:?}", l.inactive);
        assert!(l.bindings[2].schedule.as_ref().is_some_and(|s| s.catch_up));
        assert_eq!(l.inactive[0].rule, "own");

        let mut ghost = engine_rule("ghost", "F17", "pause");
        ghost["steps"] = json!([{"type": "timer", "rule": "nobody", "after_ms": 1000}]);
        let report = {
            let v: Value = serde_json::from_slice(&doc(json!([ghost]))).unwrap();
            crate::validate::check(&v, &Context::default())
        };
        assert!(
            report.problems.iter().any(|p| p.code == "step_invalid"),
            "{:?}",
            report.problems
        );
    }

    /// #101: `repeat` and `if` blocks unroll into stages with relative jumps; structure errors,
    /// nesting deeper than 3, unset variables and more than 1000 steps keep the rule inactive.
    #[test]
    fn blocks_unroll_and_bad_blocks_are_refused() {
        let modes = ModeSet::single();
        let note = json!({"type": "notify", "title": "x"});
        let steps = vec![
            json!({"type": "set_var", "name": "n", "value": "0"}),
            json!({"type": "repeat", "times": 3}),
            json!({"type": "set_var", "name": "n", "add": 1}),
            json!({"type": "end"}),
            json!({"type": "if", "when": {"var": {"name": "n", "equals": "3"}}}),
            note.clone(),
            json!({"type": "else"}),
            note.clone(),
            note.clone(),
            json!({"type": "end"}),
        ];
        let st = compile_stages(&steps, &modes).unwrap();
        // set, 3 adds, branch, then (1), skip, else (2).
        assert_eq!(st.len(), 1 + 3 + 1 + 1 + 1 + 2);
        assert!(matches!(&st[4], Stage::Branch(b) if b.skip == 2));
        assert!(matches!(&st[6], Stage::Skip(2)));
        assert_eq!(check_macro(&steps, 2, &modes), Ok(18));

        let err = |steps: Vec<Value>| compile_stages(&steps, &modes).err().map(|e| e.1);
        assert!(
            err(vec![json!({"type": "repeat", "times": 2}), note.clone()])
                .unwrap()
                .contains("no `end`")
        );
        assert!(
            err(vec![json!({"type": "else"})])
                .unwrap()
                .contains("`else`")
        );
        assert!(
            err(vec![json!({"type": "end"})])
                .unwrap()
                .contains("`end` without")
        );
        let deep: Vec<Value> = (0..4)
            .map(|_| json!({"type": "repeat", "times": 1}))
            .chain([note.clone()])
            .chain((0..4).map(|_| json!({"type": "end"})))
            .collect();
        assert!(err(deep).unwrap().contains("nest"));
        assert!(
            err(vec![
                json!({"type": "type_text", "text": "{var:who}", "variables": true})
            ])
            .unwrap()
            .contains("never set")
        );
        let huge = vec![
            json!({"type": "repeat", "times": 100}),
            json!({"type": "repeat", "times": 100}),
            note.clone(),
            json!({"type": "end"}),
            json!({"type": "end"}),
        ];
        assert!(err(huge).unwrap().contains("at most 1000"));
        assert!(
            err(vec![
                json!({"type": "if", "when": {}}),
                note,
                json!({"type": "end"})
            ])
            .is_some()
        );
    }

    /// #101: a macro with blocks loads as a rule (the schema accepts the markers and `{var:…}`).
    #[test]
    fn a_macro_with_blocks_and_variables_loads() {
        let mut r = engine_rule("count", "F13", "pause");
        r["steps"] = json!([
            {"type": "set_var", "name": "n", "value": "1"},
            {"type": "repeat", "times": 2},
            {"type": "notify", "title": "Tick"},
            {"type": "end"},
            {"type": "if", "when": {"apps": [{"exe": "notepad.exe"}]}},
            {"type": "notify", "title": "{n}"},
            {"type": "end"}
        ]);
        let mut t = engine_rule("typed", "F14", "pause");
        t["steps"] = json!([
            {"type": "set_var", "name": "who", "value": "Kim"},
            {"type": "type_text", "text": "Hi {var:who}", "variables": true}
        ]);
        t["consent"] =
            json!({"input-injection": {"granted_at": "2026-11-12T09:00:00Z", "via": "gui"}});
        let l = parse(&doc(json!([r, t])));
        let active: Vec<&str> = l.bindings.iter().map(|b| b.rule.as_str()).collect();
        assert_eq!(active, ["count", "typed"], "{:?}", l.inactive);
    }
}
