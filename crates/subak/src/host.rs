//! `subak host serve` — the Host supervisor and session manager (ADR 0003, ADR 0005, ADR 0016,
//! ADR 0019, ADR 0020; docs/SESSION_MODEL.md).
//!
//! The Host serves IPC v1 ([`crate::proto`]) on the per-user pipe. Every connection must pass,
//! in this order: the pipe's owner-only DACL (kernel), a `hello` within 5 s, the user SID of the
//! connection's own token equal to the Host's, a valid `hello`, a common protocol major. After
//! `welcome` it may send requests; what it may call depends on its client kind
//! ([`proto::authorize`]). A process is only ever started by `terminal.create` from a `cli` or
//! `gui` client (or `--shell` at Host start), and only typed into by an interactively attached
//! `cli` or `gui` connection.
//!
//! **Session manager.** The Host owns any number of Terminals ([`crate::session`]): each has
//! internal Windows with Panes (any number since Step 21, ADR 0054), and each Pane its own pseudo
//! console (ConPTY) and its own kill-on-close job, so every pane's process tree is isolated. The model and the state
//! machine live in `session.rs`; this file owns the OS side (pseudo consoles, jobs, threads,
//! output rings) and applies every state change under one lock, emitting its event in the same
//! step, so a client that subscribes and then lists never misses or double-counts a change.
//!
//! Ending is claimed once, under the lock, at two levels. A Terminal's end (a stop, a kill, a
//! Host stop, or the exit of its last running pane) is claimed by whoever sets its `ending`; it
//! claims every pane and applies the state machine once they are down. A single pane's end (a
//! `pane.kill`, a `window.kill`, or the exit of a pane while others still run) claims that pane
//! only and takes it out of the model. Anyone who arrives later waits for the same result, and a
//! kill arriving during a graceful stop escalates it. A pane-level end never takes a Terminal's
//! last running pane: that is the Terminal's own end, so no window or pane command can end a
//! whole Terminal. Client connections own nothing: a client that detaches, disconnects or crashes only loses
//! its subscriptions and attachments.
//!
//! Lifetime (ADR 0003 §4): the Host runs while it owns a live Terminal or has a client. When both
//! are gone it waits the idle grace period (`--idle-exit`, default 10 s) and exits. It never
//! polls: idle, every thread is blocked in the kernel, and the only timed wait is that grace.
//! Until Step 29 persists them, stopped Terminal records live only as long as the Host.
//!
//! Threads: an accept loop, a handler and a writer per connection, an output reader and an exit
//! waiter per pane, and the main thread that supervises lifetime and teardown. A panic while
//! handling one request fails that request with `internal_error`; the Host continues.

use crate::claude;
use crate::layout;
use crate::output::{CliError, Code};
use crate::proto::{
    self, ClientKind, ErrorCode, FLAG_INPUT, FLAG_SNAPSHOT, Frame, IpcError, Refusal, Request,
};
use crate::pty::{self, EnvOverrides, Pty};
use crate::redact::{self, Redactor};
use crate::screen::{self, HistoryRange, Limits, Pos, Screen};
use crate::session::{self, Command, EndCause, LastExit, Level, Registry, Spec, Transition};
use crate::state;
use crate::win::pipe::{self, Listener, Pipe};
use crate::win::{self, EnvBlock, JobLimits};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

/// Initial pseudo console size until a client attaches with its own.
pub const DEFAULT_SIZE: (u16, u16) = (120, 30);
/// Idle grace period of ADR 0003 §4.
pub const DEFAULT_IDLE_EXIT_S: u64 = 10;
/// Most connections (in handshake or established) served at once; more get `busy`.
pub const MAX_CONNECTIONS: usize = 64;

/// The connection id hooks run under (Step 25): no real connection has it (they start at 1).
const HOOK_CONN: u64 = 0;
/// Most Terminals (live and stored) one Host owns; more get `busy`.
pub const MAX_TERMINALS: usize = 256;
/// Most panes one Terminal holds across its windows (Step 21); more get `busy`.
pub const MAX_PANES_PER_TERMINAL: usize = 64;
/// Most running panes one Host owns (Step 21: each is a pseudo console and a conhost process);
/// more get `busy`.
pub const MAX_PANES: usize = 512;
/// Bytes queued per connection; each queued frame also counts [`FRAME_COST`], so many tiny
/// frames (ConPTY's output arrives in chunks of ~80 bytes on Windows 10) are bounded by memory,
/// not by a frame count (ADR 0025). Output that does not fit makes its stream lag: its queued
/// output is dropped, and the client gets `pane.resync` and a fresh snapshot once its queue has
/// drained to [`RESYNC_BELOW`] (ADR 0030). A control message that does not fit disconnects it.
const QUEUE_BYTES: usize = 16 * 1024 * 1024;
const FRAME_COST: usize = 64;
/// A lagging stream is resynced once its connection's queue holds no more than this.
const RESYNC_BELOW: usize = QUEUE_BYTES / 4;
/// Largest JSON size of the lines one `pane.capture` answer carries (a control message is at
/// most 256 KiB); older lines are left out and the answer says `truncated`.
const CAPTURE_BYTES: usize = 192 * 1024;
/// Most bytes the writer of one connection takes off its queue for one write.
const WRITE_BATCH: usize = 256 * 1024;
/// How long a graceful stop waits for the processes after `ClosePseudoConsole`.
const GRACE: Duration = Duration::from_secs(5);
/// How long a pane's job may take to empty after its root process exited before what is left is
/// terminated. Windows takes an exiting process out of its job only after it has woken the
/// threads waiting for its exit, so right after the root's exit the job's active-process count
/// still includes it: on this machine in 300 of 300 runs of `cmd /c exit 42`, for up to 3.8 ms
/// idle, longer under load (Step 19). Checking at once made a program that ended by itself look
/// forced (`last_exit.forced`); a job that empties within this bound ended by itself.
const JOB_SETTLE: Duration = Duration::from_secs(1);
/// How long a second party waits for a teardown someone else performs.
const TEARDOWN_WAIT: Duration = Duration::from_secs(30);
/// Longest command line `CreateProcessW` accepts.
const MAX_COMMAND_LINE: usize = win::launch::MAX_COMMAND_LINE;

/// What `host serve` needs.
#[derive(Debug)]
pub struct ServeOpts {
    pub endpoint: String,
    /// Step 29: where `state.json` lives; `None`: nothing is persisted.
    pub state_dir: Option<PathBuf>,
    /// The command line of a first Terminal started with the Host; `None` starts a Host that
    /// owns no Terminal (what start on demand uses).
    pub shell: Option<String>,
    pub job_breakaway: Option<bool>,
    pub idle_exit: Duration,
}

/// The shell started when no command is given.
pub fn default_shell() -> String {
    std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into())
}

/// The user profile, the Host's working directory (ADR 0003 §2): from the logon environment
/// when given, else from this process's environment.
pub fn profile_dir(env: Option<&EnvBlock>) -> Option<PathBuf> {
    env.and_then(|e| e.get("USERPROFILE"))
        .or_else(|| std::env::var("USERPROFILE").ok())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// One queued frame. Output frames are shared by every client attached to the pane (`Arc`), so
/// several lagging clients hold one copy of the same output, not one each.
struct Out {
    bytes: Arc<[u8]>,
    ack: Option<mpsc::Sender<()>>,
    /// The stream of an output (or snapshot) frame, so a lagging stream's frames can be dropped.
    stream: Option<u32>,
    /// For an output frame: the pane's history mark (`total_lines`) before its bytes, so a resync
    /// carries every history line the dropped frames had added.
    mark: u64,
}

#[derive(Default)]
struct Pending {
    items: std::collections::VecDeque<Out>,
    /// Bytes plus [`FRAME_COST`] per item.
    cost: usize,
    closed: bool,
    /// A stream of this connection dropped output; the writer asks for a resync once drained.
    lagging: bool,
}

/// The queue between a connection's senders and its writer thread.
#[derive(Default)]
struct Queue {
    pending: Mutex<Pending>,
    ready: Condvar,
    /// The connection id once its handshake registered it (for the writer's resync request).
    conn: std::sync::atomic::AtomicU64,
}

impl Queue {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The writer's side: the next batch (several small frames are written at once; empty when
    /// only a resync is due), or `None` once the queue is closed and drained.
    fn take(&self) -> Option<Vec<Out>> {
        let mut q = self.lock();
        while q.items.is_empty() && !q.closed && !q.lagging {
            q = self
                .ready
                .wait(q)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        let mut batch = Vec::new();
        let mut size = 0;
        while let Some(o) = q.items.front() {
            if !batch.is_empty() && size + o.bytes.len() > WRITE_BATCH {
                break;
            }
            let o = q.items.pop_front().expect("front exists");
            size += o.bytes.len();
            q.cost -= o.bytes.len() + FRAME_COST;
            let ack = o.ack.is_some();
            batch.push(o);
            if ack {
                break; // a barrier is acknowledged as soon as everything before it is written
            }
        }
        (!batch.is_empty() || (q.lagging && !q.closed)).then_some(batch)
    }

    /// Whether a lagging stream should be resynced now (the queue has drained far enough);
    /// clears the request.
    fn resync_due(&self) -> bool {
        let mut q = self.lock();
        let due = q.lagging && !q.closed && q.cost <= RESYNC_BELOW;
        if due {
            q.lagging = false;
        }
        due
    }

    /// No more frames: the writer drains what is queued and ends.
    fn close(&self) {
        self.lock().closed = true;
        self.ready.notify_all();
    }
}

/// The sending side of one connection: a queue bounded in bytes to its writer thread. A client
/// that falls that far behind is disconnected; it never blocks the Host.
#[derive(Clone)]
struct Sink {
    queue: Arc<Queue>,
    pipe: Arc<Pipe>,
}

impl Sink {
    fn push(&self, bytes: Arc<[u8]>, ack: Option<mpsc::Sender<()>>) -> bool {
        let cost = bytes.len() + FRAME_COST;
        {
            let mut q = self.queue.lock();
            if !q.closed && q.cost + cost <= QUEUE_BYTES {
                q.cost += cost;
                q.items.push_back(Out {
                    bytes,
                    ack,
                    stream: None,
                    mark: 0,
                });
                drop(q);
                self.queue.ready.notify_one();
                return true;
            }
        }
        self.pipe.cancel(); // its handler sees the error and removes the connection
        false
    }

    /// Queues an output or snapshot frame of `stream`. When it does not fit, the stream lags:
    /// its queued output frames are dropped (control messages stay) and `Err` returns the
    /// history mark the resync must start from. The pane never waits for a client.
    fn push_output(&self, stream: u32, bytes: Arc<[u8]>, mark: u64) -> Result<(), u64> {
        let cost = bytes.len() + FRAME_COST;
        let mut q = self.queue.lock();
        if q.closed {
            return Ok(()); // the connection is going away
        }
        if q.cost + cost <= QUEUE_BYTES {
            q.cost += cost;
            q.items.push_back(Out {
                bytes,
                ack: None,
                stream: Some(stream),
                mark,
            });
            drop(q);
            self.queue.ready.notify_one();
            return Ok(());
        }
        let mut since = mark;
        let mut freed = 0;
        q.items.retain(|o| {
            if o.stream == Some(stream) {
                since = since.min(o.mark);
                freed += o.bytes.len() + FRAME_COST;
                false
            } else {
                true
            }
        });
        q.cost -= freed;
        q.lagging = true;
        drop(q);
        self.queue.ready.notify_one();
        Err(since)
    }

    fn control(&self, msg: &Value) -> bool {
        self.push(proto::encode_control(msg).into(), None)
    }

    /// Queues a last message and waits (bounded) until it was written.
    fn last(&self, msg: &Value) {
        let (atx, arx) = mpsc::channel();
        if self.push(proto::encode_control(msg).into(), Some(atx)) {
            let _ = arx.recv_timeout(Duration::from_secs(1));
        }
    }

    /// A barrier: signals once everything queued before it was written.
    fn flushed(&self) -> Option<mpsc::Receiver<()>> {
        let (atx, arx) = mpsc::channel();
        self.push(Arc::from(&[][..]), Some(atx)).then_some(arx)
    }
}

/// One attachment of a connection to a pane's stream.
#[derive(Debug, Clone, Copy)]
struct Att {
    interactive: bool,
    /// The stream lags (its output was dropped): no output is queued until the resync, which
    /// carries the history lines since this mark.
    lag: Option<u64>,
}

struct Conn {
    sink: Sink,
    pid: Option<u32>,
    kind: ClientKind,
    name: String,
    version: String,
    /// Streams this connection is attached to.
    attached: BTreeMap<u32, Att>,
    topics: Vec<String>,
    event_seq: u64,
}

impl Conn {
    fn event(&mut self, name: &str, data: &Value) -> bool {
        self.event_seq += 1;
        self.sink.push(
            proto::encode_control(&proto::event(self.event_seq, name, data)).into(),
            None,
        )
    }
}

/// The terminal model and counters of one pane (ADR 0030). Kept while its Terminal exists, so
/// a `stopped` Terminal can still be captured; dropped when the Terminal is removed.
struct PaneOut {
    screen: Screen,
    terminal_id: String,
    seq: u64,
    input_bytes: u64,
}

/// The OS side of one running pane: its pseudo console and job, its reader thread, and the
/// result of its teardown once someone performed it.
struct PaneRt {
    pane_id: String,
    terminal_id: String,
    stream: u32,
    pty: Arc<dyn Pty>,
    reader: Mutex<Option<JoinHandle<()>>>,
    /// The job was terminated (not every process ended by itself).
    forced: AtomicBool,
    /// Someone performs this pane's teardown (set under the Host lock, once).
    claimed: AtomicBool,
    done: Mutex<Option<Value>>,
    done_cv: Condvar,
}

impl PaneRt {
    fn new(pane_id: &str, terminal_id: &str, stream: u32, pty: Arc<dyn Pty>) -> Arc<Self> {
        Arc::new(Self {
            pane_id: pane_id.to_string(),
            terminal_id: terminal_id.to_string(),
            stream,
            pty,
            reader: Mutex::new(None),
            forced: AtomicBool::new(false),
            claimed: AtomicBool::new(false),
            done: Mutex::new(None),
            done_cv: Condvar::new(),
        })
    }

    fn is_claimed(&self) -> bool {
        self.claimed.load(Ordering::SeqCst)
    }

    fn terminate(&self) {
        self.forced.store(true, Ordering::SeqCst);
        self.pty.kill_tree(1);
    }

    fn set_done(&self, report: Value) {
        *self
            .done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(report);
        self.done_cv.notify_all();
    }

    /// The teardown report of whoever claimed it, waiting (bounded) until it is there.
    fn wait_done(&self) -> Value {
        let guard = self
            .done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (guard, _) = self
            .done_cv
            .wait_timeout_while(guard, TEARDOWN_WAIT, |d| d.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.clone().unwrap_or_else(
            || json!({"pane_id": self.pane_id, "terminal_id": self.terminal_id, "pending": true}),
        )
    }
}

#[derive(Default)]
struct Stats {
    /// Step 25: hook command lines run and refused.
    hooks_run: u64,
    hooks_failed: u64,
    /// Step 27: Claude reports accepted, and refused (not from a process in the pane).
    claude_reports: u64,
    claude_refused: u64,
    connections_total: u64,
    rejected_identity: u64,
    rejected_limit: u64,
    rejected_handshake: u64,
    rejected_stopping: u64,
    requests_total: u64,
    errors: BTreeMap<&'static str, u64>,
    protocol_errors: u64,
    panics: u64,
    input_frames_dropped: u64,
    lagging_clients_dropped: u64,
    /// Lagging attachments brought back with a fresh snapshot (Step 13).
    lagging_resyncs: u64,
    /// Output frames not queued for a lagging attachment.
    output_frames_skipped: u64,
    snapshots: u64,
    snapshot_bytes: u64,
    captures: u64,
    terminals_created: u64,
    spawn_failures: u64,
    terminals_removed: u64,
    /// Output and input bytes, and history lines evicted or cleared, of panes already removed.
    ended_output_bytes: u64,
    ended_input_bytes: u64,
    ended_evicted_lines: u64,
    ended_cleared_lines: u64,
}

#[derive(Clone, Copy)]
enum Reject {
    Identity,
    Limit,
    Handshake,
    Stopping,
}

struct State {
    reg: Registry,
    out: HashMap<String, PaneOut>,
    rt: HashMap<String, Arc<PaneRt>>,
    conns: HashMap<u64, Conn>,
    /// Accepted connections that have not finished their handshake.
    pending: usize,
    next_conn: u64,
    stopping: bool,
    stats: Stats,
    /// The Terminal started with `--shell` (its pane is `host.status.pane`).
    initial: Option<String>,
    /// Step 24: tmux paste buffers, newest first; in this Host's memory only (never written,
    /// never logged), gone when it exits.
    buffers: Vec<PasteBuffer>,
    /// The number of the next automatic buffer name (`buffer0`, `buffer1`, …).
    next_buffer: u64,
    /// Step 25: tmux hooks: hook name → a tmux command line run by the Host (memory only).
    hooks: BTreeMap<String, String>,
    /// Step 25: runs of hooks waiting for the hook thread, and whether one is running (its own
    /// events never trigger hooks: no loops).
    hook_tx: Option<mpsc::Sender<HookRun>>,
    in_hook: bool,
    /// Step 27: Claude Code sessions per pane (memory only): pane id → session id → state.
    claude: HashMap<String, BTreeMap<String, ClaudeSession>>,
    /// Step 28: the Claude state last announced per Terminal (`terminal.claude_changed`).
    claude_told: HashMap<String, &'static str>,
    /// Step 29 (ADR 0063): the durable state. `saved`: Terminals of an earlier Host run, kept
    /// as records (never started here); `restore`: the policy of live Terminals.
    persist: Persist,
}

/// The Host's side of `state.json` (Step 29).
#[derive(Debug, Default)]
struct Persist {
    store: Option<state::Store>,
    /// Read-only: a newer version's file, or a store that could not be read.
    read_only: Option<String>,
    saved: Vec<Value>,
    restore: HashMap<String, String>,
    dirty: bool,
    /// After the final write of a clean stop nothing more is written (the ending panes must
    /// not erase the Terminals that should survive).
    frozen: bool,
    tx: Option<mpsc::Sender<()>>,
    writes: u64,
    last_write: Option<String>,
    last_bytes: u64,
    last_error: Option<String>,
    loaded: Value,
}

/// One Claude Code session in a pane (Step 27): what its last hook said, and when.
#[derive(Debug, Clone)]
struct ClaudeSession {
    terminal_id: String,
    state: String,
    event: String,
    reason: Option<String>,
    since: SystemTime,
    at: Instant,
    /// Step 28: its going stale was announced.
    stale_told: bool,
}

/// After this long without a hook, an `in-progress` session is `unknown` (a missed hook, a
/// killed Claude): Subak does not guess (ADR 0010 §4).
pub const CLAUDE_STALE: Duration = Duration::from_secs(30 * 60);

impl ClaudeSession {
    /// The state as shown: `unknown` once [`CLAUDE_STALE`] in progress without a hook.
    fn shown(&self) -> &'static str {
        if self.state == "in-progress" && self.at.elapsed() > stale_after() {
            "unknown"
        } else {
            claude::PRIORITY
                .iter()
                .copied()
                .find(|p| *p == self.state)
                .unwrap_or("unknown")
        }
    }
}

/// [`CLAUDE_STALE`], or `SUBAK_TEST_CLAUDE_STALE_MS` in debug builds (tests of stale signals).
fn stale_after() -> Duration {
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("SUBAK_TEST_CLAUDE_STALE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        return Duration::from_millis(ms);
    }
    CLAUDE_STALE
}

/// One hook run (Step 25): the command line and where its event happened.
#[derive(Debug, Clone)]
struct HookRun {
    hook: String,
    line: String,
    terminal: Option<String>,
    window: Option<String>,
    pane: Option<String>,
}

/// tmux's hook names Subak runs, and the event each follows (Step 25, ADR 0059).
pub const HOOKS: &[(&str, &str)] = &[
    ("after-new-window", "window.created"),
    ("after-split-window", "pane.created"),
    ("pane-exited", "pane.exited"),
    ("after-select-pane", "pane.selected"),
    ("after-select-window", "window.selected"),
    ("window-renamed", "window.renamed"),
    ("session-renamed", "terminal.renamed"),
    ("session-created", "terminal.created"),
    ("session-closed", "terminal.removed"),
    ("window-layout-changed", "window.layout"),
    ("alert-activity", "window.alert:activity"),
    ("alert-bell", "window.alert:bell"),
    ("window-created", "window.created"),
    ("window-closed", "window.removed"),
    ("pane-created", "pane.created"),
    ("window-pane-changed", "pane.selected"),
    ("session-window-changed", "window.selected"),
];

/// At most this many hooks, and this long a hook's command line.
pub const MAX_HOOKS: usize = 32;
pub const MAX_HOOK_LINE: usize = 1024;

/// One paste buffer (Step 24).
#[derive(Debug, Clone)]
struct PasteBuffer {
    name: String,
    data: String,
    /// Named by the user (`-b`), so a full list never drops it first (tmux).
    named: bool,
}

/// At most this many buffers (tmux `buffer-limit`); the oldest automatic one goes first.
pub const MAX_BUFFERS: usize = 50;
/// The largest buffer, in bytes.
pub const MAX_BUFFER_BYTES: usize = 1 << 20;

impl State {
    fn live_terminals(&self) -> usize {
        self.reg
            .terminals
            .iter()
            .filter(|t| t.state.is_live())
            .count()
    }

    fn idle(&self) -> bool {
        self.live_terminals() == 0 && self.conns.is_empty() && self.pending == 0
    }

    fn live(&self, p: &session::Pane) -> session::Live {
        let rt = self.rt.get(&p.id);
        session::Live {
            running: rt.is_some_and(|r| r.pty.running()),
            processes: rt.map_or(0, |r| r.pty.processes()),
            clients: self
                .conns
                .values()
                .filter(|c| c.attached.contains_key(&p.stream))
                .count(),
            output_bytes: self.out.get(&p.id).map_or(0, |o| o.seq),
            history: self.out.get(&p.id).map(|o| o.screen.history()),
        }
    }

    /// Step 29: asks the state writer for a (coalesced) write.
    fn mark_dirty(&mut self) {
        let p = &mut self.persist;
        if p.store.is_none() || p.read_only.is_some() || p.frozen || p.dirty {
            return;
        }
        p.dirty = true;
        if let Some(tx) = &p.tx {
            let _ = tx.send(());
        }
    }

    /// Step 29: the document to write now: live Terminals, then saved ones (not live).
    fn state_doc(&self) -> Value {
        let claude = |pane: &str| {
            self.claude.get(pane).and_then(|sessions| {
                sessions
                    .iter()
                    .max_by_key(|(_, s)| s.at)
                    .map(|(sid, _)| sid.clone())
            })
        };
        let mut terminals: Vec<Value> = self
            .reg
            .terminals
            .iter()
            .map(|t| {
                let policy = self
                    .persist
                    .restore
                    .get(&t.id)
                    .map_or("ask", String::as_str);
                state::terminal_record(t, policy, &claude)
            })
            .collect();
        for s in &self.persist.saved {
            if !terminals.iter().any(|t| t["id"] == s["id"]) {
                terminals.push(s.clone());
            }
        }
        state::seal(&terminals, &proto::rfc3339(SystemTime::now()))
    }

    /// Step 28: a Terminal's Claude state, the most urgent of its panes' sessions (`none`).
    fn claude_of(&self, terminal_id: &str) -> &'static str {
        claude::aggregate(
            self.claude
                .values()
                .flat_map(BTreeMap::values)
                .filter(|s| s.terminal_id == terminal_id)
                .map(ClaudeSession::shown),
        )
    }

    /// Step 28: how long until the next in-progress session goes stale (not yet announced).
    fn claude_deadline(&self) -> Option<Duration> {
        let now = Instant::now();
        self.claude
            .values()
            .flat_map(BTreeMap::values)
            .filter(|s| s.state == "in-progress" && !s.stale_told)
            .map(|s| (s.at + stale_after()).saturating_duration_since(now))
            .min()
    }

    /// Step 28: announces sessions that went stale (`unknown`) and the Terminals whose
    /// aggregate changed with them.
    fn claude_tick(&mut self) {
        let mut told = Vec::new();
        for (pane, sessions) in &mut self.claude {
            for (sid, s) in sessions.iter_mut() {
                if s.state == "in-progress" && !s.stale_told && s.shown() == "unknown" {
                    s.stale_told = true;
                    told.push((pane.clone(), sid.clone(), s.terminal_id.clone()));
                }
            }
        }
        let mut terminals = std::collections::BTreeSet::new();
        for (pane, sid, tid) in told {
            let data = json!({"pane_id": pane, "terminal_id": tid, "session_id": sid,
                "state": "unknown", "previous_state": "in-progress", "event": "stale"});
            self.notify("claude", "claude.state_changed", &data);
            terminals.insert(tid);
        }
        for tid in terminals {
            let before = self.claude_told.get(&tid).copied().unwrap_or("none");
            self.claude_changed(&tid, before);
        }
    }

    /// Step 28: tells `terminals` subscribers when a Terminal's Claude state changed.
    fn claude_changed(&mut self, terminal_id: &str, before: &'static str) {
        let now = self.claude_of(terminal_id);
        self.claude_told.insert(terminal_id.to_string(), now);
        if now != before {
            let data = json!({"terminal_id": terminal_id, "claude_state": now, "previous": before});
            self.notify("terminals", "terminal.claude_changed", &data);
        }
    }

    fn terminal_json(&self, t: &session::Terminal) -> Value {
        let mut j = session::terminal_json(t, &|p| self.live(p));
        j["claude_state"] = json!(self.claude_of(&t.id));
        j
    }

    fn summary_json(&self, t: &session::Terminal) -> Value {
        let mut j = session::summary_json(t, &|p| self.live(p));
        j["claude_state"] = json!(self.claude_of(&t.id));
        j
    }

    fn window_json(&self, t: &session::Terminal, w: &session::Window) -> Value {
        let (mut clients, mut busy) = (0, false);
        session::window_json(t, w, &|p| self.live(p), &mut clients, &mut busy)
    }

    /// Step 22: gives the panes whose layout size changed that size in their screens, and
    /// returns their pseudo consoles to resize once the lock is released ([`apply_resizes`]).
    fn resized(&mut self, changed: Vec<(String, (u16, u16))>) -> Vec<(Arc<PaneRt>, (u16, u16))> {
        let mut out = Vec::new();
        for (id, size) in changed {
            resize_model(self, &id, size);
            if let Some(rt) = self.rt.get(&id) {
                out.push((Arc::clone(rt), size));
            }
        }
        out
    }

    /// Step 22: lays the window of `pane_id` out at a client's size (the latest interactive
    /// client's size is the window's, tmux `window-size latest`); announces a changed layout.
    fn resize_window(
        &mut self,
        tid: &str,
        pane_id: &str,
        size: (u16, u16),
    ) -> Vec<(Arc<PaneRt>, (u16, u16))> {
        let Some(t) = self.reg.get_mut(tid) else {
            return Vec::new();
        };
        let Some(w) = t
            .windows
            .iter_mut()
            .find(|w| w.panes.iter().any(|p| p.id == pane_id))
        else {
            return Vec::new();
        };
        let before = w.layout().size();
        let changed = w.resize(size);
        let (wid, after) = (w.id.clone(), w.layout().size());
        // A window of one pane is just that pane's size, as before Step 22: no event.
        if w.panes.len() > 1 && (before != after || !changed.is_empty()) {
            self.layout_event(tid, &wid);
        }
        self.resized(changed)
    }

    /// `window.layout` (Step 22): a window's layout, size, zoomed pane and pane places changed.
    fn layout_event(&mut self, tid: &str, wid: &str) {
        let Some(t) = self.reg.get(tid) else {
            return;
        };
        let Some(w) = t.windows.iter().find(|w| w.id == wid) else {
            return;
        };
        let data = layout_json(t, w);
        self.notify("terminals", "window.layout", &data);
    }

    fn rt_of_stream(&self, stream: u32) -> Option<&Arc<PaneRt>> {
        self.rt.values().find(|r| r.stream == stream)
    }

    /// `host.status.scrollback`: what the Host keeps of terminal output, and where.
    fn scrollback_json(&self) -> Value {
        let lines: usize = self.out.values().map(|o| o.screen.history().lines).sum();
        let bytes: usize = self.out.values().map(|o| o.screen.history().bytes).sum();
        let memory: usize = self.out.values().map(|o| o.screen.memory_bytes()).sum();
        json!({
            "panes": self.out.len(),
            "lines": lines,
            "bytes": bytes,
            "memory_bytes": memory,
            "default_limit_lines": screen::DEFAULT_HISTORY_LINES,
            "limit_bytes_per_pane": screen::HISTORY_BYTES,
            "persisted": false,
        })
    }

    /// Sends an event to every connection subscribed to `topic`, in the same critical section
    /// as the change it reports.
    fn notify(&mut self, topic: &str, name: &str, data: &Value) {
        // Step 29: what Terminals, windows, panes and Claude references look like changed.
        if matches!(topic, "terminals" | "claude") {
            self.mark_dirty();
        }
        for c in self.conns.values_mut() {
            if c.topics.iter().any(|t| t == topic) {
                c.event(name, data);
            }
        }
        // Step 25: a hook for this event runs its tmux command line on the hook thread.
        if self.in_hook || self.hooks.is_empty() {
            return;
        }
        let key = match name {
            "window.alert" => format!("window.alert:{}", data["kind"].as_str().unwrap_or("")),
            n => n.to_string(),
        };
        for (hook, event) in HOOKS {
            if *event == key
                && let Some(line) = self.hooks.get(*hook)
                && let Some(tx) = &self.hook_tx
            {
                let s = |k: &str| data[k].as_str().map(String::from);
                let _ = tx.send(HookRun {
                    hook: (*hook).to_string(),
                    line: line.clone(),
                    terminal: s("terminal_id")
                        .or_else(|| data["terminal"]["id"].as_str().map(String::from)),
                    window: s("window_id")
                        .or_else(|| data["window"]["id"].as_str().map(String::from)),
                    pane: s("pane_id").or_else(|| data["pane"]["id"].as_str().map(String::from)),
                });
            }
        }
    }

    fn state_changed(&mut self, terminal_id: &str, previous: &str, cause: Option<EndCause>) {
        let Some(t) = self.reg.get(terminal_id) else {
            return;
        };
        let data = json!({
            "terminal_id": t.id, "name": t.name.display, "state": t.state.as_str(),
            "previous_state": previous, "cause": cause.map(EndCause::as_str),
        });
        self.notify("terminals", "terminal.state_changed", &data);
    }

    fn removed(&mut self, terminal_id: &str, name: &str, reason: &str) {
        self.stats.terminals_removed += 1;
        // Its screen and history go with it (docs/SCROLLBACK.md §5).
        let panes: Vec<String> = self
            .out
            .iter()
            .filter(|(_, o)| o.terminal_id == terminal_id)
            .map(|(k, _)| k.clone())
            .collect();
        for k in panes {
            if let Some(o) = self.out.remove(&k) {
                let h = o.screen.history();
                self.stats.ended_output_bytes += o.seq;
                self.stats.ended_input_bytes += o.input_bytes;
                self.stats.ended_evicted_lines += h.evicted_lines;
                self.stats.ended_cleared_lines += h.cleared_lines;
            }
        }
        let data = json!({"terminal_id": terminal_id, "name": name, "reason": reason});
        self.notify("terminals", "terminal.removed", &data);
    }

    /// Drops one pane's screen and history (the pane left its Terminal, Step 21), keeping its
    /// counters in the Host's totals.
    fn drop_screen(&mut self, pane_id: &str) {
        if let Some(o) = self.out.remove(pane_id) {
            let h = o.screen.history();
            self.stats.ended_output_bytes += o.seq;
            self.stats.ended_input_bytes += o.input_bytes;
            self.stats.ended_evicted_lines += h.evicted_lines;
            self.stats.ended_cleared_lines += h.cleared_lines;
        }
    }

    /// The running panes of `t` nobody is ending yet, other than `except`.
    fn others_running(&self, t: &session::Terminal, except: &[&str]) -> usize {
        t.panes()
            .filter(|p| !except.contains(&p.id.as_str()))
            .filter_map(|p| self.rt.get(&p.id))
            .filter(|r| !r.is_claimed())
            .count()
    }

    /// The selection events after the active window or pane of Terminal `tid` changed (Step 21):
    /// `window.selected` when its active window changed, else `pane.selected` when the active
    /// pane of a window changed. Every client sees the same selection.
    fn selected(&mut self, tid: &str, window_changed: bool, pane_changed: bool) {
        let Some(t) = self.reg.get(tid) else {
            return;
        };
        let w = &t.windows[t.active_window];
        let data = json!({
            "terminal_id": t.id, "window_id": w.id, "pane_id": w.panes[w.active_pane].id,
        });
        if window_changed {
            self.notify("terminals", "window.selected", &data);
        } else if pane_changed {
            self.notify("terminals", "pane.selected", &data);
        }
    }

    /// The events of a pane taken out of its Terminal ([`session::Terminal::remove_pane`]).
    fn pane_removed(&mut self, tid: &str, pane_id: &str, r: &session::Removal, reason: &str) {
        let data = json!({
            "terminal_id": tid, "window_id": r.window_id, "pane_id": pane_id, "reason": reason,
        });
        self.notify("terminals", "pane.removed", &data);
        if r.window_removed {
            let data = json!({"terminal_id": tid, "window_id": r.window_id, "reason": reason});
            self.notify("terminals", "window.removed", &data);
        }
        self.selected(tid, r.window_changed, r.pane_changed);
        if let Some(p) = &r.window_pane {
            let data = json!({"terminal_id": tid, "window_id": r.window_id, "pane_id": p});
            self.notify("terminals", "pane.selected", &data);
        }
    }
}

/// What `window.select_layout` arranges (Step 25: also `next`, `previous`, `spread`).
#[derive(Debug, Clone, Copy)]
enum Pick {
    Preset(layout::Preset),
    Next,
    Previous,
    Spread,
}

/// The layout methods of Step 22 ([`Host::layout_op`]).
#[derive(Debug, Clone, Copy)]
enum LayoutOp {
    Adjust,
    Swap,
    Zoom,
    /// `window.rotate` (tmux `rotate-window`).
    Rotate,
    /// `window.select_layout` (tmux `select-layout`, the basic layouts).
    Preset,
}

enum Ev {
    Stop { conn: u64, id: u64 },
    Changed,
}

enum Exit {
    Stop { conn: u64, id: u64 },
    Idle,
}

struct Host {
    state: Mutex<State>,
    events: mpsc::Sender<Ev>,
    pid: u32,
    sid: String,
    started_at: String,
    started: Instant,
    endpoint: String,
    job_breakaway: Option<bool>,
    in_job: bool,
    job: Option<JobLimits>,
    session: u32,
    idle_exit: Duration,
    /// Whether the logon environment could be built at start (`user`) or not (`inherited`).
    /// Each new pane builds its own, fresh (`win::launch`, ADR 0025).
    environment: &'static str,
    /// The default working directory of new panes: the user profile.
    home: Option<PathBuf>,
    /// Step 27: the endpoint's name (`SUBAK_ENDPOINT` in every pane).
    endpoint_name: String,
}

/// Runs the Host until it is stopped or idles out. Returns the final report.
pub fn serve(opts: &ServeOpts) -> Result<Value, CliError> {
    let sid = win::user_sid()
        .map_err(|e| CliError::new(Code::Io, format!("cannot determine the current user: {e}")))?;
    let mut listener = match Listener::bind(&opts.endpoint) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(crate::client::occupied(&opts.endpoint));
        }
        Err(e) => {
            return Err(CliError::new(
                Code::Io,
                format!("cannot create the Host pipe: {e}"),
            ));
        }
    };
    // However the Host was started, panes must receive Ctrl+C normally.
    win::enable_ctrl_c_for_children();
    // Panes get the user's logon environment, never the launcher's (ADR 0003 §6).
    let env = EnvBlock::for_current_user().ok();
    let home = profile_dir(env.as_ref());
    let (ev_tx, ev_rx) = mpsc::channel();
    let host = Arc::new(Host {
        state: Mutex::new(State {
            reg: Registry::default(),
            out: HashMap::new(),
            rt: HashMap::new(),
            conns: HashMap::new(),
            pending: 0,
            next_conn: 1,
            stopping: false,
            stats: Stats::default(),
            buffers: Vec::new(),
            next_buffer: 0,
            hooks: BTreeMap::new(),
            hook_tx: None,
            in_hook: false,
            claude: HashMap::new(),
            claude_told: HashMap::new(),
            persist: Persist::default(),
            initial: None,
        }),
        events: ev_tx,
        pid: win::current_pid(),
        sid,
        started_at: proto::rfc3339(SystemTime::now()),
        started: Instant::now(),
        endpoint: pipe::path(&opts.endpoint),
        job_breakaway: opts.job_breakaway,
        in_job: win::current_process_in_job(),
        job: win::current_job_limits(),
        session: win::session_id(),
        idle_exit: opts.idle_exit,
        environment: if env.is_some() { "user" } else { "inherited" },
        home,
        endpoint_name: opts.endpoint.clone(),
    });
    if let Some(shell) = &opts.shell {
        let spec = Spec {
            name: None,
            command: session::split_command_line(shell),
            command_line: Some(shell.clone()),
            cwd: host.home.as_ref().map(|p| p.display().to_string()),
            env: pty::EnvNames::default(),
            size: DEFAULT_SIZE,
            remain_on_exit: false,
            history_limit: screen::DEFAULT_HISTORY_LINES,
        };
        let launch = Launch {
            raw_line: Some(shell.clone()),
            env: EnvOverrides::default(),
        };
        let created = host.create(spec, &launch, false).map_err(|e| {
            CliError::new(
                Code::SpawnFailed,
                format!("cannot start `{shell}`: {}", e.message),
            )
            .hint("check the --shell command line")
        })?;
        host.lock().initial = Some(created["terminal_id"].as_str().unwrap_or("").to_string());
    }

    // Step 29 (ADR 0063): the state of earlier runs (kept, never started), then the writer.
    if let Some(dir) = &opts.state_dir {
        let store = state::Store::new(dir.clone());
        let l = store.load();
        let mut st = host.lock();
        st.persist.loaded = json!({
            "terminals": l.doc.as_ref().and_then(|d| d["terminals"].as_array()).map_or(0, Vec::len),
            "migrated": l.migrated,
            "recovered_from": l.recovered_from.as_ref().map(|p| p.display().to_string()),
            "corrupt_kept": l.corrupt_kept.as_ref().map(|p| p.display().to_string()),
            "newer_version": l.newer,
            "problems": l.problems,
        });
        if let Some(v) = l.newer {
            st.persist.read_only = Some(format!(
                "state.json is from a newer Subak (schema version {v})"
            ));
        } else {
            let _ = store.backup_current("start");
        }
        st.persist.saved = l
            .doc
            .and_then(|d| d["terminals"].as_array().cloned())
            .unwrap_or_default();
        st.persist.store = Some(store);
        let (tx, rx) = mpsc::channel::<()>();
        st.persist.tx = Some(tx);
        drop(st);
        thread::spawn({
            let host = Arc::clone(&host);
            move || {
                while rx.recv().is_ok() {
                    // Coalesce: everything that changes in the next moment goes in one write.
                    thread::sleep(state::COALESCE);
                    while rx.try_recv().is_ok() {}
                    host.write_state();
                }
            }
        });
    }
    // Step 25: the hook thread runs hooks' command lines one after another.
    let (hook_tx, hook_rx) = mpsc::channel::<HookRun>();
    host.lock().hook_tx = Some(hook_tx);
    thread::spawn({
        let host = Arc::clone(&host);
        move || {
            for run in hook_rx {
                host.run_hook(&run);
            }
        }
    });
    let stopper = listener.stopper();
    let acceptor = thread::spawn({
        let host = Arc::clone(&host);
        move || {
            let mut backoff = Duration::from_millis(50);
            loop {
                match listener.accept() {
                    Ok(Some(p)) => {
                        backoff = Duration::from_millis(50);
                        host.lock().pending += 1;
                        let host = Arc::clone(&host);
                        thread::spawn(move || host.serve_conn(&Arc::new(p)));
                    }
                    Ok(None) => return,
                    // Out of resources for a new instance: retry with a bounded backoff.
                    Err(_) => {
                        thread::sleep(backoff);
                        backoff = (backoff * 2).min(Duration::from_secs(1));
                    }
                }
            }
        }
    });

    let exit = host.supervise(&ev_rx);
    // Step 29: a clean stop writes the Terminals as they are, then freezes the state: ending
    // their processes now must not erase them (they are what a later start restores).
    host.write_state();
    host.lock().persist.frozen = true;
    let (requester, reason) = match exit {
        Exit::Stop { conn, id } => (Some((conn, id)), "stop_requested"),
        Exit::Idle => (None, "idle"),
    };
    host.lock()
        .notify("host", "host.stopping", &json!({ "reason": reason }));
    let initial = host.lock().initial.clone();
    let ended = host.end_all();
    let shell = initial
        .and_then(|tid| {
            ended
                .iter()
                .find(|r| r["terminal_id"] == tid.as_str())
                .cloned()
        })
        .unwrap_or(Value::Null);
    let mut acks = Vec::new();
    {
        let st = host.lock();
        if let Some((conn, id)) = requester
            && let Some(c) = st.conns.get(&conn)
        {
            let result = json!({"stopping": true, "shell": shell, "terminals": ended});
            c.sink.control(&proto::response(id, Ok(result)));
        }
        // Every event and the response reach the clients before the Host exits.
        acks.extend(st.conns.values().filter_map(|c| c.sink.flushed()));
    }
    let until = Instant::now() + Duration::from_secs(2);
    for a in acks {
        let _ = a.recv_timeout(until.saturating_duration_since(Instant::now()));
    }
    stopper.set();
    join_bounded(acceptor, Duration::from_secs(2));
    Ok(json!({
        "reason": reason,
        "host": { "pid": host.pid },
        "shell": shell,
        "terminals": ended,
    }))
}

/// Joins a thread, but gives up after `limit` (process exit reclaims anything left).
fn join_bounded(t: JoinHandle<()>, limit: Duration) {
    let until = Instant::now() + limit;
    while !t.is_finished() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    if t.is_finished() {
        let _ = t.join();
    }
}

/// Waits up to `limit` for the pane's job to hold no process; whether it emptied. Polls the
/// count (a cheap query) every 2 ms: it settles within milliseconds when nothing is left, and
/// the bound only ever delays the end of a pane whose job really still runs something.
fn job_drained(pty: &dyn Pty, limit: Duration) -> bool {
    let until = Instant::now() + limit;
    loop {
        if pty.processes() == 0 {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn flag(params: &Value, name: &str) -> Result<bool, IpcError> {
    match &params[name] {
        Value::Null => Ok(false),
        Value::Bool(b) => Ok(*b),
        _ => Err(IpcError::param(
            name,
            format!("{name} must be true or false"),
        )),
    }
}

/// Resizes the pane's terminal model to `size` and says whether its pseudo console must follow:
/// not when the model already has that size (a reattach or resize at the same size would make
/// ConPTY repaint the whole screen for nothing).
/// The data of `window.layout` and of the layout methods' results (Step 22).
fn layout_json(t: &session::Terminal, w: &session::Window) -> Value {
    let l = w.layout();
    let panes: Vec<Value> = w
        .panes
        .iter()
        .map(|p| {
            let mut j = json!({"id": p.id, "index": p.index});
            session::place_json(w, p, &mut j);
            j
        })
        .collect();
    json!({
        "terminal_id": t.id,
        "window_id": w.id,
        "active_pane": w.panes[w.active_pane].id,
        "layout": l.json(),
        "size": {"cols": l.size().0, "rows": l.size().1},
        "zoomed": l.zoomed(),
        "panes": panes,
    })
}

/// Resizes pseudo consoles after a layout change (outside the lock); the first failure.
fn apply_resizes(list: &[(Arc<PaneRt>, (u16, u16))]) -> Result<(), IpcError> {
    let mut first = None;
    for (rt, (c, r)) in list {
        if let Err(e) = rt.pty.resize(*c, *r) {
            first.get_or_insert_with(|| {
                IpcError::new(ErrorCode::InternalError, format!("resize failed: {e}"))
            });
        }
    }
    first.map_or(Ok(()), Err)
}

/// What a layout change was refused for, as an IPC error.
fn layout_refused(e: &layout::Refused, what: &str) -> IpcError {
    match e {
        layout::Refused::NoPane => IpcError::new(
            ErrorCode::TargetNotFound,
            format!("the pane is not in that window's layout ({what})"),
        ),
        layout::Refused::NoSpace { need, have } => IpcError::new(
            ErrorCode::InvalidState,
            format!(
                "not enough room to {what}: the pane has {have} cells and a split needs {need}"
            ),
        )
        .details(json!({"need": need, "have": have,
            "hint": "make the window or the pane bigger first (subak pane resize), or close a pane"})),
    }
}

fn resize_model(st: &mut State, pane_id: &str, (c, r): (u16, u16)) -> bool {
    match st.out.get_mut(pane_id) {
        Some(o) if o.screen.size() == (c, r) => false,
        Some(o) => {
            o.screen.resize(c, r);
            true
        }
        None => true,
    }
}

fn size_param(v: &Value, field: &str) -> Result<Option<(u16, u16)>, IpcError> {
    if v.is_null() {
        return Ok(None);
    }
    let dim = |k: &str| {
        v[k].as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| (2..=1000).contains(n))
    };
    match (dim("cols"), dim("rows")) {
        (Some(c), Some(r)) => Ok(Some((c, r))),
        _ => Err(IpcError::param(
            field,
            "size needs cols and rows between 2 and 1000",
        )),
    }
}

/// The required `target` string.
fn target(params: &Value) -> Result<&str, IpcError> {
    params["target"]
        .as_str()
        .ok_or_else(|| IpcError::param("target", "target must be a Terminal name or id"))
}

fn optional_string<'a>(params: &'a Value, name: &str) -> Result<Option<&'a str>, IpcError> {
    match &params[name] {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.as_str())),
        _ => Err(IpcError::param(name, format!("{name} must be a string"))),
    }
}

/// How a new pane's program is started, beyond what the model keeps: the verbatim `--shell`
/// line, and the environment overrides with their values (used once, never stored).
#[derive(Debug, Default)]
struct Launch {
    raw_line: Option<String>,
    env: EnvOverrides,
}

/// A new pane's program and settings, checked (`terminal.create`, `window.create`,
/// `pane.create`); the environment values are used once and never stored.
struct PaneLaunch {
    command: Command,
    raw_line: Option<String>,
    cwd: Option<String>,
    env: EnvOverrides,
    size: (u16, u16),
    history_limit: usize,
}

/// `invalid_state` unless the Terminal runs and nothing is ending it (Step 21: windows and panes
/// are created and ended only in a running Terminal).
fn require_running(t: &session::Terminal) -> Result<(), IpcError> {
    if t.state == session::State::Running && t.ending.is_none() {
        return Ok(());
    }
    let state = if t.ending.is_some() {
        "stopping"
    } else {
        t.state.as_str()
    };
    Err(IpcError::new(
        ErrorCode::InvalidState,
        format!(
            "Terminal `{}` is {state}; windows and panes change only while it runs",
            t.name.display
        ),
    )
    .details(json!({"terminal_id": t.id, "state": state})))
}

/// `invalid_params` for a target of the wrong level (Step 21: each operation takes one level, so
/// no window or pane command acts on a whole Terminal, and the reverse).
fn wrong_level(method: &str, wants: &str) -> IpcError {
    IpcError::param("target", format!("{method} takes {wants}"))
}

/// The `spawn_failed` error of a failed start (IPC_PROTOCOL §6): `details.reason` and, when
/// there is one, `details.hint` and `details.os_error`.
fn spawn_failed(e: &pty::SpawnError, program: &str, keep: bool, tid: Option<&str>) -> IpcError {
    let mut details = json!({
        "reason": e.reason.as_str(),
        "program": program,
        "kept": keep,
        "terminal_id": tid,
    });
    if let Some(h) = &e.hint {
        details["hint"] = json!(h);
    }
    if let Some(c) = e.os_error {
        details["os_error"] = json!(c);
    }
    let code = if e.reason == pty::Reason::UnsupportedPlatform {
        ErrorCode::UnsupportedPlatform
    } else {
        ErrorCode::SpawnFailed
    };
    IpcError::new(code, e.message.clone()).details(details)
}

enum Reply {
    Now(Result<Value, IpcError>),
    Queued,
}

/// Ends a freshly started pane that must not be kept (the Host began stopping meanwhile).
fn discard(pty: &Arc<dyn Pty>, mut output: Box<dyn Read + Send>) {
    pty.kill_tree(1);
    let drain = thread::spawn(move || {
        let mut buf = vec![0u8; 16 * 1024];
        while matches!(output.read(&mut buf), Ok(n) if n > 0) {}
    });
    let closer = thread::spawn({
        let pty = Arc::clone(pty);
        move || pty.close()
    });
    pty.wait_exit(Some(GRACE));
    join_bounded(closer, GRACE);
    join_bounded(drain, GRACE);
}

impl Host {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Step 29: writes `state.json` now (the document is built under the lock, written outside).
    fn write_state(&self) {
        let (doc, store) = {
            let mut st = self.lock();
            if st.persist.frozen || st.persist.read_only.is_some() {
                return;
            }
            let Some(store) = st.persist.store.clone() else {
                return;
            };
            st.persist.dirty = false;
            (st.state_doc(), store)
        };
        let r = store.write(&doc);
        let mut st = self.lock();
        match r {
            Ok(bytes) => {
                st.persist.writes += 1;
                st.persist.last_bytes = bytes;
                st.persist.last_write = doc["written_at"].as_str().map(String::from);
                st.persist.last_error = None;
            }
            Err(e) => st.persist.last_error = Some(e.to_string()),
        }
    }

    /// Lifetime of ADR 0003 §4. Returns why the Host stops; `stopping` is set by then.
    fn supervise(&self, rx: &mpsc::Receiver<Ev>) -> Exit {
        loop {
            let idle = self.lock().idle();
            // Read under the lock, waited on without it.
            let claude_wait = if idle {
                None
            } else {
                self.lock().claude_deadline()
            };
            let ev = if idle {
                match rx.recv_timeout(self.idle_exit) {
                    Ok(ev) => ev,
                    Err(_) => {
                        let mut st = self.lock();
                        if st.idle() && !st.stopping {
                            st.stopping = true;
                            return Exit::Idle;
                        }
                        continue;
                    }
                }
            } else if let Some(wait) = claude_wait {
                // Step 28: wake when an in-progress Claude session would go stale (only then;
                // an idle Host or one without such sessions never wakes for it).
                match rx.recv_timeout(wait + Duration::from_millis(20)) {
                    Ok(ev) => ev,
                    Err(_) => {
                        self.lock().claude_tick();
                        continue;
                    }
                }
            } else {
                match rx.recv() {
                    Ok(ev) => ev,
                    // Unreachable: the Host itself holds a sender.
                    Err(_) => return Exit::Idle,
                }
            };
            match ev {
                Ev::Changed => {}
                Ev::Stop { conn, id } => return Exit::Stop { conn, id },
            }
        }
    }

    /// Creates a Terminal (`terminal.create`, or the `--shell` Terminal of `host serve`): the
    /// name and ids are reserved under the lock (so concurrent creates with one name give one
    /// Terminal), the pane is started outside it, and a failed start leaves nothing behind
    /// unless `keep` asks for a `failed` record. Returns the `terminal.create` result.
    fn create(
        self: &Arc<Self>,
        spec: Spec,
        launch: &Launch,
        keep: bool,
    ) -> Result<Value, IpcError> {
        let (tid, pane_id, stream, size, cwd, command, history_limit) = {
            let mut st = self.lock();
            if st.stopping {
                return Err(IpcError::new(
                    ErrorCode::HostStopping,
                    "the Host is shutting down",
                ));
            }
            if st.reg.terminals.len() >= MAX_TERMINALS {
                return Err(IpcError::new(
                    ErrorCode::Busy,
                    format!("this Host already owns {MAX_TERMINALS} Terminals"),
                ));
            }
            let created_at = proto::rfc3339(SystemTime::now());
            let i = st.reg.reserve(spec, created_at, proto::new_object_id)?;
            st.stats.terminals_created += 1;
            let t = &st.reg.terminals[i];
            let p = t.active_pane();
            let out = (
                t.id.clone(),
                p.id.clone(),
                p.stream,
                p.size,
                p.cwd.clone(),
                p.command.clone(),
                p.history_limit,
            );
            let data = json!({"terminal": st.terminal_json(t)});
            st.notify("terminals", "terminal.created", &data);
            out
        };
        let program = command.program.clone();
        let cwd = cwd
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let spawned = pty::spawn(&pty::Spec {
            program: &command.program,
            args: &command.args,
            raw_line: launch.raw_line.as_deref(),
            cwd: &cwd,
            env: &launch.env,
            // ADR 0010 §1: how a program (and Claude Code's hooks) knows which pane it runs in.
            host_vars: &[
                ("SUBAK_PANE_ID", &pane_id),
                ("SUBAK_TERMINAL_ID", &tid),
                ("SUBAK_ENDPOINT", &self.endpoint_name),
            ],
            size,
        });
        let started = match spawned {
            Ok(s) => s,
            Err(e) => {
                let mut st = self.lock();
                st.stats.spawn_failures += 1;
                let name = st
                    .reg
                    .get(&tid)
                    .map(|t| t.name.display.clone())
                    .unwrap_or_default();
                let _ = st.reg.apply(&tid, Transition::SpawnFailed { keep });
                if keep {
                    if let Some(t) = st.reg.get_mut(&tid) {
                        t.failure = Some(session::Failure {
                            reason: e.reason.as_str(),
                            message: e.message.clone(),
                        });
                    }
                    st.state_changed(&tid, "starting", None);
                } else {
                    st.removed(&tid, &name, "spawn_failed");
                }
                return Err(spawn_failed(&e, &program, keep, keep.then_some(&*tid)));
            }
        };
        let pty = started.pty;
        let output = pty
            .take_output()
            .expect("a fresh pseudo console has its output end");
        let rt = PaneRt::new(&pane_id, &tid, stream, Arc::clone(&pty));
        let result = {
            let mut st = self.lock();
            if st.stopping || st.reg.get(&tid).is_none() {
                drop(st);
                discard(&pty, output);
                let mut st = self.lock();
                if let Some(i) = st.reg.index_of(&tid) {
                    let t = st.reg.terminals.remove(i);
                    st.removed(&tid, &t.name.display, "host_stopping");
                }
                return Err(IpcError::new(
                    ErrorCode::HostStopping,
                    "the Host began shutting down while the Terminal started",
                ));
            }
            st.reg.apply(&tid, Transition::Spawned)?;
            if let Some(t) = st.reg.get_mut(&tid) {
                let p = t.active_pane_mut();
                p.pid = Some(pty.pid());
                p.executable = Some(started.executable.display().to_string());
            }
            st.out.insert(
                pane_id.clone(),
                PaneOut {
                    screen: Screen::new(
                        size.0,
                        size.1,
                        Limits {
                            lines: history_limit,
                            bytes: screen::HISTORY_BYTES,
                        },
                    ),
                    terminal_id: tid.clone(),
                    seq: 0,
                    input_bytes: 0,
                },
            );
            st.rt.insert(pane_id.clone(), Arc::clone(&rt));
            st.state_changed(&tid, "starting", None);
            let t = st.reg.get(&tid).expect("just registered");
            json!({
                "terminal_id": t.id,
                "window_id": t.windows[0].id,
                "pane_id": pane_id,
                "name": t.name.display,
                "terminal": st.terminal_json(t),
            })
        };
        self.run_pane(&rt, output);
        let _ = self.events.send(Ev::Changed);
        Ok(result)
    }

    /// Starts the threads of a pane that just started: its output reader and its exit waiter.
    /// The reader is registered before the exit waiter can tear the pane down.
    fn run_pane(self: &Arc<Self>, rt: &Arc<PaneRt>, output: Box<dyn Read + Send>) {
        let reader = thread::spawn({
            let host = Arc::clone(self);
            let rt = Arc::clone(rt);
            let mut output = output;
            move || host.read_output(&rt, &mut *output)
        });
        *rt.reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(reader);
        thread::spawn({
            let host = Arc::clone(self);
            let rt = Arc::clone(rt);
            move || {
                rt.pty.wait_exit(None);
                host.pane_exited(&rt);
            }
        });
    }

    /// A pane's root process exited by itself. While other panes of its Terminal still run, the
    /// pane alone ends and leaves the model (tmux closes a pane whose program exits); the last
    /// running pane's exit is the Terminal's own end (removed, or `stopped` with
    /// `remain_on_exit`). A pane someone already ends, or a Terminal already ending, is left to
    /// whoever ends it.
    fn pane_exited(&self, rt: &Arc<PaneRt>) {
        let alone = {
            let st = self.lock();
            if rt.is_claimed() {
                return;
            }
            let Some(t) = st.reg.get(&rt.terminal_id) else {
                return;
            };
            if t.ending.is_some() {
                return;
            }
            let others = st.others_running(t, &[&rt.pane_id]) > 0;
            if others {
                rt.claimed.store(true, Ordering::SeqCst);
            }
            others
        };
        if alone {
            self.end_alone(rt, false, "exited");
        } else {
            self.end_terminal(&rt.terminal_id, EndCause::Exited, false);
        }
    }

    /// Ends every running pane of a Terminal once (stop, kill, Host stop, or the exit of its last
    /// running pane) and applies the end to the Terminal; returns one report per pane. The first
    /// caller claims the end and performs it; later callers wait for the same result (a later,
    /// stronger cause still decides the Terminal's fate, and `immediate` ends the jobs at once).
    /// A pane someone else already ends (its own exit, a `pane.kill`) is waited for.
    fn end_terminal(&self, tid: &str, cause: EndCause, immediate: bool) -> Vec<Value> {
        let (rts, mine) = {
            let mut st = self.lock();
            let Some(t) = st.reg.get(tid) else {
                return Vec::new();
            };
            let rts: Vec<Arc<PaneRt>> = t
                .panes()
                .filter_map(|p| st.rt.get(&p.id).cloned())
                .collect();
            let (state, ending) = (t.state, t.ending);
            if let Some(prev) = ending {
                if cause > prev
                    && let Some(t) = st.reg.get_mut(tid)
                {
                    t.ending = Some(cause);
                }
                (rts, false)
            } else if rts.is_empty() || state != session::State::Running {
                return Vec::new();
            } else {
                if let Some(t) = st.reg.get_mut(tid) {
                    t.ending = Some(cause);
                }
                if st.reg.apply(tid, Transition::EndBegins).is_ok() {
                    st.state_changed(tid, "running", Some(cause));
                }
                (rts, true)
            }
        };
        if !mine {
            if immediate {
                for r in &rts {
                    r.terminate();
                }
            }
            return rts
                .iter()
                .map(|r| r.wait_done())
                .filter(|r| !r.is_null())
                .collect();
        }
        let ended: Vec<(Value, bool)> = thread::scope(|s| {
            let handles: Vec<_> = rts
                .iter()
                .map(|rt| s.spawn(move || self.claim_and_teardown(rt, immediate)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or((Value::Null, false)))
                .collect()
        });
        self.finish_terminal(tid, &rts, &ended);
        for (rt, (report, own)) in rts.iter().zip(&ended) {
            if *own {
                rt.set_done(report.clone());
            }
        }
        ended
            .into_iter()
            .map(|(r, _)| r)
            .filter(|r| !r.is_null())
            .collect()
    }

    /// Claims a pane's teardown and performs it (`true`), or waits for whoever claimed it first.
    fn claim_and_teardown(&self, rt: &PaneRt, immediate: bool) -> (Value, bool) {
        let mine = {
            let _st = self.lock();
            !rt.claimed.swap(true, Ordering::SeqCst)
        };
        if !mine {
            if immediate {
                rt.terminate();
            }
            return (rt.wait_done(), false);
        }
        (self.teardown(rt, immediate), true)
    }

    /// Ends one pane that the caller claimed (under the lock) while other panes of its Terminal
    /// keep running (`pane.kill`, `window.kill`, or its own exit), and takes it out of the model.
    fn end_alone(&self, rt: &PaneRt, immediate: bool, reason: &str) -> Value {
        let report = self.teardown(rt, immediate);
        self.finish_pane(rt, &report, reason);
        rt.set_done(report.clone());
        report
    }

    /// Graceful, then forced (ADR 0004, ARCHITECTURE §5): `ClosePseudoConsole` sends
    /// `CTRL_CLOSE_EVENT`, the processes get the grace period, and whatever is left in the job
    /// is terminated. `immediate` terminates the job first. All output is drained before the
    /// report, so `pane.exited` follows the pane's last output.
    fn teardown(&self, rt: &PaneRt, immediate: bool) -> Value {
        let processes = rt.pty.processes();
        if immediate {
            rt.terminate();
        }
        // On Windows 10 ClosePseudoConsole blocks until conhost exits, while the reader drains.
        let closer = thread::spawn({
            let pty = Arc::clone(&rt.pty);
            move || pty.close()
        });
        let mut exit = rt.pty.wait_exit(Some(GRACE));
        if exit.is_some() {
            // The job's count lags the root's exit (see JOB_SETTLE): let it settle first.
            job_drained(&*rt.pty, JOB_SETTLE);
        }
        if exit.is_none() || rt.pty.processes() > 0 {
            rt.terminate();
            exit = rt.pty.wait_exit(Some(GRACE));
        }
        join_bounded(closer, GRACE);
        let reader = rt
            .reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(r) = reader {
            join_bounded(r, GRACE);
        }
        json!({
            "terminal_id": rt.terminal_id,
            "pane_id": rt.pane_id,
            "pid": rt.pty.pid(),
            "exit_code": exit,
            "processes_in_job": processes,
            "forced": rt.forced.load(Ordering::SeqCst),
            "job_processes_left": rt.pty.processes(),
        })
    }

    /// What every ended pane does to the Host: its runtime goes, its clients are detached, and
    /// `pane.exited` follows its last output.
    fn release_pane(st: &mut State, rt: &PaneRt, report: &Value) {
        st.rt.remove(&rt.pane_id);
        for c in st.conns.values_mut() {
            c.attached.remove(&rt.stream);
        }
        let exit_code = report["exit_code"]
            .as_u64()
            .and_then(|c| u32::try_from(c).ok());
        let data = json!({
            "pane_id": rt.pane_id, "terminal_id": rt.terminal_id, "pid": rt.pty.pid(),
            "exit_code": exit_code,
        });
        st.notify("panes", "pane.exited", &data);
        // Step 27: a pane that ended has no Claude session any more (ADR 0061).
        let before = st.claude_of(&rt.terminal_id);
        if let Some(sessions) = st.claude.remove(&rt.pane_id) {
            for (sid, s) in sessions {
                let data = json!({"pane_id": rt.pane_id, "terminal_id": rt.terminal_id,
                    "session_id": sid, "state": "none", "previous_state": s.state,
                    "event": "pane.exited"});
                st.notify("claude", "claude.state_changed", &data);
            }
            st.claude_changed(&rt.terminal_id, before);
        }
    }

    /// Applies the end of a Terminal to the model: releases the panes it ended, records how it
    /// ended, and moves it to `stopped` (its windows, panes, screens and histories stay for
    /// `pane.capture`) or removes it, with the events, in one critical section.
    fn finish_terminal(&self, tid: &str, rts: &[Arc<PaneRt>], ended: &[(Value, bool)]) {
        {
            let mut st = self.lock();
            for (rt, (report, own)) in rts.iter().zip(ended) {
                if *own {
                    Self::release_pane(&mut st, rt, report);
                }
            }
            let Some(t) = st.reg.get_mut(tid) else {
                drop(st);
                let _ = self.events.send(Ev::Changed);
                return;
            };
            let cause = t.ending.take().unwrap_or(EndCause::Exited);
            // How it ended: the active pane's report when it is among them, else the first.
            let active = t.active_pane().id.clone();
            let reports: Vec<&Value> = ended
                .iter()
                .map(|(r, _)| r)
                .filter(|r| !r.is_null())
                .collect();
            let main = reports
                .iter()
                .find(|r| r["pane_id"] == active.as_str())
                .or_else(|| reports.first());
            t.last_exit = Some(LastExit {
                pid: main
                    .and_then(|r| r["pid"].as_u64())
                    .and_then(|p| u32::try_from(p).ok()),
                exit_code: main
                    .and_then(|r| r["exit_code"].as_u64())
                    .and_then(|c| u32::try_from(c).ok()),
                forced: reports.iter().any(|r| r["forced"] == true),
                cause,
            });
            for w in &mut t.windows {
                for p in &mut w.panes {
                    p.pid = None;
                }
            }
            let (name, remain) = (t.name.display.clone(), t.remain_on_exit);
            match st.reg.apply(
                tid,
                Transition::Ended {
                    cause,
                    remain_on_exit: remain,
                },
            ) {
                Ok(Some(_)) => st.state_changed(tid, "stopping", Some(cause)),
                Ok(None) => st.removed(tid, &name, cause.as_str()),
                Err(_) => {}
            }
        }
        let _ = self.events.send(Ev::Changed);
    }

    /// Applies the end of one pane whose Terminal goes on (Step 21): releases it, takes it out of
    /// the model (its window too when it was the last pane there) with its screen, and announces
    /// the new selection when the active pane or window went.
    fn finish_pane(&self, rt: &PaneRt, report: &Value, reason: &str) {
        let resizes = {
            let mut st = self.lock();
            Self::release_pane(&mut st, rt, report);
            let removal = st.reg.get_mut(&rt.terminal_id).and_then(|t| {
                if let Some(p) = t.pane_mut(&rt.pane_id) {
                    p.pid = None;
                }
                t.remove_pane(&rt.pane_id)
            });
            match removal {
                Some(r) => {
                    st.drop_screen(&rt.pane_id);
                    st.pane_removed(&rt.terminal_id, &rt.pane_id, &r, reason);
                    // Step 22: its space went to a neighbour (ADR 0056).
                    if !r.window_removed {
                        st.layout_event(&rt.terminal_id, &r.window_id);
                    }
                    st.resized(r.resized)
                }
                None => Vec::new(),
            }
        };
        let _ = apply_resizes(&resizes);
        let _ = self.events.send(Ev::Changed);
    }

    /// Ends every live Terminal in parallel (Host stop or idle exit); returns the pane reports.
    fn end_all(&self) -> Vec<Value> {
        let tids: Vec<String> = self
            .lock()
            .reg
            .terminals
            .iter()
            .filter(|t| t.state.is_live())
            .map(|t| t.id.clone())
            .collect();
        thread::scope(|s| {
            let handles: Vec<_> = tids
                .iter()
                .map(|t| s.spawn(move || self.end_terminal(t, EndCause::HostStop, false)))
                .collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .flatten()
                .collect()
        })
    }

    /// The pane's reader thread: drains the pseudo terminal continuously (a full output pipe
    /// would block the pane's programs), feeds the pane's terminal model, and fans each chunk
    /// out to the attached clients without ever waiting for one. Model, sequence number and
    /// fan-out change under the one lock an attach takes, so a snapshot and the live output
    /// after it meet exactly (ADR 0030).
    fn read_output(&self, rt: &PaneRt, out: &mut dyn Read) {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match out.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            let mut guard = self.lock();
            let st = &mut *guard;
            let Some(pane) = st.out.get_mut(&rt.pane_id) else {
                return;
            };
            let mark = pane.screen.history().total_lines;
            let bells = pane.screen.bells();
            pane.screen.feed(&buf[..n]);
            let rang = pane.screen.bells() > bells;
            let seq = pane.seq;
            pane.seq += n as u64;
            let frame: Arc<[u8]> = proto::encode_data(rt.stream, seq, 0, &buf[..n]).into();
            for c in st.conns.values_mut() {
                let Some(att) = c.attached.get_mut(&rt.stream) else {
                    continue;
                };
                // Never block the pane on a client: one that lags skips output until its queue
                // drained, then gets a fresh snapshot (`pane.resync`).
                if att.lag.is_some() {
                    st.stats.output_frames_skipped += 1;
                } else if let Err(since) = c.sink.push_output(rt.stream, Arc::clone(&frame), mark) {
                    att.lag = Some(since);
                    st.stats.output_frames_skipped += 1;
                }
            }
            // Step 25, tmux `monitor-activity` / `monitor-bell`: output or a bell in a window
            // that is not shown flags it once, until it is shown.
            let alert = st.reg.get_mut(&rt.terminal_id).and_then(|t| {
                let active = t.active_window;
                let (wi, w) = t
                    .windows
                    .iter_mut()
                    .enumerate()
                    .find(|(_, w)| w.panes.iter().any(|p| p.id == rt.pane_id))?;
                if wi == active {
                    return None;
                }
                let kind = if rang && w.options.monitor_bell && !w.bell {
                    w.bell = true;
                    "bell"
                } else if w.options.monitor_activity && !w.activity {
                    w.activity = true;
                    "activity"
                } else {
                    return None;
                };
                Some((t.id.clone(), w.id.clone(), kind))
            });
            if let Some((tid, wid, kind)) = alert {
                let data = json!({"terminal_id": tid, "window_id": wid, "pane_id": rt.pane_id, "kind": kind});
                st.notify("terminals", "window.alert", &data);
            }
        }
    }

    /// Brings the lagging attachments of `conn` back once its queue has drained: `pane.resync`
    /// (to a connection subscribed to `panes`), then a snapshot carrying the history lines the
    /// client missed, then live output from the snapshot's sequence number on.
    fn resync(&self, conn: u64) {
        let mut guard = self.lock();
        let st = &mut *guard;
        let Some(c) = st.conns.get_mut(&conn) else {
            return;
        };
        let lagging: Vec<(u32, u64)> = c
            .attached
            .iter()
            .filter_map(|(s, a)| a.lag.map(|m| (*s, m)))
            .collect();
        for (stream, since) in lagging {
            let Some((t, p)) = st.reg.pane_of_stream(stream) else {
                continue;
            };
            let (tid, pid) = (t.id.clone(), p.id.clone());
            let Some(out) = st.out.get(&pid) else {
                continue;
            };
            let snap = out.screen.snapshot(HistoryRange::Since(since), true);
            let (seq, mark) = (out.seq, out.screen.history().total_lines);
            if c.topics.iter().any(|t| t == "panes") {
                c.event(
                    "pane.resync",
                    &json!({"pane_id": pid, "terminal_id": tid, "stream_id": stream, "seq": seq}),
                );
            }
            let mut lag = None;
            for chunk in snap.chunks(proto::MAX_DATA) {
                let frame: Arc<[u8]> = proto::encode_data(stream, seq, FLAG_SNAPSHOT, chunk).into();
                if let Err(m) = c.sink.push_output(stream, frame, mark) {
                    lag = Some(m.min(since));
                    break;
                }
            }
            if let Some(a) = c.attached.get_mut(&stream) {
                a.lag = lag;
            }
            st.stats.lagging_resyncs += 1;
            st.stats.snapshots += 1;
            st.stats.snapshot_bytes += snap.len() as u64;
        }
    }

    fn reject(&self, sink: &Sink, why: Reject, err: &IpcError) -> Option<u64> {
        {
            let mut st = self.lock();
            let s = &mut st.stats;
            match why {
                Reject::Identity => s.rejected_identity += 1,
                Reject::Limit => s.rejected_limit += 1,
                Reject::Handshake => s.rejected_handshake += 1,
                Reject::Stopping => s.rejected_stopping += 1,
            }
        }
        sink.last(&proto::connection_error(err));
        None
    }

    fn serve_conn(self: Arc<Self>, pipe: &Arc<Pipe>) {
        let queue = Arc::new(Queue::default());
        let sink = Sink {
            queue: Arc::clone(&queue),
            pipe: Arc::clone(pipe),
        };
        let writer = thread::spawn({
            let pipe = Arc::clone(pipe);
            // The writer holds the Host only weakly, so a stuck client never keeps it alive.
            let host = Arc::downgrade(&self);
            move || {
                let mut buf = Vec::new();
                while let Some(batch) = queue.take() {
                    // One write for a run of small frames (the client still reads them one
                    // by one); a single frame is written as it is.
                    let ok = match batch.as_slice() {
                        [] => true,
                        [one] => pipe.write_all_bytes(&one.bytes).is_ok(),
                        _ => {
                            buf.clear();
                            for o in &batch {
                                buf.extend_from_slice(&o.bytes);
                            }
                            pipe.write_all_bytes(&buf).is_ok()
                        }
                    };
                    if !ok {
                        pipe.cancel();
                        return;
                    }
                    for a in batch.into_iter().filter_map(|o| o.ack) {
                        let _ = a.send(());
                    }
                    // A lagging stream gets its snapshot once this client has caught up.
                    if queue.resync_due()
                        && let Some(h) = host.upgrade()
                    {
                        h.resync(queue.conn.load(Ordering::SeqCst));
                    }
                }
            }
        });
        let hs = catch_unwind(AssertUnwindSafe(|| self.handshake(pipe, &sink)));
        {
            let mut st = self.lock();
            st.pending = st.pending.saturating_sub(1);
            if hs.is_err() {
                st.stats.panics += 1;
            }
        }
        if let Ok(Some(id)) = hs {
            let r = catch_unwind(AssertUnwindSafe(|| self.requests(id, pipe, &sink)));
            let mut st = self.lock();
            if r.is_err() {
                st.stats.panics += 1;
            }
            // A client that disappears owns nothing: only its subscriptions and attachments go.
            st.conns.remove(&id);
        }
        let _ = self.events.send(Ev::Changed);
        // The writer sends what is queued (e.g. a last error message), then ends.
        sink.queue.close();
        drop(sink);
        // Unblocks a writer stuck on a client that stopped reading.
        let until = Instant::now() + Duration::from_secs(2);
        while !writer.is_finished() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        pipe.cancel();
        let _ = writer.join();
    }

    fn handshake(&self, pipe: &Arc<Pipe>, sink: &Sink) -> Option<u64> {
        let over_limit = {
            let mut st = self.lock();
            st.stats.connections_total += 1;
            st.conns.len() + st.pending > MAX_CONNECTIONS
        };
        if over_limit {
            return self.reject(
                sink,
                Reject::Limit,
                &IpcError::new(
                    ErrorCode::Busy,
                    format!("the Host serves at most {MAX_CONNECTIONS} connections"),
                ),
            );
        }
        let deadline = Instant::now() + proto::HANDSHAKE_TIMEOUT;
        let msg = match proto::read_frame(&mut &**pipe, Some(deadline), |_| deadline) {
            Ok(Some(Frame::Control(m))) => m,
            Ok(Some(Frame::Data { .. })) => {
                let e = IpcError::new(ErrorCode::ProtocolError, "expected hello, got a data frame");
                return self.reject(sink, Reject::Handshake, &e);
            }
            Ok(None) => return None,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                let e = IpcError::new(
                    ErrorCode::Timeout,
                    format!("no hello within {} s", proto::HANDSHAKE_TIMEOUT.as_secs()),
                );
                return self.reject(sink, Reject::Handshake, &e);
            }
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                let e = IpcError::new(ErrorCode::ProtocolError, e.to_string());
                return self.reject(sink, Reject::Handshake, &e);
            }
            Err(_) => return None,
        };
        // Who is on the other end: the token of this very connection (ADR 0005, ADR 0019).
        if !pipe.client_user_sid().is_ok_and(|sid| sid == self.sid) {
            let e = IpcError::new(
                ErrorCode::Unauthorized,
                "the client does not run as the Host's user",
            );
            return self.reject(sink, Reject::Identity, &e);
        }
        let hello = match proto::parse_hello(&msg) {
            Ok(h) => h,
            Err(e) => return self.reject(sink, Reject::Handshake, &e),
        };
        let Some(protocol) = proto::negotiate(hello.min, hello.max) else {
            let e = proto::unsupported(hello.min, hello.max);
            return self.reject(sink, Reject::Handshake, &e);
        };
        let mut st = self.lock();
        if st.stopping {
            drop(st);
            let e = IpcError::new(ErrorCode::HostStopping, "the Host is shutting down");
            return self.reject(sink, Reject::Stopping, &e);
        }
        let id = st.next_conn;
        st.next_conn += 1;
        sink.queue.conn.store(id, Ordering::SeqCst);
        // Queued under the lock, so no event or output can precede the welcome.
        if !sink.control(&proto::welcome(protocol, self.pid, &self.started_at)) {
            return None;
        }
        st.conns.insert(
            id,
            Conn {
                sink: sink.clone(),
                pid: pipe.client_pid(),
                kind: hello.kind,
                name: hello.name,
                version: hello.version,
                attached: BTreeMap::new(),
                topics: Vec::new(),
                event_seq: 0,
            },
        );
        Some(id)
    }

    fn violation(&self, sink: &Sink, message: &str) {
        self.lock().stats.protocol_errors += 1;
        sink.last(&proto::connection_error(&IpcError::new(
            ErrorCode::ProtocolError,
            message,
        )));
    }

    fn answer(&self, sink: &Sink, id: u64, result: Result<Value, IpcError>) {
        if let Err(e) = &result {
            *self.lock().stats.errors.entry(e.code.as_str()).or_default() += 1;
        }
        sink.control(&proto::response(id, result));
    }

    fn requests(self: &Arc<Self>, id: u64, pipe: &Arc<Pipe>, sink: &Sink) {
        loop {
            let frame = match proto::read_frame(&mut &**pipe, None, |t| t + proto::FRAME_TIMEOUT) {
                Ok(Some(f)) => f,
                Ok(None) => return,
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    return self.violation(sink, &e.to_string());
                }
                Err(_) => return,
            };
            match frame {
                Frame::Data {
                    stream,
                    flags,
                    bytes,
                    ..
                } => {
                    if flags != FLAG_INPUT {
                        return self.violation(sink, "clients send only input data frames");
                    }
                    self.input(id, stream, &bytes);
                }
                Frame::Control(m) => {
                    let req = match proto::parse_request(&m) {
                        Ok(r) => r,
                        Err(Refusal::Fatal(msg)) => return self.violation(sink, &msg),
                        Err(Refusal::Reply(rid, e)) => {
                            self.answer(sink, rid, Err(e));
                            continue;
                        }
                    };
                    let kind = {
                        let mut st = self.lock();
                        st.stats.requests_total += 1;
                        st.conns.get(&id).map(|c| c.kind)
                    };
                    let Some(kind) = kind else { return };
                    let reply = match proto::authorize(kind, &req.method) {
                        Err(e) => Reply::Now(Err(e)),
                        Ok(_) => catch_unwind(AssertUnwindSafe(|| self.handle(id, &req)))
                            .unwrap_or_else(|_| {
                                self.lock().stats.panics += 1;
                                Reply::Now(Err(IpcError::new(
                                    ErrorCode::InternalError,
                                    "the request failed inside the Host; the Host continues",
                                )))
                            }),
                    };
                    if let Reply::Now(r) = reply {
                        self.answer(sink, req.id, r);
                    }
                }
            }
        }
    }

    fn input(&self, conn: u64, stream: u32, bytes: &[u8]) {
        let rt = {
            let mut st = self.lock();
            let interactive = st
                .conns
                .get(&conn)
                .and_then(|c| c.attached.get(&stream))
                .is_some_and(|a| a.interactive);
            let rt = st.rt_of_stream(stream).cloned().filter(|r| {
                interactive
                    && st
                        .reg
                        .get(&r.terminal_id)
                        .is_some_and(|t| t.state == session::State::Running && t.ending.is_none())
            });
            match (&rt, st.stopping) {
                (Some(r), false) => {
                    if let Some(o) = st.out.get_mut(&r.pane_id) {
                        o.input_bytes += bytes.len() as u64;
                    }
                }
                _ => st.stats.input_frames_dropped += 1,
            }
            rt.filter(|_| !st.stopping)
        };
        // Only an interactively attached cli/gui connection reaches a pane; nothing else does.
        if let Some(r) = rt {
            // Step 25, tmux `synchronize-panes`: the window's other running panes get it too.
            let others = self.synced(&r);
            let _ = r.pty.write_input(bytes);
            for o in others {
                let _ = o.pty.write_input(bytes);
            }
        }
    }

    fn handle(self: &Arc<Self>, conn: u64, req: &Request) -> Reply {
        let method = req.method.as_str();
        if self.lock().stopping && !matches!(method, "host.status" | "host.health") {
            return Reply::Now(Err(IpcError::new(
                ErrorCode::HostStopping,
                "the Host is shutting down",
            )));
        }
        let p = &req.params;
        match method {
            "host.health" => Reply::Now(Ok(self.health())),
            "host.status" => Reply::Now(Ok(self.status())),
            "host.stats" => Reply::Now(Ok(self.stats())),
            "host.stop" => self.stop(conn, req),
            "events.subscribe" => Reply::Now(self.subscribe(conn, p)),
            "pane.attach" => self.attach(conn, req),
            "pane.detach" => Reply::Now(self.detach(conn, p)),
            "client.detach" => Reply::Now(self.detach_clients(conn, p)),
            "pane.resize" => Reply::Now(self.resize(conn, p)),
            "pane.capture" => Reply::Now(self.capture(p)),
            "pane.clear_history" => Reply::Now(self.clear_history(p)),
            "terminal.list" => Reply::Now(self.list(p)),
            "terminal.info" => Reply::Now(self.info(p)),
            "terminal.create" => Reply::Now(self.create_request(p)),
            "terminal.rename" => Reply::Now(self.rename(p)),
            "terminal.stop" => Reply::Now(self.end_request(p, Op::Stop)),
            "terminal.kill" => Reply::Now(self.end_request(p, Op::Kill)),
            "terminal.remove" => Reply::Now(self.end_request(p, Op::Remove)),
            // Step 21 (ADR 0054): internal windows and panes.
            "window.create" => Reply::Now(self.create_in(p, method)),
            "pane.create" => Reply::Now(self.create_in(p, method)),
            // Step 22 (ADR 0056): splits and the layout.
            "pane.split" => Reply::Now(self.create_in(p, method)),
            "pane.adjust" => Reply::Now(self.layout_op(p, LayoutOp::Adjust)),
            "pane.swap" => Reply::Now(self.layout_op(p, LayoutOp::Swap)),
            "pane.zoom" => Reply::Now(self.layout_op(p, LayoutOp::Zoom)),
            "window.rotate" => Reply::Now(self.layout_op(p, LayoutOp::Rotate)),
            // Step 24 (ADR 0058): keys, paste buffers, moving panes between windows.
            "pane.send_keys" => Reply::Now(self.send_keys(p)),
            "pane.break" => Reply::Now(self.break_pane(p)),
            "pane.join" => Reply::Now(self.join_pane(p)),
            "buffer.set" => Reply::Now(self.buffer_set(p)),
            "buffer.list" => Reply::Now(Ok(self.buffer_list())),
            "buffer.get" => Reply::Now(self.buffer_get(p)),
            "buffer.delete" => Reply::Now(self.buffer_delete(p)),
            "buffer.paste" => Reply::Now(self.buffer_paste(p)),
            // Step 25 (ADR 0059): window options and hooks.
            "window.set_option" => Reply::Now(self.set_option(p)),
            "window.options" => Reply::Now(self.window_options(p)),
            "hook.set" => Reply::Now(self.hook_set(p)),
            "hook.list" => Reply::Now(Ok(self.hook_list())),
            // Step 29 (ADR 0063): the durable state.
            "state.status" => Reply::Now(Ok(self.state_status())),
            "state.forget" => Reply::Now(self.state_forget(p)),
            "state.set_restore" => Reply::Now(self.state_set_restore(p)),
            // Step 27 (ADR 0061).
            "claude.report" => Reply::Now(self.claude_report(conn, p)),
            "claude.status" => Reply::Now(self.claude_status(p)),
            "window.select_layout" => Reply::Now(self.layout_op(p, LayoutOp::Preset)),
            "window.list" => Reply::Now(self.topology(p, false)),
            "pane.list" => Reply::Now(self.topology(p, true)),
            "window.select" => Reply::Now(self.select(p, false)),
            "pane.select" => Reply::Now(self.select(p, true)),
            "window.rename" => Reply::Now(self.window_rename(p)),
            "window.kill" => Reply::Now(self.close(p, true)),
            "pane.kill" => Reply::Now(self.close(p, false)),
            other => Reply::Now(Err(IpcError::new(
                ErrorCode::UnknownRequest,
                format!("unknown method `{other}`"),
            ))),
        }
    }

    fn uptime_s(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    fn health(&self) -> Value {
        let st = self.lock();
        json!({
            "healthy": !st.stopping,
            "pid": self.pid,
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": proto::PROTOCOL_MAX,
            "uptime_s": self.uptime_s(),
            "stopping": st.stopping,
            "terminals": st.live_terminals(),
            "clients": st.conns.len(),
        })
    }

    /// `host.status.pane`: the pane of the Terminal started with `--shell`, while it exists
    /// (the Step 09 shape).
    fn initial_pane(&self, st: &State) -> Value {
        let Some(t) = st.initial.as_deref().and_then(|id| st.reg.get(id)) else {
            return Value::Null;
        };
        let p = t.active_pane();
        let live = st.live(p);
        let running = t.state.is_live() && live.running;
        let pid = p.pid.or_else(|| t.last_exit.as_ref().and_then(|e| e.pid));
        let line = std::iter::once(redact::program(&p.command.program))
            .chain(redact::args(&p.command.args))
            .collect::<Vec<_>>()
            .join(" ");
        json!({
            "id": p.id,
            "terminal_id": t.id,
            "pid": pid,
            "command": line,
            "running": running,
            "exit_code": t.last_exit.as_ref().and_then(|e| e.exit_code),
            "size": {"cols": p.size.0, "rows": p.size.1},
            "job_processes": live.processes,
            "output_bytes": live.output_bytes,
        })
    }

    fn status(&self) -> Value {
        let st = self.lock();
        let attached = st.conns.values().filter(|c| !c.attached.is_empty()).count();
        let mut conns: Vec<(&u64, &Conn)> = st.conns.iter().collect();
        conns.sort_unstable_by_key(|(id, _)| **id);
        let clients: Vec<Value> = conns
            .into_iter()
            .map(|(id, c)| {
                json!({
                    // Step 12: the id `client.detach` (`subak detach --client`) takes.
                    "id": id,
                    "pid": c.pid, "kind": c.kind.as_str(), "name": c.name, "version": c.version,
                    "attached": !c.attached.is_empty(),
                    "interactive": c.attached.values().any(|a| a.interactive),
                    "streams": c.attached.keys().collect::<Vec<_>>(),
                    "topics": c.topics,
                })
            })
            .collect();
        json!({
            "protocol": proto::PROTOCOL_MAX,
            "endpoint": self.endpoint,
            "host": {
                "pid": self.pid,
                "version": env!("CARGO_PKG_VERSION"),
                "started_at": self.started_at,
                "uptime_s": self.uptime_s(),
                "session_id": self.session,
                "job_breakaway": self.job_breakaway,
                "in_job": self.in_job,
                "job": self.job.map(|j| json!({
                    "kill_on_close": j.kill_on_close,
                    "breakaway_ok": j.breakaway_ok,
                    "silent_breakaway_ok": j.silent_breakaway_ok,
                })),
                "stopping": st.stopping,
                "idle_exit_s": self.idle_exit.as_secs(),
                "environment": self.environment,
            },
            "terminals": st.live_terminals(),
            "terminals_total": st.reg.terminals.len(),
            "pane": self.initial_pane(&st),
            "clients": {"connected": st.conns.len(), "attached": attached, "list": clients},
            // Step 13 (docs/SCROLLBACK.md): what the Host keeps of terminal output.
            "scrollback": st.scrollback_json(),
        })
    }

    /// Local metrics (ADR 0012): numbers and enumerations only.
    fn stats(&self) -> Value {
        let mem = win::memory();
        let st = self.lock();
        let s = &st.stats;
        let out: u64 = st.out.values().map(|o| o.seq).sum::<u64>() + s.ended_output_bytes;
        let input: u64 = st.out.values().map(|o| o.input_bytes).sum::<u64>() + s.ended_input_bytes;
        json!({
            "host": {
                "uptime_s": self.uptime_s(),
                "handle_count": win::handle_count(),
                "memory": {
                    "working_set_bytes": mem.working_set,
                    "private_bytes": mem.private,
                    "peak_private_bytes": mem.peak_private,
                },
            },
            "terminals": st.live_terminals(),
            "terminals_total": st.reg.terminals.len(),
            "panes": st.rt.len(),
            "clients": st.conns.len(),
            "pane": {"output_bytes_total": out, "input_bytes_total": input},
            "scrollback": {
                "panes": st.out.len(),
                "lines": st.out.values().map(|o| o.screen.history().lines).sum::<usize>(),
                "bytes": st.out.values().map(|o| o.screen.history().bytes).sum::<usize>(),
                "memory_bytes": st.out.values().map(|o| o.screen.memory_bytes()).sum::<usize>(),
                "evicted_lines_total": st.out.values().map(|o| o.screen.history().evicted_lines).sum::<u64>() + s.ended_evicted_lines,
                "cleared_lines_total": st.out.values().map(|o| o.screen.history().cleared_lines).sum::<u64>() + s.ended_cleared_lines,
                "snapshots_total": s.snapshots,
                "snapshot_bytes_total": s.snapshot_bytes,
                "captures_total": s.captures,
            },
            "sessions": {
                "created_total": s.terminals_created,
                "spawn_failures_total": s.spawn_failures,
                "removed_total": s.terminals_removed,
            },
            "ipc": {
                "connections_total": s.connections_total,
                "rejected": {
                    "identity": s.rejected_identity,
                    "limit": s.rejected_limit,
                    "handshake": s.rejected_handshake,
                    "stopping": s.rejected_stopping,
                },
                "requests_total": s.requests_total,
                "errors_total": s.errors,
                "protocol_errors": s.protocol_errors,
                "panics": s.panics,
                "input_frames_dropped": s.input_frames_dropped,
                "lagging_clients_dropped": s.lagging_clients_dropped,
                "lagging_resyncs": s.lagging_resyncs,
                "output_frames_skipped": s.output_frames_skipped,
                "hooks_run": s.hooks_run,
                "hooks_failed": s.hooks_failed,
                "claude_reports": s.claude_reports,
                "claude_refused": s.claude_refused,
            },
        })
    }

    fn stop(&self, conn: u64, req: &Request) -> Reply {
        let (dry_run, kill) = match (
            flag(&req.params, "dry_run"),
            flag(&req.params, "kill_terminals"),
        ) {
            (Ok(d), Ok(k)) => (d, k),
            (Err(e), _) | (_, Err(e)) => return Reply::Now(Err(e)),
        };
        let (panes, would_end) = {
            let st = self.lock();
            let live: Vec<&session::Terminal> = st
                .reg
                .terminals
                .iter()
                .filter(|t| t.state.is_live())
                .collect();
            let panes: Vec<String> = live
                .iter()
                .flat_map(|t| t.panes())
                .filter(|p| st.rt.contains_key(&p.id))
                .map(|p| p.id.clone())
                .collect();
            let would_end: Vec<Value> = live
                .iter()
                .map(|t| {
                    let p = t.active_pane();
                    // Step 21: the processes of every pane, busy when any pane is.
                    let lives: Vec<(&session::Pane, session::Live)> =
                        t.panes().map(|p| (p, st.live(p))).collect();
                    json!({
                        "terminal_id": t.id, "name": t.name.display, "pane_id": p.id,
                        "pid": p.pid,
                        "processes": lives.iter().map(|(_, l)| l.processes).sum::<u32>(),
                        "busy": lives.iter().any(|(p, l)| session::busy(&p.command.program, l.processes)),
                    })
                })
                .collect();
            (panes, would_end)
        };
        if dry_run {
            return Reply::Now(Ok(json!({
                "dry_run": true, "would_kill": panes, "would_end": would_end,
                "would_stop": self.status(),
            })));
        }
        if !panes.is_empty() && !kill {
            return Reply::Now(Err(IpcError::new(
                ErrorCode::Busy,
                "the Host owns live Terminals; set kill_terminals to end them",
            )
            .details(json!({ "live": panes, "terminals": would_end }))));
        }
        {
            let mut st = self.lock();
            if st.stopping {
                return Reply::Now(Err(IpcError::new(
                    ErrorCode::HostStopping,
                    "the Host is already stopping",
                )));
            }
            st.stopping = true;
        }
        let _ = self.events.send(Ev::Stop { conn, id: req.id });
        Reply::Queued // answered when the panes have ended
    }

    fn subscribe(&self, conn: u64, params: &Value) -> Result<Value, IpcError> {
        let bad = || {
            IpcError::param(
                "topics",
                "topics must be a list of: terminals, panes, claude, host",
            )
        };
        let topics: Vec<String> = params["topics"]
            .as_array()
            .ok_or_else(bad)?
            .iter()
            .map(|t| {
                t.as_str()
                    .filter(|t| proto::TOPICS.contains(t))
                    .map(String::from)
            })
            .collect::<Option<_>>()
            .ok_or_else(bad)?;
        if let Some(c) = self.lock().conns.get_mut(&conn) {
            c.topics.clone_from(&topics);
        }
        Ok(json!({ "topics": topics }))
    }

    /// `pane.attach`: a target (any level; a Terminal or window means its active pane), or none
    /// when the Host has exactly one running pane.
    fn attach(&self, conn: u64, req: &Request) -> Reply {
        let p = &req.params;
        let interactive = match &p["interactive"] {
            Value::Null => true,
            Value::Bool(b) => *b,
            _ => {
                return Reply::Now(Err(IpcError::param(
                    "interactive",
                    "interactive must be true or false",
                )));
            }
        };
        let size = match size_param(&p["size"], "size") {
            Ok(s) if interactive => s,
            Ok(_) => None, // a read-only viewer never resizes the pane
            Err(e) => return Reply::Now(Err(e)),
        };
        // Step 13: how many history lines the snapshot carries (default: all that are kept).
        let history = match &p["history_lines"] {
            Value::Null => usize::MAX,
            v => match v.as_u64().and_then(|n| usize::try_from(n).ok()) {
                Some(n) => n,
                None => {
                    return Reply::Now(Err(IpcError::param(
                        "history_lines",
                        "history_lines must be a non-negative integer",
                    )));
                }
            },
        };
        let (rt, resize) = {
            let mut st = self.lock();
            let rt = match self.attach_target(&st, &p["target"]) {
                Ok(rt) => rt,
                Err(e) => return Reply::Now(Err(e)),
            };
            // The model takes the new size first, as a terminal does before it tells its PTY;
            // output still formatted for the old size is repainted by ConPTY after the resize.
            // The same size is no change: no model resize, and no PTY resize (ConPTY would
            // repaint the whole screen after the snapshot for nothing).
            // Step 22: a client's size is its window's; the panes share it by the layout.
            let (tid, pid) = (rt.terminal_id.clone(), rt.pane_id.clone());
            let resizes = size
                .map(|s| st.resize_window(&tid, &pid, s))
                .unwrap_or_default();
            (rt, resizes)
        };
        if let Err(e) = apply_resizes(&resize) {
            return Reply::Now(Err(e));
        }
        // Response, snapshot and the start of live output are queued under the lock the reader
        // takes for every chunk, so the snapshot reflects exactly the output before `seq` and
        // live output continues at `seq`: nothing lost, duplicated or reordered (ADR 0030).
        let mut guard = self.lock();
        let st = &mut *guard;
        // Re-checked under the lock that ends attachments, so no client stays attached to a
        // pane that ended between the check above and here.
        if !st.rt.contains_key(&rt.pane_id) {
            return Reply::Now(Err(IpcError::new(
                ErrorCode::InvalidState,
                "the pane's process has exited",
            )));
        }
        let Some(t) = st.reg.get_mut(&rt.terminal_id) else {
            return Reply::Queued;
        };
        let Some(pane) = t.pane_mut(&rt.pane_id) else {
            return Reply::Queued;
        };
        let pane = pane.clone();
        let Some(out) = st.out.get(&rt.pane_id) else {
            return Reply::Queued;
        };
        let info = out.screen.history();
        let range = if history == 0 {
            HistoryRange::None
        } else {
            HistoryRange::Last(history)
        };
        let snap = out.screen.snapshot(range, false);
        let (seq, alternate, (cols, rows)) = (out.seq, out.screen.alternate(), out.screen.size());
        let Some(c) = st.conns.get_mut(&conn) else {
            return Reply::Queued;
        };
        let frames = snap.len().div_ceil(proto::MAX_DATA);
        let result = json!({
            "stream_id": rt.stream,
            "pane_id": rt.pane_id,
            "terminal_id": rt.terminal_id,
            "seq": seq,
            "interactive": interactive,
            "pane": {"id": rt.pane_id, "pid": rt.pty.pid(),
                     "size": {"cols": pane.size.0, "rows": pane.size.1}},
            "replay_bytes": snap.len(),
            "snapshot": {
                "bytes": snap.len(), "frames": frames,
                "history_lines": info.lines.min(history),
                "alternate": alternate, "size": {"cols": cols, "rows": rows},
            },
        });
        c.sink.control(&proto::response(req.id, Ok(result)));
        let mut lag = None;
        for chunk in snap.chunks(proto::MAX_DATA) {
            let frame: Arc<[u8]> = proto::encode_data(rt.stream, seq, FLAG_SNAPSHOT, chunk).into();
            if let Err(m) = c.sink.push_output(rt.stream, frame, info.total_lines) {
                lag = Some(m);
                break;
            }
        }
        c.attached.insert(rt.stream, Att { interactive, lag });
        st.stats.snapshots += 1;
        st.stats.snapshot_bytes += snap.len() as u64;
        Reply::Queued
    }

    /// The pane a capture or history request means: a target of any level (also a `stopped`
    /// Terminal, whose screen is kept), or none when exactly one pane has a screen.
    fn screen_target(st: &State, target: &Value) -> Result<(String, String), IpcError> {
        let pane_id = match target {
            Value::Null => {
                let mut panes: Vec<(&String, &PaneOut)> = st.out.iter().collect();
                panes.sort_by(|a, b| a.0.cmp(b.0));
                match panes.as_slice() {
                    [] => {
                        return Err(IpcError::new(
                            ErrorCode::TargetNotFound,
                            "this Host keeps no pane screen (no Terminal has started)",
                        ));
                    }
                    [(id, _)] => (*id).clone(),
                    many => {
                        return Err(IpcError::new(
                            ErrorCode::AmbiguousTarget,
                            "this Host has several Terminals; name one as the target",
                        )
                        .details(json!({
                            "candidates": many.iter().map(|(_, o)| o.terminal_id.clone()).collect::<Vec<_>>()
                        })));
                    }
                }
            }
            Value::String(s) => {
                let r = st.reg.resolve(s)?;
                st.reg.terminals[r.terminal].windows[r.window].panes[r.pane]
                    .id
                    .clone()
            }
            _ => {
                return Err(IpcError::param(
                    "target",
                    "target must be a Terminal, window or pane",
                ));
            }
        };
        let t = st
            .reg
            .owner_of_pane(&pane_id)
            .ok_or_else(|| IpcError::new(ErrorCode::TargetNotFound, "no such pane"))?;
        if !st.out.contains_key(&pane_id) {
            return Err(IpcError::new(
                ErrorCode::InvalidState,
                format!(
                    "Terminal `{}` is {} and has no screen to read",
                    t.name.display,
                    t.state.as_str()
                ),
            )
            .details(json!({"terminal_id": t.id, "state": t.state.as_str()})));
        }
        Ok((pane_id, t.id.clone()))
    }

    /// `pane.capture` (tmux `capture-pane`, docs/SCROLLBACK.md §4): lines of the history and the
    /// screen as text or VT, redacted unless `redact` is false. At most [`CAPTURE_BYTES`] of
    /// lines per answer; older ones are left out and `truncated` says so.
    fn capture(&self, p: &Value) -> Result<Value, IpcError> {
        let pos = |name: &str| -> Result<Option<Pos>, IpcError> {
            match &p[name] {
                Value::Null => Ok(None),
                Value::String(s) if s == "-" => Ok(Some(Pos::Edge)),
                v => v.as_i64().map(|n| Some(Pos::Line(n))).ok_or_else(|| {
                    IpcError::param(name, format!("{name} must be a line number or \"-\""))
                }),
            }
        };
        let start = pos("start")?.unwrap_or(Pos::Line(0));
        let end = pos("end")?.unwrap_or(Pos::Edge);
        let vt = match optional_string(p, "format")? {
            None | Some("text") => false,
            Some("vt") => true,
            Some(_) => return Err(IpcError::param("format", "format must be text or vt")),
        };
        let join = flag(p, "join")?;
        let redact = match &p["redact"] {
            Value::Null => true,
            Value::Bool(b) => *b,
            _ => return Err(IpcError::param("redact", "redact must be true or false")),
        };
        let mut st = self.lock();
        let (pane_id, terminal_id) = Self::screen_target(&st, &p["target"])?;
        st.stats.captures += 1;
        let s = &st.out[&pane_id].screen;
        let rows = s.capture(start, end, vt, false);
        let mut red = Redactor::default();
        let (lines, range, truncated) = match rows.range {
            None => (Vec::new(), None, false),
            Some((first, last)) => {
                // Keep the newest lines that fit in one answer.
                let mut size = 0;
                let mut keep = rows.lines.len();
                for (i, l) in rows.lines.iter().enumerate().rev() {
                    let n = serde_json::to_string(l).map_or(l.len(), |j| j.len()) + 1;
                    if size + n > CAPTURE_BYTES {
                        break;
                    }
                    size += n;
                    keep = i;
                }
                let from = first + i64::try_from(keep).unwrap_or(0);
                if redact {
                    // Earlier lines still decide whether a private key block is open.
                    for l in &rows.lines[..keep] {
                        red.line(l);
                    }
                    red.hidden = 0;
                }
                let lines = if join {
                    s.capture(Pos::Line(from), Pos::Line(last), vt, true).lines
                } else {
                    rows.lines[keep..].to_vec()
                };
                let lines: Vec<String> = if redact {
                    lines.iter().map(|l| red.line(l)).collect()
                } else {
                    lines
                };
                (lines, Some((from, last)), keep > 0)
            }
        };
        let cur = s.cursor();
        let (cols, rws) = s.size();
        Ok(json!({
            "terminal_id": terminal_id,
            "pane_id": pane_id,
            "format": if vt { "vt" } else { "text" },
            "join": join,
            "redact": redact,
            "redacted": red.hidden,
            "start": range.map(|r| r.0),
            "end": range.map(|r| r.1),
            "truncated": truncated,
            "lines": lines,
            "screen": {
                "size": {"cols": cols, "rows": rws},
                "alternate": s.alternate(),
                "cursor": {"x": cur.x, "y": cur.y, "visible": cur.visible},
            },
            "history": session::history_json(&s.history()),
        }))
    }

    /// `pane.clear_history` (tmux `clear-history`): drops the pane's history lines; the screen
    /// stays. What clients already show in their own scrollback is theirs.
    fn clear_history(&self, p: &Value) -> Result<Value, IpcError> {
        let dry_run = flag(p, "dry_run")?;
        let mut st = self.lock();
        let (pane_id, terminal_id) = Self::screen_target(&st, &p["target"])?;
        let s = &mut st
            .out
            .get_mut(&pane_id)
            .expect("checked by screen_target")
            .screen;
        let before = s.history();
        if dry_run {
            return Ok(json!({"dry_run": true, "would_clear": {
                "terminal_id": terminal_id, "pane_id": pane_id,
                "lines": before.lines, "bytes": before.bytes,
            }}));
        }
        s.clear_history();
        Ok(json!({
            "terminal_id": terminal_id,
            "pane_id": pane_id,
            "cleared_lines": before.lines,
            "cleared_bytes": before.bytes,
            "history": session::history_json(&s.history()),
        }))
    }

    fn attach_target(&self, st: &State, target: &Value) -> Result<Arc<PaneRt>, IpcError> {
        let pane_id = match target {
            Value::Null => {
                let live: Vec<&Arc<PaneRt>> = st.rt.values().collect();
                match live.as_slice() {
                    [] => {
                        return Err(IpcError::new(
                            ErrorCode::TargetNotFound,
                            "this Host owns no running pane (it was started with --no-shell, or every Terminal ended)",
                        ));
                    }
                    [one] => one.pane_id.clone(),
                    many => {
                        return Err(IpcError::new(
                            ErrorCode::AmbiguousTarget,
                            "this Host owns several running Terminals; name one as the target",
                        )
                        .details(json!({
                            "candidates": many.iter().map(|r| r.terminal_id.clone()).collect::<Vec<_>>()
                        })));
                    }
                }
            }
            Value::String(s) => {
                let r = st.reg.resolve(s)?;
                st.reg.terminals[r.terminal].windows[r.window].panes[r.pane]
                    .id
                    .clone()
            }
            _ => {
                return Err(IpcError::param(
                    "target",
                    "target must be a Terminal, window or pane",
                ));
            }
        };
        let t = st
            .reg
            .owner_of_pane(&pane_id)
            .ok_or_else(|| IpcError::new(ErrorCode::TargetNotFound, "no such pane"))?;
        if t.state != session::State::Running || t.ending.is_some() {
            return Err(IpcError::new(
                ErrorCode::InvalidState,
                format!(
                    "Terminal `{}` is {}; only a running Terminal can be attached",
                    t.name.display,
                    if t.ending.is_some() {
                        "stopping"
                    } else {
                        t.state.as_str()
                    }
                ),
            )
            .details(json!({"terminal_id": t.id, "state": t.state.as_str()})));
        }
        st.rt
            .get(&pane_id)
            .cloned()
            .ok_or_else(|| IpcError::new(ErrorCode::InvalidState, "the pane's process has exited"))
    }

    fn detach(&self, conn: u64, params: &Value) -> Result<Value, IpcError> {
        let mut st = self.lock();
        let known = |st: &State, s: u32| st.reg.pane_of_stream(s).is_some();
        match &params["stream_id"] {
            Value::Null => {
                if let Some(c) = st.conns.get_mut(&conn) {
                    c.attached.clear();
                }
            }
            v => {
                let s = v
                    .as_u64()
                    .and_then(|s| u32::try_from(s).ok())
                    .ok_or_else(|| IpcError::param("stream_id", "no such stream"))?;
                let was = st
                    .conns
                    .get_mut(&conn)
                    .is_some_and(|c| c.attached.remove(&s).is_some());
                if !was && !known(&st, s) {
                    return Err(IpcError::param("stream_id", "no such stream"));
                }
            }
        }
        Ok(json!({}))
    }

    /// `client.detach` (Step 12, ADR 0027 §3): ends the attachments of clients — every client of
    /// the panes `target` names, the client `client_id`, or that client's attachments to that
    /// target — and nothing else: no process, Terminal or connection is touched. Each detached
    /// connection subscribed to `panes` gets `pane.detached` in the same critical section that
    /// removes the attachment, so it arrives before the output stops.
    fn detach_clients(&self, conn: u64, p: &Value) -> Result<Value, IpcError> {
        let client = match &p["client_id"] {
            Value::Null => None,
            v => Some(v.as_u64().filter(|c| *c > 0).ok_or_else(|| {
                IpcError::param("client_id", "client_id must be a positive integer")
            })?),
        };
        let target = optional_string(p, "target")?;
        if target.is_none() && client.is_none() {
            return Err(IpcError::param(
                "target",
                "name a target, a client_id, or both",
            ));
        }
        let dry_run = flag(p, "dry_run")?;
        let mut st = self.lock();
        let (streams, terminal) = match target {
            None => (None, Value::Null),
            Some(t) => {
                let r = st.reg.resolve(t)?;
                let term = &st.reg.terminals[r.terminal];
                let streams: Vec<u32> = match r.level {
                    Level::Terminal => term.panes().map(|p| p.stream).collect(),
                    Level::Window => term.windows[r.window]
                        .panes
                        .iter()
                        .map(|p| p.stream)
                        .collect(),
                    Level::Pane => vec![term.windows[r.window].panes[r.pane].stream],
                };
                (
                    Some(streams),
                    json!({"id": term.id, "name": term.name.display}),
                )
            }
        };
        if let Some(c) = client
            && !st.conns.contains_key(&c)
        {
            return Err(IpcError::new(
                ErrorCode::TargetNotFound,
                format!("no client {c} is connected to this Host"),
            )
            .details(json!({"client_id": c})));
        }
        let mut ids: Vec<u64> = st
            .conns
            .keys()
            .copied()
            .filter(|id| client.is_none_or(|c| c == *id))
            .collect();
        ids.sort_unstable();
        let mut found: Vec<(u64, u32, Value)> = Vec::new();
        for id in ids {
            let c = &st.conns[&id];
            for (&stream, att) in &c.attached {
                let interactive = att.interactive;
                if streams.as_ref().is_some_and(|s| !s.contains(&stream)) {
                    continue;
                }
                let Some((t, pane)) = st.reg.pane_of_stream(stream) else {
                    continue;
                };
                found.push((
                    id,
                    stream,
                    json!({
                        "client": {"id": id, "pid": c.pid, "kind": c.kind.as_str(), "name": c.name},
                        "stream_id": stream, "pane_id": pane.id, "terminal_id": t.id,
                        "interactive": interactive,
                    }),
                ));
            }
        }
        let entries: Vec<Value> = found.iter().map(|(_, _, e)| e.clone()).collect();
        if dry_run {
            return Ok(json!({"dry_run": true, "would_detach": entries, "terminal": terminal}));
        }
        let by = st.conns.get(&conn).map_or(
            Value::Null,
            |c| json!({"client_id": conn, "pid": c.pid, "kind": c.kind.as_str()}),
        );
        for (id, stream, e) in &found {
            if let Some(c) = st.conns.get_mut(id) {
                c.attached.remove(stream);
                if c.topics.iter().any(|t| t == "panes") {
                    c.event(
                        "pane.detached",
                        &json!({"pane_id": e["pane_id"], "terminal_id": e["terminal_id"],
                                "stream_id": stream, "by": by}),
                    );
                }
            }
        }
        Ok(json!({"detached": entries, "terminal": terminal}))
    }

    fn resize(&self, conn: u64, params: &Value) -> Result<Value, IpcError> {
        let (c, r) = size_param(params, "cols")?
            .ok_or_else(|| IpcError::param("cols", "cols and rows are required"))?;
        let (rt, changed) = {
            let mut st = self.lock();
            if st.rt.is_empty() {
                return Err(IpcError::new(
                    ErrorCode::TargetNotFound,
                    "this Host owns no running pane",
                ));
            }
            let mine: Vec<u32> = st
                .conns
                .get(&conn)
                .map(|c| {
                    c.attached
                        .iter()
                        .filter(|(_, a)| a.interactive)
                        .map(|(s, _)| *s)
                        .collect()
                })
                .unwrap_or_default();
            let stream = match &params["stream_id"] {
                Value::Null => match mine.as_slice() {
                    [one] => *one,
                    [] => {
                        return Err(IpcError::new(
                            ErrorCode::InvalidState,
                            "attach interactively before resizing the pane",
                        ));
                    }
                    _ => {
                        return Err(IpcError::param(
                            "stream_id",
                            "attached to several panes: name the stream_id to resize",
                        ));
                    }
                },
                v => {
                    let s = v
                        .as_u64()
                        .and_then(|s| u32::try_from(s).ok())
                        .ok_or_else(|| IpcError::param("stream_id", "no such stream"))?;
                    if !mine.contains(&s) {
                        return Err(IpcError::new(
                            ErrorCode::InvalidState,
                            "attach interactively to this stream before resizing it",
                        ));
                    }
                    s
                }
            };
            let rt = st
                .rt_of_stream(stream)
                .cloned()
                .ok_or_else(|| IpcError::new(ErrorCode::InvalidState, "the pane has ended"))?;
            // The model first, as a terminal does (see `attach`); the same size changes nothing.
            // Step 22: the size is the window's; every pane whose share changed is resized.
            let (tid, pid) = (rt.terminal_id.clone(), rt.pane_id.clone());
            let changed = st.resize_window(&tid, &pid, (c, r));
            (rt, changed)
        };
        apply_resizes(&changed)?;
        // The pane's own share of the window (the whole window when it is alone or zoomed).
        let (pc, pr) = self
            .lock()
            .reg
            .get_mut(&rt.terminal_id)
            .and_then(|t| t.pane_mut(&rt.pane_id))
            .map_or((c, r), |p| p.size);
        Ok(json!({"cols": pc, "rows": pr, "stream_id": rt.stream,
            "window_size": {"cols": c, "rows": r}}))
    }

    fn list(&self, params: &Value) -> Result<Value, IpcError> {
        let full = match optional_string(params, "detail")? {
            None | Some("summary") => false,
            Some("full") => true,
            Some(_) => {
                return Err(IpcError::param("detail", "detail must be summary or full"));
            }
        };
        let st = self.lock();
        let terminals: Vec<Value> = st
            .reg
            .terminals
            .iter()
            .map(|t| {
                if full {
                    st.terminal_json(t)
                } else {
                    st.summary_json(t)
                }
            })
            .collect();
        Ok(json!({ "terminals": terminals }))
    }

    fn info(&self, params: &Value) -> Result<Value, IpcError> {
        let target = target(params)?;
        let st = self.lock();
        let i = st.reg.resolve_terminal(target)?;
        Ok(json!({ "terminal": st.terminal_json(&st.reg.terminals[i]) }))
    }

    /// The program, working directory, environment, size and history of a new pane: the
    /// parameters `terminal.create`, `window.create` and `pane.create` share, checked.
    fn launch_params(&self, p: &Value) -> Result<PaneLaunch, IpcError> {
        let env = EnvOverrides::from_json(&p["env"]).map_err(|m| IpcError::param("env", m))?;
        // Step 12 (ADR 0027 §4): a verbatim command line, for programs such as `cmd.exe /c` that
        // parse their command line themselves. Only its first word is resolved; `command` is its
        // split, for display and the busy rule.
        let raw_line = match optional_string(p, "command_line")? {
            None => None,
            Some(_) if !p["command"].is_null() => {
                return Err(IpcError::param(
                    "command_line",
                    "give either command or command_line, not both",
                ));
            }
            Some(l) if l.contains('\0') || session::split_command_line(l).program.is_empty() => {
                return Err(IpcError::param(
                    "command_line",
                    "command_line must start with a program",
                ));
            }
            Some(l) => Some(l.to_string()),
        };
        let command = match &p["command"] {
            Value::Null if raw_line.is_some() => {
                session::split_command_line(raw_line.as_deref().unwrap_or_default())
            }
            Value::Null => session::split_command_line(&default_shell()),
            Value::Object(c) => {
                let program = c
                    .get("program")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty() && !s.contains('\0'))
                    .ok_or_else(|| {
                        IpcError::param("command", "command.program must be a non-empty string")
                    })?;
                let args = match c.get("args") {
                    None | Some(Value::Null) => Vec::new(),
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|v| v.as_str().filter(|s| !s.contains('\0')).map(String::from))
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| {
                            IpcError::param("command", "command.args must be a list of strings")
                        })?,
                    Some(_) => {
                        return Err(IpcError::param(
                            "command",
                            "command.args must be a list of strings",
                        ));
                    }
                };
                Command {
                    program: program.to_string(),
                    args,
                }
            }
            _ => {
                return Err(IpcError::param(
                    "command",
                    "command must be {program, args}",
                ));
            }
        };
        let line = raw_line
            .clone()
            .unwrap_or_else(|| win::launch::command_line(&command.program, &command.args));
        if line.encode_utf16().count() >= MAX_COMMAND_LINE {
            return Err(IpcError::param(
                if raw_line.is_some() {
                    "command_line"
                } else {
                    "command"
                },
                "the command line is longer than 32766 characters",
            ));
        }
        let cwd = match optional_string(p, "cwd")? {
            Some(c) if !Path::new(c).is_absolute() => {
                return Err(IpcError::param("cwd", "cwd must be an absolute path"));
            }
            Some(c) => Some(c.to_string()),
            None => self.home.as_ref().map(|h| h.display().to_string()),
        };
        let size = size_param(&p["size"], "size")?.unwrap_or(DEFAULT_SIZE);
        // Step 13: history rows the pane keeps (tmux `history-limit`).
        let history_limit = match &p["history_limit"] {
            Value::Null => screen::DEFAULT_HISTORY_LINES,
            v => v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n <= screen::MAX_HISTORY_LINES)
                .ok_or_else(|| {
                    IpcError::param(
                        "history_limit",
                        format!(
                            "history_limit must be a number of lines from 0 to {}",
                            screen::MAX_HISTORY_LINES
                        ),
                    )
                })?,
        };
        Ok(PaneLaunch {
            command,
            raw_line,
            cwd,
            env,
            size,
            history_limit,
        })
    }

    /// What a new pane would run (`dry_run`): the same checks as a real start (working
    /// directory, program on the pane's PATH, scripts), with nothing started.
    fn plan_pane(l: &PaneLaunch) -> Result<Value, IpcError> {
        let dir = l
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let executable = pty::plan(&pty::Spec {
            program: &l.command.program,
            args: &l.command.args,
            raw_line: l.raw_line.as_deref(),
            cwd: &dir,
            env: &l.env,
            host_vars: &[],
            size: l.size,
        })
        .map_err(|e| spawn_failed(&e, &l.command.program, false, None))?;
        Ok(json!({
            "command": {"program": redact::program(&l.command.program), "args": redact::args(&l.command.args)},
            "command_line": l.raw_line.as_deref().map(redact::command_line),
            "executable": executable.display().to_string(),
            "cwd": l.cwd,
            "env": session::env_json(&l.env.names()),
            "size": {"cols": l.size.0, "rows": l.size.1},
            "history_limit": l.history_limit,
        }))
    }

    fn create_request(self: &Arc<Self>, p: &Value) -> Result<Value, IpcError> {
        if !p["profile"].is_null() {
            return Err(IpcError::param(
                "profile",
                "`profile` is part of IPC v1 but not implemented by this Host yet (Step 33)",
            ));
        }
        let name = optional_string(p, "name")?
            .map(session::parse_name)
            .transpose()?;
        let PaneLaunch {
            command,
            raw_line,
            cwd,
            env,
            size,
            history_limit,
        } = self.launch_params(p)?;
        let remain_on_exit = flag(p, "remain_on_exit")?;
        let keep = flag(p, "keep_on_failure")?;
        if flag(p, "dry_run")? {
            let st = self.lock();
            let name = match &name {
                Some(n) => {
                    st.reg.check_free(n, None)?;
                    n.display.clone()
                }
                None => st.reg.default_name().display,
            };
            drop(st);
            // The same checks as a real start (working directory, program on the pane's PATH,
            // scripts), with nothing started.
            let dir = cwd
                .as_deref()
                .map(PathBuf::from)
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_default();
            let executable = pty::plan(&pty::Spec {
                program: &command.program,
                args: &command.args,
                raw_line: raw_line.as_deref(),
                cwd: &dir,
                env: &env,
                host_vars: &[],
                size,
            })
            .map_err(|e| spawn_failed(&e, &command.program, false, None))?;
            return Ok(json!({"dry_run": true, "would_create": {
                "name": name,
                "command": {"program": redact::program(&command.program), "args": redact::args(&command.args)},
                "command_line": raw_line.as_deref().map(redact::command_line),
                "executable": executable.display().to_string(),
                "cwd": cwd,
                "env": session::env_json(&env.names()),
                "size": {"cols": size.0, "rows": size.1},
                "remain_on_exit": remain_on_exit,
                "keep_on_failure": keep,
                "history_limit": history_limit,
            }}));
        }
        let spec = Spec {
            name,
            command,
            command_line: raw_line.clone(),
            cwd,
            env: env.names(),
            size,
            remain_on_exit,
            history_limit,
        };
        self.create(spec, &Launch { raw_line, env }, keep)
    }

    fn rename(&self, p: &Value) -> Result<Value, IpcError> {
        let target = target(p)?;
        let raw = optional_string(p, "name")?
            .ok_or_else(|| IpcError::param("name", "name is required"))?;
        let name = session::parse_name(raw)?;
        let dry_run = flag(p, "dry_run")?;
        let mut st = self.lock();
        let i = st.reg.resolve_terminal(target)?;
        let id = st.reg.terminals[i].id.clone();
        st.reg.check_free(&name, Some(&id))?;
        let previous = st.reg.terminals[i].name.display.clone();
        if dry_run {
            return Ok(json!({"dry_run": true, "would_rename": {
                "terminal_id": id, "from": previous, "to": name.display,
            }}));
        }
        let changed = previous != name.display;
        if changed {
            st.reg.terminals[i].name = name.clone();
            let data = json!({"terminal_id": id, "name": name.display, "previous_name": previous});
            st.notify("terminals", "terminal.renamed", &data);
        }
        Ok(json!({
            "terminal_id": id, "name": name.display, "previous_name": previous, "changed": changed,
        }))
    }

    /// `window.create` (a new internal window with one pane) and `pane.create` (a new pane in a
    /// window) of a running Terminal (Step 21, ADR 0054). The ids are reserved under the lock, the
    /// pane's process is started outside it, and the window or pane enters the model only once
    /// it runs, so no client ever sees a half-made one; a failed start leaves nothing. `select`
    /// (default true, tmux `new-window` without `-d`) makes it the Terminal's active pane.
    ///
    /// Step 22 (ADR 0056): a new pane splits a pane of the window (`pane.split`: the target pane,
    /// or the active pane of a target window or Terminal; `direction`, `before`, `cells` or
    /// `percent`), and starts at its share of the window. `pane.create` is a vertical split of
    /// the window's active pane. A new window starts at the size of the Terminal's active window
    /// unless a `size` is given.
    fn create_in(self: &Arc<Self>, p: &Value, method: &str) -> Result<Value, IpcError> {
        let new_window = method == "window.create";
        let split = method == "pane.split";
        let target = target(p)?;
        let dir = match optional_string(p, "direction")? {
            None | Some("vertical") => layout::Dir::Vertical,
            Some("horizontal") => layout::Dir::Horizontal,
            Some(_) => {
                return Err(IpcError::param(
                    "direction",
                    "direction must be horizontal (side by side) or vertical (stacked)",
                ));
            }
        };
        let before = flag(p, "before")?;
        let cells = |k: &str| -> Result<Option<u64>, IpcError> {
            match &p[k] {
                Value::Null => Ok(None),
                v => v
                    .as_u64()
                    .filter(|n| (1..=1000).contains(n))
                    .map(Some)
                    .ok_or_else(|| {
                        IpcError::param(k, format!("{k} must be a number from 1 to 1000"))
                    }),
            }
        };
        let amount = match (cells("cells")?, cells("percent")?) {
            (Some(_), Some(_)) => {
                return Err(IpcError::param(
                    "cells",
                    "give either cells or percent, not both",
                ));
            }
            (Some(n), None) => Some(layout::Amount::Cells(u16::try_from(n).unwrap_or(1000))),
            (None, Some(n)) if n <= 99 => {
                Some(layout::Amount::Percent(u8::try_from(n).unwrap_or(50)))
            }
            (None, Some(_)) => {
                return Err(IpcError::param("percent", "percent must be from 1 to 99"));
            }
            (None, None) => None,
        };
        if !split && (before || amount.is_some() || !p["direction"].is_null()) {
            return Err(IpcError::param(
                "direction",
                format!("{method} takes no split options; use pane.split"),
            ));
        }
        let name = if new_window {
            optional_string(p, "name")?
                .map(session::parse_window_name)
                .transpose()?
        } else {
            None
        };
        let select = match &p["select"] {
            Value::Null => true,
            Value::Bool(b) => *b,
            _ => return Err(IpcError::param("select", "select must be true or false")),
        };
        let dry_run = flag(p, "dry_run")?;
        let mut l = self.launch_params(p)?;
        let sized = !p["size"].is_null();
        let (tid, wid, pane_id, stream, split_of) = {
            let mut st = self.lock();
            if st.stopping {
                return Err(IpcError::new(
                    ErrorCode::HostStopping,
                    "the Host is shutting down",
                ));
            }
            let r = st.reg.resolve(target)?;
            if new_window && r.level != Level::Terminal {
                return Err(wrong_level(method, "a Terminal (its name or t- id)"));
            }
            if method == "pane.create" && r.level == Level::Pane {
                return Err(wrong_level(
                    method,
                    "a Terminal (its active window) or a window (terminal:window or a w- id)",
                ));
            }
            let t = &st.reg.terminals[r.terminal];
            require_running(t)?;
            if t.panes().count() >= MAX_PANES_PER_TERMINAL || st.rt.len() >= MAX_PANES {
                return Err(IpcError::new(
                    ErrorCode::Busy,
                    format!(
                        "a Terminal holds at most {MAX_PANES_PER_TERMINAL} panes and a Host runs at most {MAX_PANES}"
                    ),
                ));
            }
            let tid = t.id.clone();
            let existing = (!new_window).then(|| t.windows[r.window].id.clone());
            // The pane to split and the new pane's share, planned on a copy of the layout.
            let split_of = if new_window {
                if !sized {
                    l.size = t.windows[t.active_window].layout().size();
                }
                None
            } else {
                let w = &t.windows[r.window];
                let at = if r.level == Level::Pane {
                    r.pane
                } else {
                    w.active_pane
                };
                let of = w.panes[at].id.clone();
                let mut probe = w.layout().clone();
                probe
                    .split(&of, "p-new", dir, before, amount)
                    .map_err(|e| layout_refused(&e, "split the pane"))?;
                l.size = probe.rect("p-new").map_or(l.size, layout::Rect::size);
                Some(of)
            };
            if dry_run {
                let (tname, index) = (t.name.display.clone(), t.free_window_index());
                drop(st);
                let mut plan = Self::plan_pane(&l)?;
                plan["terminal_id"] = json!(tid);
                plan["terminal"] = json!(tname);
                plan["window_id"] = json!(existing);
                plan["window_index"] = json!(if new_window { Some(index) } else { None });
                plan["window_name"] = json!(if new_window {
                    Some(name.unwrap_or_else(|| session::window_name(&l.command.program)))
                } else {
                    None
                });
                plan["select"] = json!(select);
                if let Some(of) = &split_of {
                    plan["split"] =
                        json!({"pane_id": of, "direction": dir.name(), "before": before});
                }
                return Ok(json!({"dry_run": true, "would_create": plan}));
            }
            let wid = existing.unwrap_or_else(|| st.reg.issue_id('w', proto::new_object_id));
            let pane_id = st.reg.issue_id('p', proto::new_object_id);
            let stream = st.reg.alloc_stream();
            (tid, wid, pane_id, stream, split_of)
        };
        let cwd = l
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let started = pty::spawn(&pty::Spec {
            program: &l.command.program,
            args: &l.command.args,
            raw_line: l.raw_line.as_deref(),
            cwd: &cwd,
            env: &l.env,
            host_vars: &[
                ("SUBAK_PANE_ID", &pane_id),
                ("SUBAK_TERMINAL_ID", &tid),
                ("SUBAK_ENDPOINT", &self.endpoint_name),
            ],
            size: l.size,
        })
        .map_err(|e| {
            self.lock().stats.spawn_failures += 1;
            spawn_failed(&e, &l.command.program, false, None)
        })?;
        let pty = started.pty;
        let output = pty
            .take_output()
            .expect("a fresh pseudo console has its output end");
        let rt = PaneRt::new(&pane_id, &tid, stream, Arc::clone(&pty));
        let result = {
            let mut st = self.lock();
            let fits = st.reg.get(&tid).is_some_and(|t| {
                require_running(t).is_ok()
                    && (new_window
                        || t.window_pos(&wid).is_some_and(|wi| {
                            split_of
                                .as_ref()
                                .is_some_and(|of| t.windows[wi].layout().contains(of))
                        }))
            });
            if st.stopping || !fits {
                let stopping = st.stopping;
                drop(st);
                discard(&pty, output);
                return Err(if stopping {
                    IpcError::new(
                        ErrorCode::HostStopping,
                        "the Host began shutting down while the pane started; nothing was kept",
                    )
                } else {
                    IpcError::new(
                        ErrorCode::InvalidState,
                        "the Terminal or window ended while the pane started; nothing was kept",
                    )
                    .details(json!({"terminal_id": tid}))
                });
            }
            let pane = session::Pane {
                id: pane_id.clone(),
                index: 0,
                command: l.command,
                command_line: l.raw_line,
                cwd: l.cwd,
                executable: Some(started.executable.display().to_string()),
                env: l.env.names(),
                size: l.size,
                stream,
                pid: Some(pty.pid()),
                history_limit: l.history_limit,
            };
            let t = st.reg.get_mut(&tid).expect("checked above");
            let wi = if new_window {
                let index = t.free_window_index();
                let automatic_name = name.is_none();
                let wname = name.unwrap_or_else(|| session::window_name(&pane.command.program));
                t.insert_window(session::Window {
                    id: wid.clone(),
                    index,
                    name: wname,
                    automatic_name,
                    panes: Vec::new(),
                    active_pane: 0,
                    layout: None,
                    options: session::WindowOptions::new(),
                    activity: false,
                    bell: false,
                    last_pane: None,
                })
            } else {
                t.window_pos(&wid).expect("checked above")
            };
            let (pi, changed) = match &split_of {
                None => (t.push_pane(wi, pane), Vec::new()),
                Some(of) => match t.windows[wi].split(of, pane, dir, before, amount) {
                    Ok(done) => done,
                    Err(e) => {
                        drop(st);
                        discard(&pty, output);
                        return Err(layout_refused(&e, "split the pane"));
                    }
                },
            };
            let (window_changed, pane_changed) = if select {
                t.select_pane(wi, pi)
            } else {
                (false, false)
            };
            st.out.insert(
                pane_id.clone(),
                PaneOut {
                    screen: Screen::new(
                        l.size.0,
                        l.size.1,
                        Limits {
                            lines: l.history_limit,
                            bytes: screen::HISTORY_BYTES,
                        },
                    ),
                    terminal_id: tid.clone(),
                    seq: 0,
                    input_bytes: 0,
                },
            );
            st.rt.insert(pane_id.clone(), Arc::clone(&rt));
            let resizes = st.resized(changed);
            let t = st.reg.get(&tid).expect("checked above");
            let w = &t.windows[wi];
            let window = st.window_json(t, w);
            let mut pane = session::pane_json(t, &w.panes[pi], &st.live(&w.panes[pi]));
            session::place_json(w, &w.panes[pi], &mut pane);
            if new_window {
                let data = json!({"terminal_id": tid, "window": window});
                st.notify("terminals", "window.created", &data);
            } else {
                let data = json!({"terminal_id": tid, "window_id": wid, "pane": pane});
                st.notify("terminals", "pane.created", &data);
            }
            st.selected(&tid, window_changed, pane_changed);
            if !new_window {
                st.layout_event(&tid, &wid);
            }
            let result = json!({
                "terminal_id": tid,
                "window_id": wid,
                "pane_id": pane_id,
                "selected": select,
                "window": window,
            });
            (result, resizes)
        };
        let (result, resizes) = result;
        let _ = apply_resizes(&resizes);
        self.run_pane(&rt, output);
        let _ = self.events.send(Ev::Changed);
        Ok(result)
    }

    /// `window.list` (a Terminal's windows) and `pane.list` (a Terminal's or one window's panes,
    /// in their windows): `{terminal_id, name, active_window, windows}`.
    fn topology(&self, p: &Value, panes: bool) -> Result<Value, IpcError> {
        let target = target(p)?;
        let st = self.lock();
        let r = st.reg.resolve(target)?;
        let only = match (panes, r.level) {
            (_, Level::Terminal) => None,
            (true, Level::Window) => Some(r.window),
            (false, _) => {
                return Err(wrong_level("window.list", "a Terminal (its name or t- id)"));
            }
            (true, Level::Pane) => {
                return Err(wrong_level(
                    "pane.list",
                    "a Terminal or a window (terminal:window or a w- id)",
                ));
            }
        };
        let t = &st.reg.terminals[r.terminal];
        let windows: Vec<Value> = t
            .windows
            .iter()
            .enumerate()
            .filter(|(i, _)| only.is_none_or(|o| o == *i))
            .map(|(_, w)| st.window_json(t, w))
            .collect();
        Ok(json!({
            "terminal_id": t.id,
            "name": t.name.display,
            "state": t.state.as_str(),
            "active_window": t.windows[t.active_window].id,
            "last_window": t.last_window,
            "windows": windows,
        }))
    }

    /// `window.select` and `pane.select` (tmux `select-window`, `select-pane`): the Terminal's
    /// active window, or a window's active pane and that window, for every client. Selecting
    /// changes no process. Allowed in any state (a stopped Terminal keeps its selection).
    ///
    /// Step 22: `pane.select` with a `direction` (`left`, `right`, `up`, `down`: the pane next to
    /// the target pane, or the active pane of a target window or Terminal, by the layout; `next`,
    /// `previous`: by pane index; both wrap around, as tmux does). Selecting another pane of a
    /// zoomed window unzooms it.
    fn select(&self, p: &Value, pane: bool) -> Result<Value, IpcError> {
        let target = target(p)?;
        let dry_run = flag(p, "dry_run")?;
        let direction = optional_string(p, "direction")?;
        if direction.is_some() && !pane {
            return Err(IpcError::param(
                "direction",
                "only pane.select takes a direction",
            ));
        }
        let resizes;
        let out = {
            let mut st = self.lock();
            let mut r = st.reg.resolve(target)?;
            if let Some(d) = direction {
                let t = &st.reg.terminals[r.terminal];
                let w = &t.windows[r.window];
                let from = if r.level == Level::Pane {
                    r.pane
                } else {
                    w.active_pane
                };
                let n = w.panes.len();
                let to = match d {
                    "next" => (from + 1) % n,
                    "previous" => (from + n - 1) % n,
                    side => {
                        let side = layout::Side::parse(side).ok_or_else(|| {
                            IpcError::param(
                                "direction",
                                "direction must be left, right, up, down, next or previous",
                            )
                        })?;
                        w.layout()
                            .neighbour(&w.panes[from].id, side)
                            .and_then(|id| w.panes.iter().position(|q| q.id == id))
                            .unwrap_or(from)
                    }
                };
                r.pane = to;
                r.level = Level::Pane;
            }
            match (pane, r.level) {
                (false, Level::Window) | (true, Level::Pane) => {}
                (false, _) => {
                    return Err(wrong_level(
                        "window.select",
                        "a window (terminal:window or a w- id)",
                    ));
                }
                (true, _) => {
                    return Err(wrong_level(
                        "pane.select",
                        "a pane (terminal:window.pane or a p- id)",
                    ));
                }
            }
            let t = &mut st.reg.terminals[r.terminal];
            if dry_run {
                let w = &t.windows[r.window];
                let pi = if pane { r.pane } else { w.active_pane };
                return Ok(json!({"dry_run": true, "would_select": {
                    "terminal_id": t.id, "window_id": w.id, "pane_id": w.panes[pi].id,
                    "changed": t.active_window != r.window || w.active_pane != pi,
                }}));
            }
            let (wc, pc) = if pane {
                t.select_pane(r.window, r.pane)
            } else {
                (t.select_window(r.window), false)
            };
            let w = &mut t.windows[r.window];
            // Another pane of a zoomed window: unzoomed (tmux `select-pane` without -Z).
            let unzoom = pc
                && w.layout().zoomed().is_some()
                && w.layout().zoomed() != Some(w.panes[w.active_pane].id.as_str());
            let changed = if unzoom {
                w.relayout(|l| l.zoom(None))
                    .map(|(_, c)| c)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let out = json!({
                "terminal_id": t.id,
                "window_id": w.id,
                "pane_id": w.panes[w.active_pane].id,
                "changed": wc || pc,
            });
            let (tid, wid) = (t.id.clone(), w.id.clone());
            st.selected(&tid, wc, pc);
            if unzoom {
                st.layout_event(&tid, &wid);
            }
            resizes = st.resized(changed);
            out
        };
        apply_resizes(&resizes)?;
        Ok(out)
    }

    /// `window.set_option` (tmux `set-window-option`): `synchronize-panes`, `monitor-activity`,
    /// `monitor-bell` of the target window (a Terminal: its active window), `value` true/false
    /// or omitted to toggle.
    fn set_option(&self, p: &Value) -> Result<Value, IpcError> {
        let target = target(p)?;
        let name = optional_string(p, "option")?
            .ok_or_else(|| IpcError::param("option", "option is required"))?;
        let value = match &p["value"] {
            Value::Null => None,
            Value::Bool(b) => Some(*b),
            _ => {
                return Err(IpcError::param(
                    "value",
                    "value is true, false or omitted (toggle)",
                ));
            }
        };
        let mut st = self.lock();
        let r = st.reg.resolve(target)?;
        let t = &mut st.reg.terminals[r.terminal];
        let tid = t.id.clone();
        let w = &mut t.windows[r.window];
        let now = w
            .options
            .set(name, value)
            .map_err(|m| IpcError::param("option", m))?;
        let (wid, options) = (w.id.clone(), w.options.json());
        let data = json!({"terminal_id": tid, "window_id": wid, "options": options});
        st.notify("terminals", "window.options", &data);
        Ok(
            json!({"terminal_id": tid, "window_id": wid, "option": name, "value": now, "options": options}),
        )
    }

    /// `state.status`: where the state lives, what was loaded and written, the saved records.
    fn state_status(&self) -> Value {
        let st = self.lock();
        let p = &st.persist;
        json!({
            "enabled": p.store.is_some(),
            "path": p.store.as_ref().map(|s| s.file().display().to_string()),
            "read_only": p.read_only,
            "loaded": p.loaded,
            "writes": p.writes,
            "last_write": p.last_write,
            "last_bytes": p.last_bytes,
            "last_error": p.last_error,
            "saved": p.saved.iter().map(saved_summary).collect::<Vec<_>>(),
            "live": st.reg.terminals.iter().map(|t| json!({"id": t.id, "name": t.name.display,
                "restore": p.restore.get(&t.id).map_or("ask", String::as_str)})).collect::<Vec<_>>(),
        })
    }

    /// The saved record a target names (its id or name; names are compared ignoring case).
    fn saved_index(st: &State, target: &str) -> Result<usize, IpcError> {
        let key = target.to_lowercase();
        st.persist
            .saved
            .iter()
            .position(|t| {
                t["id"] == target || t["name"].as_str().is_some_and(|n| n.to_lowercase() == key)
            })
            .ok_or_else(|| {
                IpcError::new(
                    ErrorCode::TargetNotFound,
                    format!("no saved Terminal `{target}` (live Terminals are in `subak ls`)"),
                )
            })
    }

    /// `state.forget`: removes a saved record (a live Terminal is not saved: end it instead).
    fn state_forget(&self, p: &Value) -> Result<Value, IpcError> {
        let target = target(p)?;
        let mut st = self.lock();
        if st.persist.read_only.is_some() || st.persist.store.is_none() {
            return Err(IpcError::new(
                ErrorCode::InvalidState,
                "this Host does not write state",
            ));
        }
        let i = Self::saved_index(&st, target)?;
        let summary = saved_summary(&st.persist.saved[i]);
        if flag(p, "dry_run")? {
            return Ok(json!({"dry_run": true, "would_forget": summary}));
        }
        st.persist.saved.remove(i);
        st.mark_dirty();
        Ok(json!({"forgotten": summary}))
    }

    /// `state.set_restore`: whether a Terminal is restored when Subak next opens (`ask`, `auto`,
    /// `never`); a live or a saved one. Changing it starts and ends nothing.
    fn state_set_restore(&self, p: &Value) -> Result<Value, IpcError> {
        let target = target(p)?;
        let restore = optional_string(p, "restore")?
            .filter(|r| matches!(*r, "ask" | "auto" | "never"))
            .ok_or_else(|| IpcError::param("restore", "restore is ask, auto or never"))?
            .to_string();
        let mut st = self.lock();
        if st.persist.read_only.is_some() || st.persist.store.is_none() {
            return Err(IpcError::new(
                ErrorCode::InvalidState,
                "this Host does not write state",
            ));
        }
        let (id, name, live) = if let Ok(i) = st.reg.resolve_terminal(target) {
            let t = &st.reg.terminals[i];
            (t.id.clone(), t.name.display.clone(), true)
        } else {
            let i = Self::saved_index(&st, target)?;
            let t = &st.persist.saved[i];
            (
                t["id"].as_str().unwrap_or("").to_string(),
                t["name"].as_str().unwrap_or("").to_string(),
                false,
            )
        };
        let result = json!({"terminal_id": id, "name": name, "live": live, "restore": restore});
        if flag(p, "dry_run")? {
            return Ok(json!({"dry_run": true, "would_set": result}));
        }
        if live {
            st.persist.restore.insert(id, restore);
        } else if let Some(t) = st.persist.saved.iter_mut().find(|t| t["id"] == id) {
            t["restore"] = json!(restore);
        }
        st.mark_dirty();
        Ok(result)
    }

    /// `claude.report` (a `hook` client: `subak claude report` run by a Claude Code hook): one
    /// event of one session in a pane. Believed only from a process **in that pane's job**;
    /// carries ids, the event and a coarse reason, nothing else (ADR 0061).
    fn claude_report(&self, conn: u64, p: &Value) -> Result<Value, IpcError> {
        let field = |k: &str, max: usize| -> Result<String, IpcError> {
            p[k].as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= max
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                })
                .map(String::from)
                .ok_or_else(|| {
                    IpcError::param(k, format!("{k} is 1-{max} letters, digits, - or _"))
                })
        };
        let pane_id = field("pane_id", 64)?;
        let session_id = field("session_id", 128)?;
        let event = field("event", 64)?;
        let reason = match &p["reason"] {
            Value::Null => None,
            _ => Some(field("reason", 32)?),
        };
        let (rt, pid) = {
            let st = self.lock();
            let pid = st.conns.get(&conn).and_then(|c| c.pid);
            (st.rt.get(&pane_id).cloned(), pid)
        };
        let Some(rt) = rt else {
            return Err(IpcError::new(
                ErrorCode::TargetNotFound,
                format!("no running pane `{pane_id}`"),
            ));
        };
        if !pid.is_some_and(|pid| rt.pty.contains(pid)) {
            let mut st = self.lock();
            st.stats.claude_refused += 1;
            return Err(IpcError::new(
                ErrorCode::Unauthorized,
                "a report for a pane is accepted only from a process running in it",
            ));
        }
        let mut st = self.lock();
        st.stats.claude_reports += 1;
        let before = st.claude_of(&rt.terminal_id);
        let sessions = st.claude.entry(pane_id.clone()).or_default();
        let previous = sessions.get(&session_id).map(|s| s.state.clone());
        let now_state = previous.clone().unwrap_or_else(|| "unknown".into());
        let next = claude::next_state(&event, reason.as_deref(), &now_state);
        match &next {
            Some(state) => {
                sessions.insert(
                    session_id.clone(),
                    ClaudeSession {
                        terminal_id: rt.terminal_id.clone(),
                        state: state.clone(),
                        event: event.clone(),
                        reason,
                        since: SystemTime::now(),
                        at: Instant::now(),
                        stale_told: false,
                    },
                );
            }
            None => {
                sessions.remove(&session_id);
            }
        }
        if sessions.is_empty() {
            st.claude.remove(&pane_id);
        }
        let state = next.unwrap_or_else(|| "none".into());
        if previous.as_deref() != Some(state.as_str()) {
            let data = json!({"pane_id": pane_id, "terminal_id": rt.terminal_id,
                "session_id": session_id, "state": state, "previous_state": previous,
                "event": event});
            st.notify("claude", "claude.state_changed", &data);
        }
        st.claude_changed(&rt.terminal_id, before);
        drop(st);
        // The supervisor recomputes when the next session could go stale.
        let _ = self.events.send(Ev::Changed);
        Ok(json!({"pane_id": pane_id, "session_id": session_id, "state": state}))
    }

    /// `claude.status`: the Claude Code sessions Subak knows of (all panes, or a target's).
    /// A pane's state is its most recently reported session's; `unknown` after
    /// [`CLAUDE_STALE`] in progress without a hook.
    fn claude_status(&self, p: &Value) -> Result<Value, IpcError> {
        let st = self.lock();
        let only: Option<Vec<String>> = match optional_string(p, "target")? {
            None => None,
            Some(t) => {
                let r = st.reg.resolve(t)?;
                let term = &st.reg.terminals[r.terminal];
                Some(match r.level {
                    Level::Pane => vec![term.windows[r.window].panes[r.pane].id.clone()],
                    Level::Window => term.windows[r.window]
                        .panes
                        .iter()
                        .map(|x| x.id.clone())
                        .collect(),
                    Level::Terminal => term
                        .windows
                        .iter()
                        .flat_map(|w| w.panes.iter().map(|x| x.id.clone()))
                        .collect(),
                })
            }
        };
        let shown = |s: &ClaudeSession| -> (String, bool) {
            let state = s.shown();
            (
                state.to_string(),
                state == "unknown" && s.state == "in-progress",
            )
        };
        let mut panes: Vec<Value> = st
            .claude
            .iter()
            .filter(|(id, _)| only.as_ref().is_none_or(|o| o.contains(id)))
            .map(|(id, sessions)| {
                let latest = sessions
                    .iter()
                    .max_by_key(|(_, s)| s.at)
                    .map(|(sid, s)| (sid.clone(), s.clone()))
                    .expect("never empty");
                let (state, stale) = shown(&latest.1);
                json!({
                    "pane_id": id,
                    "terminal_id": latest.1.terminal_id,
                    "state": state,
                    "stale": stale,
                    "since": proto::rfc3339(latest.1.since),
                    "session_id": latest.0,
                    "sessions": sessions.iter().map(|(sid, s)| {
                        let (state, stale) = shown(s);
                        json!({"session_id": sid, "state": state, "stale": stale,
                            "event": s.event, "reason": s.reason,
                            "since": proto::rfc3339(s.since)})
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        panes.sort_by(|a, b| a["pane_id"].as_str().cmp(&b["pane_id"].as_str()));
        Ok(json!({"panes": panes, "persisted": false}))
    }

    /// `window.options` (tmux `show-window-options`).
    fn window_options(&self, p: &Value) -> Result<Value, IpcError> {
        let target = target(p)?;
        let st = self.lock();
        let r = st.reg.resolve(target)?;
        let t = &st.reg.terminals[r.terminal];
        let w = &t.windows[r.window];
        Ok(json!({"terminal_id": t.id, "window_id": w.id, "options": w.options.json()}))
    }

    /// `hook.set` (tmux `set-hook`): the tmux command line `command` runs after `name`'s event
    /// (null removes it). Hooks run tmux commands only, never a shell; their own events never
    /// trigger hooks.
    fn hook_set(&self, p: &Value) -> Result<Value, IpcError> {
        let name = optional_string(p, "name")?
            .ok_or_else(|| IpcError::param("name", "name is required"))?;
        if !HOOKS.iter().any(|(h, _)| *h == name) {
            return Err(IpcError::param(
                "name",
                format!(
                    "unknown hook `{name}`; Subak runs {}",
                    HOOKS.iter().map(|h| h.0).collect::<Vec<_>>().join(", ")
                ),
            ));
        }
        let mut st = self.lock();
        match optional_string(p, "command")? {
            None => {
                st.hooks.remove(name);
            }
            Some(line) => {
                if line.len() > MAX_HOOK_LINE {
                    return Err(IpcError::param(
                        "command",
                        "a hook's command line has at most 1024 bytes",
                    ));
                }
                let cmds = crate::tmuxcmd::parse(line).map_err(|e| {
                    IpcError::param("command", format!("not a tmux command line: {}", e.message))
                })?;
                if let Some(c) = cmds.iter().find(|c| c.destructive()) {
                    return Err(IpcError::param(
                        "command",
                        format!(
                            "a hook cannot run `{}`: hooks never end processes",
                            c.name()
                        ),
                    ));
                }
                if !st.hooks.contains_key(name) && st.hooks.len() >= MAX_HOOKS {
                    return Err(IpcError::new(ErrorCode::Busy, "at most 32 hooks"));
                }
                st.hooks.insert(name.to_string(), line.to_string());
            }
        }
        let hooks = st.hooks.clone();
        Ok(json!({"hooks": hooks}))
    }

    /// `hook.list` (tmux `show-hooks`).
    fn hook_list(&self) -> Value {
        let st = self.lock();
        json!({"hooks": st.hooks, "available": HOOKS.iter().map(|h| h.0).collect::<Vec<_>>()})
    }

    /// Runs one hook (the hook thread): each command of its line in the event's context, as a
    /// cli client would; a refusal stops the line and is counted, never shown to anyone.
    fn run_hook(self: &Arc<Self>, run: &HookRun) {
        use crate::tmuxcmd::{self, Context, Plan};
        let Ok(cmds) = tmuxcmd::parse(&run.line) else {
            return;
        };
        let ctx = Context {
            terminal: run.terminal.clone(),
            window: run.window.clone(),
            pane: run.pane.clone().or_else(|| run.terminal.clone()),
            // A hook never gives consent: `kill-*` in a hook is refused (nothing is ended
            // without the user asking for it right then).
            yes: false,
        };
        self.lock().in_hook = true;
        for c in &cmds {
            let Ok(Plan::Request { method, params }) = tmuxcmd::plan(c, &ctx) else {
                break;
            };
            let req = Request {
                id: 0,
                method: method.to_string(),
                params,
            };
            let ok = matches!(self.handle(HOOK_CONN, &req), Reply::Now(Ok(_)));
            let mut st = self.lock();
            if ok {
                st.stats.hooks_run += 1;
            } else {
                st.stats.hooks_failed += 1;
                break;
            }
        }
        self.lock().in_hook = false;
        let _ = &run.hook;
    }

    /// The running pane a target means (a Terminal or window: its active pane), for input.
    fn input_pane(&self, p: &Value) -> Result<(Arc<PaneRt>, String), IpcError> {
        let target = target(p)?;
        let st = self.lock();
        let r = st.reg.resolve(target)?;
        let t = &st.reg.terminals[r.terminal];
        require_running(t)?;
        if t.ending.is_some() {
            return Err(IpcError::new(
                ErrorCode::InvalidState,
                "the Terminal is ending",
            ));
        }
        let w = &t.windows[r.window];
        let pi = if r.level == Level::Pane {
            r.pane
        } else {
            w.active_pane
        };
        let id = w.panes[pi].id.clone();
        let rt = st
            .rt
            .get(&id)
            .cloned()
            .ok_or_else(|| IpcError::new(ErrorCode::InvalidState, "the pane has ended"))?;
        Ok((rt, id))
    }

    /// Writes input to a pane as if a client typed it (counted as input, never logged).
    /// Step 25 (tmux `synchronize-panes`): the other running panes of `rt`'s window when the
    /// window synchronizes its panes' input; none otherwise.
    fn synced(&self, r: &PaneRt) -> Vec<Arc<PaneRt>> {
        let st = self.lock();
        st.reg
            .get(&r.terminal_id)
            .and_then(|t| {
                t.windows
                    .iter()
                    .find(|w| w.panes.iter().any(|p| p.id == r.pane_id))
            })
            .filter(|w| w.options.synchronize_panes)
            .map(|w| {
                w.panes
                    .iter()
                    .filter(|p| p.id != r.pane_id)
                    .filter_map(|p| st.rt.get(&p.id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn write_to(&self, rt: &PaneRt, bytes: &[u8]) -> Result<(), IpcError> {
        if let Some(o) = self.lock().out.get_mut(&rt.pane_id) {
            o.input_bytes += bytes.len() as u64;
        }
        rt.pty
            .write_input(bytes)
            .map_err(|e| IpcError::new(ErrorCode::InternalError, format!("input failed: {e}")))
    }

    /// `pane.send_keys` (tmux `send-keys`): tmux key names (`Enter`, `C-c`, `Up`, `F5`, …; any
    /// other word as its text) or, with `literal`, the words as text, to the target pane.
    fn send_keys(&self, p: &Value) -> Result<Value, IpcError> {
        let keys: Vec<String> = match &p["keys"] {
            Value::Array(a) => a
                .iter()
                .map(|k| k.as_str().map(String::from))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| IpcError::param("keys", "keys must be strings"))?,
            _ => return Err(IpcError::param("keys", "keys is a list of strings")),
        };
        let literal = flag(p, "literal")?;
        let bytes: Vec<u8> = keys
            .iter()
            .flat_map(|k| {
                if literal {
                    k.as_bytes().to_vec()
                } else {
                    crate::tmuxcmd::key_bytes(k)
                }
            })
            .collect();
        if bytes.len() > MAX_BUFFER_BYTES {
            return Err(IpcError::param("keys", "at most 1 MiB of input at a time"));
        }
        let (rt, pane_id) = self.input_pane(p)?;
        if flag(p, "dry_run")? {
            return Ok(
                json!({"dry_run": true, "would_send": {"pane_id": pane_id, "bytes": bytes.len()}}),
            );
        }
        self.write_to(&rt, &bytes)?;
        // As in tmux, keys sent to a synchronized window reach all its panes.
        for o in self.synced(&rt) {
            let _ = self.write_to(&o, &bytes);
        }
        Ok(json!({"pane_id": pane_id, "terminal_id": rt.terminal_id, "bytes": bytes.len()}))
    }

    /// `buffer.set` (tmux `set-buffer`): a new buffer (named, or `bufferN`), or `append` to one.
    fn buffer_set(&self, p: &Value) -> Result<Value, IpcError> {
        let data = optional_string(p, "data")?
            .ok_or_else(|| IpcError::param("data", "data is required"))?;
        let name = optional_string(p, "name")?;
        if let Some(n) = name
            && (n.is_empty() || n.chars().count() > 64 || n.chars().any(char::is_control))
        {
            return Err(IpcError::param(
                "name",
                "a buffer name has 1-64 visible characters",
            ));
        }
        let append = flag(p, "append")?;
        if flag(p, "dry_run")? {
            return Ok(json!({"dry_run": true, "would_set": {
                "name": name, "bytes": data.len(), "append": append}}));
        }
        let mut st = self.lock();
        let existing = name.and_then(|n| st.buffers.iter().position(|b| b.name == n));
        let new_len =
            existing.map_or(0, |i| if append { st.buffers[i].data.len() } else { 0 }) + data.len();
        if new_len > MAX_BUFFER_BYTES {
            return Err(IpcError::param("data", "a buffer holds at most 1 MiB"));
        }
        let name = match (existing, name) {
            (Some(i), _) => {
                let mut b = st.buffers.remove(i);
                if append {
                    b.data.push_str(data);
                } else {
                    b.data = data.to_string();
                }
                let n = b.name.clone();
                st.buffers.insert(0, b);
                n
            }
            (None, given) => {
                let named = given.is_some();
                let n = given.map_or_else(
                    || {
                        let n = format!("buffer{}", st.next_buffer);
                        st.next_buffer += 1;
                        n
                    },
                    String::from,
                );
                st.buffers.insert(
                    0,
                    PasteBuffer {
                        name: n.clone(),
                        data: data.to_string(),
                        named,
                    },
                );
                if st.buffers.len() > MAX_BUFFERS {
                    // tmux: the oldest automatic buffer goes first.
                    let drop = st
                        .buffers
                        .iter()
                        .rposition(|b| !b.named)
                        .unwrap_or(st.buffers.len() - 1);
                    st.buffers.remove(drop);
                }
                n
            }
        };
        let bytes = st.buffers[0].data.len();
        Ok(json!({"name": name, "bytes": bytes, "buffers": st.buffers.len()}))
    }

    /// `buffer.list` (tmux `list-buffers`): names, sizes and a redacted one-line preview.
    fn buffer_list(&self) -> Value {
        let st = self.lock();
        let list: Vec<Value> = st
            .buffers
            .iter()
            .map(|b| {
                let first: String = b
                    .data
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(60)
                    .collect();
                json!({
                    "name": b.name,
                    "bytes": b.data.len(),
                    "lines": b.data.lines().count(),
                    "preview": redact::command_line(&first),
                })
            })
            .collect();
        json!({"buffers": list, "persisted": false})
    }

    fn buffer_index(st: &State, p: &Value) -> Result<usize, IpcError> {
        match optional_string(p, "name")? {
            None if st.buffers.is_empty() => Err(IpcError::new(
                ErrorCode::TargetNotFound,
                "there is no paste buffer",
            )
            .details(json!({"hint": "copy in copy mode (Ctrl+B [) or `subak buffer set`"}))),
            None => Ok(0),
            Some(n) => st.buffers.iter().position(|b| b.name == n).ok_or_else(|| {
                IpcError::new(ErrorCode::TargetNotFound, format!("no buffer `{n}`"))
            }),
        }
    }

    /// `buffer.get` (tmux `show-buffer`): one buffer's text (the newest without a name).
    fn buffer_get(&self, p: &Value) -> Result<Value, IpcError> {
        let st = self.lock();
        let i = Self::buffer_index(&st, p)?;
        let b = &st.buffers[i];
        Ok(json!({"name": b.name, "bytes": b.data.len(), "data": b.data}))
    }

    /// `buffer.delete` (tmux `delete-buffer`).
    fn buffer_delete(&self, p: &Value) -> Result<Value, IpcError> {
        let mut st = self.lock();
        let i = Self::buffer_index(&st, p)?;
        if flag(p, "dry_run")? {
            let b = &st.buffers[i];
            return Ok(
                json!({"dry_run": true, "would_delete": {"name": b.name, "bytes": b.data.len()}}),
            );
        }
        let b = st.buffers.remove(i);
        Ok(json!({"deleted": b.name, "buffers": st.buffers.len()}))
    }

    /// `buffer.paste` (tmux `paste-buffer`): a buffer's text as input to the target pane, line
    /// ends as carriage returns (tmux without `-r`); `delete` drops the buffer afterwards.
    fn buffer_paste(&self, p: &Value) -> Result<Value, IpcError> {
        let data = {
            let st = self.lock();
            let i = Self::buffer_index(&st, p)?;
            (st.buffers[i].name.clone(), st.buffers[i].data.clone())
        };
        let (rt, pane_id) = self.input_pane(p)?;
        let text = data.1.replace("\r\n", "\r").replace('\n', "\r");
        if flag(p, "dry_run")? {
            return Ok(
                json!({"dry_run": true, "would_paste": {"name": data.0, "pane_id": pane_id, "bytes": text.len()}}),
            );
        }
        self.write_to(&rt, text.as_bytes())?;
        if flag(p, "delete")? {
            let mut st = self.lock();
            st.buffers.retain(|b| b.name != data.0);
        }
        Ok(json!({"name": data.0, "pane_id": pane_id, "bytes": text.len()}))
    }

    /// `pane.break` (tmux `break-pane`): the target pane leaves its window for a new window of
    /// its own (named `name`, or after its program); its process keeps running. `select` (default
    /// true) shows it.
    fn break_pane(&self, p: &Value) -> Result<Value, IpcError> {
        let target = target(p)?;
        let name = optional_string(p, "name")?
            .map(session::parse_window_name)
            .transpose()?;
        let select = !matches!(p["select"], Value::Bool(false));
        let dry_run = flag(p, "dry_run")?;
        let mut st = self.lock();
        let r = st.reg.resolve(target)?;
        if r.level != Level::Pane {
            return Err(wrong_level(
                "pane.break",
                "a pane (terminal:window.pane or a p- id)",
            ));
        }
        let t = &st.reg.terminals[r.terminal];
        require_running(t)?;
        let pane = t.windows[r.window].panes[r.pane].clone();
        if t.windows[r.window].panes.len() < 2 {
            return Err(IpcError::new(
                ErrorCode::InvalidState,
                "the pane is its window's only pane: nothing to break it out of",
            ));
        }
        let index = t.free_window_index();
        let tid = t.id.clone();
        if dry_run {
            return Ok(json!({"dry_run": true, "would_break": {
                "terminal_id": tid, "pane_id": pane.id, "window_index": index}}));
        }
        let wid = st.reg.issue_id('w', proto::new_object_id);
        let automatic = name.is_none();
        let wname = name.unwrap_or_else(|| session::window_name(&pane.command.program));
        let t = st.reg.get_mut(&tid).expect("resolved above");
        let (wc0, pc0) = (t.active_window, t.active_pane().id.clone());
        let (new, old, resized) = t
            .break_pane(
                &pane.id,
                session::Window {
                    id: wid.clone(),
                    index,
                    name: wname,
                    automatic_name: automatic,
                    panes: Vec::new(),
                    active_pane: 0,
                    layout: None,
                    options: session::WindowOptions::new(),
                    activity: false,
                    bell: false,
                    last_pane: None,
                },
                select,
            )
            .map_err(|m| IpcError::new(ErrorCode::InvalidState, m))?;
        let t = st.reg.get(&tid).expect("resolved above");
        let window = st.window_json(t, &t.windows[new]);
        let wc = t.windows[t.active_window].id
            != t.windows.get(wc0).map_or(String::new(), |w| w.id.clone())
            || select;
        let pc = t.active_pane().id != pc0;
        let data = json!({"terminal_id": tid, "window": window});
        st.notify("terminals", "window.created", &data);
        st.layout_event(&tid, &old);
        st.selected(&tid, wc && select, pc);
        let resizes = st.resized(resized);
        drop(st);
        apply_resizes(&resizes)?;
        Ok(
            json!({"terminal_id": tid, "window_id": wid, "pane_id": pane.id, "selected": select, "window": window}),
        )
    }

    /// `pane.join` (tmux `join-pane`): the `source` pane moves into the target window as a split
    /// of the target pane (a window target: its active pane), `direction`/`before`/`cells`/
    /// `percent` as in `pane.split`; its old window goes with its last pane. Both panes keep their
    /// processes. Panes of two Terminals are never joined.
    fn join_pane(&self, p: &Value) -> Result<Value, IpcError> {
        let source = optional_string(p, "source")?
            .ok_or_else(|| IpcError::param("source", "source names the pane to move"))?;
        let target = target(p)?;
        let dir = match optional_string(p, "direction")? {
            None | Some("vertical") => layout::Dir::Vertical,
            Some("horizontal") => layout::Dir::Horizontal,
            Some(_) => return Err(IpcError::param("direction", "horizontal or vertical")),
        };
        let before = flag(p, "before")?;
        let amount = match (p["cells"].as_u64(), p["percent"].as_u64()) {
            (Some(c), None) => Some(layout::Amount::Cells(
                u16::try_from(c.min(1000)).unwrap_or(1000),
            )),
            (None, Some(c)) if (1..=99).contains(&c) => {
                Some(layout::Amount::Percent(u8::try_from(c).unwrap_or(50)))
            }
            (None, None) => None,
            _ => {
                return Err(IpcError::param(
                    "cells",
                    "give cells (1-1000) or percent (1-99)",
                ));
            }
        };
        let select = !matches!(p["select"], Value::Bool(false));
        let dry_run = flag(p, "dry_run")?;
        let mut st = self.lock();
        let s = st.reg.resolve(source)?;
        if s.level != Level::Pane {
            return Err(IpcError::param("source", "source must name a pane"));
        }
        let r = st.reg.resolve(target)?;
        if r.level == Level::Terminal && r.terminal != s.terminal {
            return Err(IpcError::new(
                ErrorCode::InvalidState,
                "panes of two Terminals are never joined",
            ));
        }
        if r.terminal != s.terminal {
            return Err(IpcError::new(
                ErrorCode::InvalidState,
                "panes of two Terminals are never joined",
            ));
        }
        if r.window == s.window {
            return Err(IpcError::new(
                ErrorCode::InvalidState,
                "the pane is already in that window (use pane.swap or pane.adjust)",
            ));
        }
        let t = &st.reg.terminals[r.terminal];
        require_running(t)?;
        let pane_id = t.windows[s.window].panes[s.pane].id.clone();
        let tw = &t.windows[r.window];
        let of = if r.level == Level::Pane {
            tw.panes[r.pane].id.clone()
        } else {
            tw.panes[tw.active_pane].id.clone()
        };
        let (tid, wid) = (t.id.clone(), tw.id.clone());
        let mut probe = tw.layout().clone();
        probe
            .split(&of, &pane_id, dir, before, amount)
            .map_err(|e| layout_refused(&e, "join the pane"))?;
        if dry_run {
            return Ok(json!({"dry_run": true, "would_join": {
                "terminal_id": tid, "pane_id": pane_id, "window_id": wid, "split": of}}));
        }
        let t = st.reg.get_mut(&tid).expect("resolved above");
        let pc0 = t.active_pane().id.clone();
        let wc0 = t.windows[t.active_window].id.clone();
        let (gone, old, resized) = t
            .join_pane(
                &pane_id,
                &wid,
                &session::SplitAt {
                    of: &of,
                    dir,
                    before,
                    amount,
                },
                select,
            )
            .map_err(|e| layout_refused(&e, "join the pane"))?;
        let t = st.reg.get(&tid).expect("resolved above");
        let (wc, pc) = (
            t.windows[t.active_window].id != wc0,
            t.active_pane().id != pc0,
        );
        if gone {
            let data = json!({"terminal_id": tid, "window_id": old, "reason": "joined"});
            st.notify("terminals", "window.removed", &data);
        } else {
            st.layout_event(&tid, &old);
        }
        st.layout_event(&tid, &wid);
        st.selected(&tid, wc, pc);
        let t = st.reg.get(&tid).expect("resolved above");
        let window = t
            .windows
            .iter()
            .find(|w| w.id == wid)
            .map(|w| st.window_json(t, w));
        let resizes = st.resized(resized);
        drop(st);
        apply_resizes(&resizes)?;
        Ok(
            json!({"terminal_id": tid, "window_id": wid, "pane_id": pane_id,
            "window_removed": gone, "selected": select, "window": window}),
        )
    }

    /// `pane.adjust` (tmux `resize-pane -L/-R/-U/-D`), `pane.swap` (`swap-pane`) and `pane.zoom`
    /// (`resize-pane -Z`) on the target pane (or the active pane of a target window or
    /// Terminal) of a running Terminal (Step 22, ADR 0056). Every pane keeps its process; panes
    /// whose share changed are resized. `dry_run` reports the resulting layout and changes
    /// nothing.
    fn layout_op(&self, p: &Value, op: LayoutOp) -> Result<Value, IpcError> {
        let method = match op {
            LayoutOp::Adjust => "pane.adjust",
            LayoutOp::Swap => "pane.swap",
            LayoutOp::Zoom => "pane.zoom",
            LayoutOp::Rotate => "window.rotate",
            LayoutOp::Preset => "window.select_layout",
        };
        let up = match (op, optional_string(p, "direction")?) {
            (LayoutOp::Rotate, None | Some("up")) => true,
            (LayoutOp::Rotate, Some("down")) => false,
            (LayoutOp::Rotate, Some(_)) => {
                return Err(IpcError::param("direction", "direction must be up or down"));
            }
            _ => true,
        };
        let preset = match op {
            LayoutOp::Preset => Some(match optional_string(p, "layout")? {
                Some("next") => Pick::Next,
                Some("previous") => Pick::Previous,
                Some("spread") => Pick::Spread,
                Some(l) => Pick::Preset(layout::Preset::parse(l).ok_or_else(|| {
                    IpcError::param(
                        "layout",
                        "layout is even-horizontal, even-vertical, main-horizontal(-mirrored), main-vertical(-mirrored), tiled, next, previous or spread",
                    )
                })?),
                None => return Err(IpcError::param("layout", "layout is required")),
            }),
            _ => None,
        };
        let target = target(p)?;
        let dry_run = flag(p, "dry_run")?;
        let side = match op {
            LayoutOp::Adjust => Some(
                optional_string(p, "direction")?
                    .and_then(layout::Side::parse)
                    .ok_or_else(|| {
                        IpcError::param("direction", "direction must be left, right, up or down")
                    })?,
            ),
            _ => None,
        };
        let cells = match &p["cells"] {
            Value::Null => 1,
            v => v
                .as_u64()
                .filter(|n| (1..=1000).contains(n))
                .and_then(|n| u16::try_from(n).ok())
                .ok_or_else(|| IpcError::param("cells", "cells must be a number from 1 to 1000"))?,
        };
        let zoom = match &p["zoom"] {
            Value::Null => None,
            Value::Bool(b) => Some(*b),
            _ => {
                return Err(IpcError::param(
                    "zoom",
                    "zoom must be true or false (omit it to toggle)",
                ));
            }
        };
        let mut st = self.lock();
        let r = st.reg.resolve(target)?;
        let t = &st.reg.terminals[r.terminal];
        require_running(t)?;
        let w = &t.windows[r.window];
        let window_op = matches!(op, LayoutOp::Rotate | LayoutOp::Preset);
        let at = if r.level == Level::Pane && !window_op {
            r.pane
        } else {
            w.active_pane
        };
        let pane_id = w.panes[at].id.clone();
        // The other pane of a swap: a target in the same window, or the next or previous one.
        let other = if matches!(op, LayoutOp::Swap) {
            let n = w.panes.len();
            Some(
                match (
                    optional_string(p, "with")?,
                    optional_string(p, "direction")?,
                ) {
                    (Some(with), None) => {
                        let o = st.reg.resolve(with)?;
                        if o.level != Level::Pane {
                            return Err(IpcError::param("with", "with must name a pane"));
                        }
                        if o.terminal != r.terminal || o.window != r.window {
                            return Err(IpcError::new(
                                ErrorCode::InvalidState,
                                "pane.swap swaps two panes of one window",
                            ));
                        }
                        w.panes[o.pane].id.clone()
                    }
                    (None, Some("next")) => w.panes[(at + 1) % n].id.clone(),
                    (None, Some("previous")) => w.panes[(at + n - 1) % n].id.clone(),
                    (None, None) => {
                        return Err(IpcError::param(
                            "with",
                            "name the pane to swap with (with), or a direction",
                        ));
                    }
                    _ => {
                        return Err(IpcError::param(
                            "direction",
                            "give with (a pane) or direction (next or previous), not both",
                        ));
                    }
                },
            )
        } else {
            None
        };
        let mut next = w.clone();
        let before = next.layout().clone();
        let (moved, changed) = next
            .relayout(|l| match op {
                LayoutOp::Adjust => l.adjust(&pane_id, side.expect("parsed above"), cells),
                LayoutOp::Swap => l
                    .swap(&pane_id, other.as_deref().expect("set above"))
                    .map(|()| 0),
                LayoutOp::Zoom => {
                    let on = zoom.unwrap_or(l.zoomed() != Some(pane_id.as_str()));
                    l.zoom(on.then_some(pane_id.as_str())).map(|()| 0)
                }
                LayoutOp::Rotate => {
                    l.rotate(up);
                    Ok(0)
                }
                LayoutOp::Preset => {
                    match preset.expect("parsed above") {
                        Pick::Preset(p) => l.preset(p),
                        Pick::Next => {
                            l.cycle(true);
                        }
                        Pick::Previous => {
                            l.cycle(false);
                        }
                        Pick::Spread => l.spread(),
                    }
                    Ok(0)
                }
            })
            .map_err(|e| layout_refused(&e, method))?;
        let did = *next.layout() != before;
        let mut out = layout_json(t, &next);
        out["pane_id"] = json!(pane_id);
        out["changed"] = json!(did);
        if matches!(op, LayoutOp::Adjust) {
            out["moved"] = json!(moved);
        }
        if dry_run {
            return Ok(json!({"dry_run": true, "would_change": out}));
        }
        let tid = t.id.clone();
        let wid = w.id.clone();
        // Zooming a pane shows it: it becomes the window's active pane.
        let select = matches!(op, LayoutOp::Zoom) && next.layout().zoomed().is_some();
        let t = &mut st.reg.terminals[r.terminal];
        t.windows[r.window] = next;
        let (wc, pc) = if select {
            let pi = t.windows[r.window]
                .panes
                .iter()
                .position(|q| q.id == pane_id)
                .unwrap_or(at);
            t.select_pane(r.window, pi)
        } else {
            (false, false)
        };
        let w = &t.windows[r.window];
        out["active_pane"] = json!(w.panes[w.active_pane].id);
        st.selected(&tid, wc, pc);
        if did {
            st.layout_event(&tid, &wid);
        }
        let resizes = st.resized(changed);
        drop(st);
        apply_resizes(&resizes)?;
        Ok(out)
    }

    /// `window.rename` (tmux `rename-window`): a window's name; it stops following its program.
    fn window_rename(&self, p: &Value) -> Result<Value, IpcError> {
        let target = target(p)?;
        let raw = optional_string(p, "name")?
            .ok_or_else(|| IpcError::param("name", "name is required"))?;
        let name = session::parse_window_name(raw)?;
        let dry_run = flag(p, "dry_run")?;
        let mut st = self.lock();
        let r = st.reg.resolve(target)?;
        if r.level != Level::Window {
            return Err(wrong_level(
                "window.rename",
                "a window (terminal:window or a w- id)",
            ));
        }
        let t = &mut st.reg.terminals[r.terminal];
        let tid = t.id.clone();
        let w = &mut t.windows[r.window];
        let (wid, previous) = (w.id.clone(), w.name.clone());
        if dry_run {
            return Ok(json!({"dry_run": true, "would_rename": {
                "terminal_id": tid, "window_id": wid, "from": previous, "to": name,
            }}));
        }
        let changed = previous != name || w.automatic_name;
        w.name.clone_from(&name);
        w.automatic_name = false;
        if changed {
            let data = json!({
                "terminal_id": tid, "window_id": wid, "name": name, "previous_name": previous,
            });
            st.notify("terminals", "window.renamed", &data);
        }
        Ok(json!({
            "terminal_id": tid, "window_id": wid, "name": name, "previous_name": previous,
            "changed": changed,
        }))
    }

    /// `window.kill` and `pane.kill` (tmux `kill-window`, `kill-pane`): end the processes of one
    /// window's panes, or of one pane, at once, and take them out of the Terminal. Only that
    /// target's jobs are terminated; every other pane keeps running. A busy pane (more than an
    /// idle shell) needs `force`, as for `terminal.kill`. The last running window or pane of a
    /// Terminal is refused (`invalid_state`): ending it would end the Terminal, which only
    /// `terminal.kill` does.
    fn close(&self, p: &Value, window: bool) -> Result<Value, IpcError> {
        let method = if window { "window.kill" } else { "pane.kill" };
        let target = target(p)?;
        let force = flag(p, "force")?;
        let dry_run = flag(p, "dry_run")?;
        let (rts, tid, wid) = {
            let st = self.lock();
            let r = st.reg.resolve(target)?;
            match (window, r.level) {
                (true, Level::Window) | (false, Level::Pane) => {}
                (true, _) => {
                    return Err(wrong_level(
                        method,
                        "a window (terminal:window or a w- id); `terminal.kill` ends a Terminal",
                    ));
                }
                (false, _) => {
                    return Err(wrong_level(
                        method,
                        "a pane (terminal:window.pane or a p- id); `terminal.kill` ends a Terminal",
                    ));
                }
            }
            let t = &st.reg.terminals[r.terminal];
            require_running(t)?;
            let w = &t.windows[r.window];
            let scope: Vec<&session::Pane> = if window {
                w.panes.iter().collect()
            } else {
                vec![&w.panes[r.pane]]
            };
            let ids: Vec<&str> = scope.iter().map(|p| p.id.as_str()).collect();
            if st.others_running(t, &ids) == 0 {
                let what = if window { "window" } else { "pane" };
                return Err(IpcError::new(
                    ErrorCode::InvalidState,
                    format!(
                        "this is the last running {what} of Terminal `{}`; ending it would end the Terminal",
                        t.name.display
                    ),
                )
                .details(json!({
                    "terminal_id": t.id, "state": t.state.as_str(),
                    "reason": if window { "last_window" } else { "last_pane" },
                    "hint": format!("to end the whole Terminal use `subak kill {}`", t.name.display),
                })));
            }
            let panes: Vec<Value> = scope
                .iter()
                .map(|p| {
                    let l = st.live(p);
                    json!({
                        "pane_id": p.id, "index": p.index, "pid": p.pid.filter(|_| l.running),
                        "program": redact::program(&p.command.program),
                        "processes": l.processes,
                        "busy": l.running && session::busy(&p.command.program, l.processes),
                    })
                })
                .collect();
            let busy = panes.iter().any(|p| p["busy"] == true);
            let processes: u64 = panes.iter().filter_map(|p| p["processes"].as_u64()).sum();
            let plan = json!({
                "terminal_id": t.id, "name": t.name.display, "window_id": w.id,
                "window_index": w.index, "panes": panes, "processes": processes, "busy": busy,
                "removes_window": window || w.panes.len() == 1,
            });
            if busy && !force {
                return Err(IpcError::new(
                    ErrorCode::Busy,
                    format!(
                        "the {} runs more than an idle shell ({processes} process(es)); set force to end it",
                        if window { "window" } else { "pane" }
                    ),
                )
                .details(plan));
            }
            if dry_run {
                return Ok(json!({"dry_run": true, "would_kill": plan}));
            }
            // Claimed here, under the lock that decided "not the last one": a pane that is
            // already ending (its own exit) is left to whoever ends it.
            let rts: Vec<Arc<PaneRt>> = ids
                .iter()
                .filter_map(|id| st.rt.get(*id).cloned())
                .filter(|r| !r.claimed.swap(true, Ordering::SeqCst))
                .collect();
            (rts, t.id.clone(), w.id.clone())
        };
        let affected: Vec<Value> = thread::scope(|s| {
            let handles: Vec<_> = rts
                .iter()
                .map(|rt| s.spawn(move || self.end_alone(rt, true, "killed")))
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        let st = self.lock();
        let t = st.reg.get(&tid);
        Ok(json!({
            "terminal_id": tid,
            "window_id": wid,
            "affected": affected,
            "window_removed": t.is_none_or(|t| t.window_pos(&wid).is_none()),
            "terminal": t.map(|t| st.terminal_json(t)),
        }))
    }

    /// `terminal.stop`, `terminal.kill` and `terminal.remove` (docs/SESSION_MODEL.md §4).
    fn end_request(&self, p: &Value, op: Op) -> Result<Value, IpcError> {
        let target = target(p)?;
        let force = flag(p, "force")?;
        let dry_run = flag(p, "dry_run")?;
        let (tid, name) = {
            let mut st = self.lock();
            let i = st.reg.resolve_terminal(target)?;
            let t = &st.reg.terminals[i];
            let pane = t.active_pane();
            // Step 21: every pane of every window counts; busy when any pane is.
            let lives: Vec<(&session::Pane, session::Live)> =
                t.panes().map(|p| (p, st.live(p))).collect();
            let processes: u32 = lives.iter().map(|(_, l)| l.processes).sum();
            let busy = t.state == session::State::Running
                && lives
                    .iter()
                    .any(|(p, l)| l.running && session::busy(&p.command.program, l.processes));
            let plan = json!({
                "terminal_id": t.id, "name": t.name.display, "state": t.state.as_str(),
                "pane_id": pane.id, "pid": pane.pid, "program": redact::program(&pane.command.program),
                "processes": processes, "busy": busy,
                "ends_processes": t.state.is_live() && !(op == Op::Remove && !force),
                "removes_terminal": op != Op::Stop,
            });
            let (tid, name, state) = (t.id.clone(), t.name.display.clone(), t.state);
            match state {
                session::State::Starting => {
                    return Err(IpcError::new(
                        ErrorCode::InvalidState,
                        format!("Terminal `{name}` is still starting; try again"),
                    )
                    .details(json!({"terminal_id": tid, "state": "starting"})));
                }
                session::State::Running | session::State::Stopping
                    if op == Op::Remove && !force =>
                {
                    return Err(IpcError::new(
                        ErrorCode::InvalidState,
                        format!(
                            "Terminal `{name}` is {}; stop it first, or set force to end its processes and remove it",
                            state.as_str()
                        ),
                    )
                    .details(json!({"terminal_id": tid, "state": state.as_str()})));
                }
                session::State::Running if busy && !force => {
                    return Err(IpcError::new(
                        ErrorCode::Busy,
                        format!(
                            "Terminal `{name}` runs more than an idle shell ({processes} process(es) in its panes' jobs); set force to end them"
                        ),
                    )
                    .details(plan));
                }
                _ => {}
            }
            if dry_run {
                let key = match op {
                    Op::Stop => "would_stop",
                    Op::Kill => "would_kill",
                    Op::Remove => "would_remove",
                };
                return Ok(json!({"dry_run": true, key: plan}));
            }
            if !state.is_live() {
                // Nothing runs: stop is a no-op; kill and remove delete the stored record.
                if op == Op::Stop {
                    return Ok(json!({
                        "affected": [], "terminal": st.terminal_json(t), "removed": false,
                        "terminal_id": tid, "name": name,
                    }));
                }
                st.reg.apply(&tid, Transition::Remove)?;
                st.removed(&tid, &name, "removed");
                drop(st);
                let _ = self.events.send(Ev::Changed);
                return Ok(json!({
                    "affected": [], "terminal": null, "removed": true,
                    "terminal_id": tid, "name": name,
                }));
            }
            (tid, name)
        };
        let reports = match op {
            Op::Stop => self.end_terminal(&tid, EndCause::Stop, false),
            Op::Kill | Op::Remove => self.end_terminal(&tid, EndCause::Kill, true),
        };
        let st = self.lock();
        let after = st.reg.get(&tid);
        Ok(json!({
            "affected": reports,
            "terminal": after.map(|t| st.terminal_json(t)),
            "removed": after.is_none(),
            "terminal_id": tid,
            "name": name,
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Stop,
    Kill,
    Remove,
}

/// Step 29: what `state.status` and the CLI show of a saved record.
fn saved_summary(t: &Value) -> Value {
    let windows = t["windows"].as_array().map_or(0, Vec::len);
    let panes: usize = t["windows"].as_array().map_or(0, |w| {
        w.iter()
            .map(|w| w["panes"].as_array().map_or(0, Vec::len))
            .sum()
    });
    json!({"id": t["id"], "name": t["name"], "restore": t["restore"], "windows": windows, "panes": panes})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    /// A pane whose root has exited but whose job still counts it for the next `lag` queries, as
    /// the kernel does for a moment (it takes an exiting process out of its job only after
    /// waking the threads that wait for its exit). `u32::MAX`: a process stays.
    #[derive(Debug)]
    struct Lagging {
        lag: AtomicU32,
        queries: AtomicU32,
    }

    impl Lagging {
        fn new(lag: u32) -> Self {
            Self {
                lag: AtomicU32::new(lag),
                queries: AtomicU32::new(0),
            }
        }
    }

    impl Pty for Lagging {
        fn pid(&self) -> u32 {
            1
        }
        fn take_output(&self) -> Option<Box<dyn Read + Send>> {
            None
        }
        fn write_input(&self, _: &[u8]) -> std::io::Result<()> {
            Ok(())
        }
        fn resize(&self, _: u16, _: u16) -> std::io::Result<()> {
            Ok(())
        }
        fn close(&self) {}
        fn wait_exit(&self, _: Option<Duration>) -> Option<u32> {
            Some(42)
        }
        fn processes(&self) -> u32 {
            self.queries.fetch_add(1, Ordering::SeqCst);
            let left = self.lag.load(Ordering::SeqCst);
            if left == u32::MAX {
                return 1;
            }
            if left > 0 {
                self.lag.store(left - 1, Ordering::SeqCst);
                1
            } else {
                0
            }
        }
        fn kill_tree(&self, _: u32) {}
    }

    /// The Step 73 CI failure (`last_exit.forced` true for `cmd /c exit 42`): a job that still
    /// counts the exited root for a moment empties within the bound, so the pane ended by
    /// itself; one that keeps a process does not, and the bound holds.
    #[test]
    fn a_job_that_empties_just_after_its_root_exits_ended_by_itself() {
        let p = Lagging::new(5);
        assert_eq!(
            p.processes(),
            1,
            "right after the exit the job still counts the root"
        );
        assert!(job_drained(&p, JOB_SETTLE));
        assert_eq!(p.queries.load(Ordering::SeqCst), 6);
        let busy = Lagging::new(u32::MAX);
        let t = Instant::now();
        assert!(!job_drained(&busy, Duration::from_millis(60)));
        let took = t.elapsed();
        assert!(
            took >= Duration::from_millis(60) && took < Duration::from_secs(2),
            "{took:?}"
        );
        assert!(
            JOB_SETTLE < GRACE,
            "the settle stays well inside a graceful stop"
        );
    }
}
