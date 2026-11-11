//! Step 34 (ADR 0067): SSH sessions as ordinary pane processes.
//!
//! - **Transport:** Windows' own OpenSSH client (`%SystemRoot%\System32\OpenSSH\ssh.exe`,
//!   maintained and serviced by Microsoft) runs in the pane's pseudo console, so an SSH session
//!   is a Terminal like any other: tabs, panes, the Host lifecycle, detach and recovery. Keys,
//!   `~/.ssh/config`, `known_hosts` and the ssh-agent are OpenSSH's own.
//! - **Host keys:** always `StrictHostKeyChecking=ask`: a new host is shown to the user, who
//!   decides; a changed key is refused by OpenSSH with its warning. Subak never answers that
//!   question.
//! - **Secrets:** a password or key passphrase is kept only in Windows Credential Manager
//!   (`mouv-subak:ssh:<user>@<host>:<port>`), saved only for a host already in `known_hosts`, and
//!   handed to ssh through `SSH_ASKPASS` by `subak.exe` itself: it answers password and
//!   passphrase prompts only, once per connection, and refuses everything else (a host-key
//!   question included). Never on a command line, in config, state, logs or JSON.
//! - **No forwarding by default:** `ForwardAgent=no`; nothing is disclosed to the remote side.

use crate::cli::{Args, Cmd, EnvArg};
use crate::output::{CliError, Code};
use serde_json::{Value, json};
use std::path::PathBuf;

/// `[user@]host[:port]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub user: Option<String>,
    pub host: String,
    pub port: u16,
}

fn plain(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._:[]".contains(c))
}

impl Target {
    pub fn parse(text: &str) -> Result<Self, String> {
        let (user, rest) = match text.rsplit_once('@') {
            Some((u, r)) => (Some(u.to_string()), r),
            None => (None, text),
        };
        if let Some(u) = &user
            && (u.is_empty()
                || u.len() > 64
                || !u
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._\\".contains(c)))
        {
            return Err(format!("`{u}` is not a user name"));
        }
        let (host, port) = match rest.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') || h.starts_with('[') => (
                h.trim_matches(['[', ']']).to_string(),
                p.parse::<u16>()
                    .map_err(|_| format!("`{p}` is not a port"))?,
            ),
            _ => (rest.to_string(), 22),
        };
        if !plain(&host) || port == 0 {
            return Err(format!("`{text}` is not [user@]host[:port]"));
        }
        Ok(Self { user, host, port })
    }

    /// The Credential Manager entry of this destination.
    pub fn credential(&self) -> String {
        format!(
            "mouv-subak:ssh:{}@{}:{}",
            self.user.as_deref().unwrap_or(""),
            self.host,
            self.port
        )
    }

    /// How `known_hosts` names it: `host`, or `[host]:port` for another port.
    pub fn known_name(&self) -> String {
        if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        }
    }

    pub fn shown(&self) -> String {
        let u = self
            .user
            .as_ref()
            .map_or(String::new(), |u| format!("{u}@"));
        format!("{u}{}:{}", self.host, self.port)
    }
}

/// Windows' OpenSSH client and key tool.
pub fn openssh(tool: &str) -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("SystemRoot")?)
        .join("System32")
        .join("OpenSSH")
        .join(format!("{tool}.exe"));
    p.is_file().then_some(p)
}

/// The `known_hosts` files ssh reads for this connection: an explicit `UserKnownHostsFile`
/// option, else the first `UserKnownHostsFile` line of the `-F` config file (Step 35), else
/// `~/.ssh/known_hosts`.
pub fn known_hosts_files(options: &[String], config: Option<&str>) -> Vec<PathBuf> {
    for o in options {
        if let Some((k, f)) = o.split_once('=')
            && k.trim().eq_ignore_ascii_case("UserKnownHostsFile")
        {
            return f.split_whitespace().map(PathBuf::from).collect();
        }
    }
    if let Some(text) = config.and_then(|c| std::fs::read_to_string(c).ok()) {
        for l in text.lines() {
            let l = l.trim();
            let (k, v) = l
                .split_once(|c: char| c == '=' || c.is_whitespace())
                .unwrap_or((l, ""));
            if k.eq_ignore_ascii_case("UserKnownHostsFile") {
                return v
                    .trim_start_matches(|c: char| c == '=' || c.is_whitespace())
                    .split_whitespace()
                    .map(|f| PathBuf::from(f.trim_matches('"')))
                    .collect();
            }
        }
    }
    std::env::var_os("USERPROFILE")
        .map(|h| vec![PathBuf::from(h).join(".ssh").join("known_hosts")])
        .unwrap_or_default()
}

/// Whether `target` is already in a `known_hosts` file (asked of `ssh-keygen -F`, which
/// understands hashed entries; nothing is changed).
pub fn verified(target: &Target, files: &[PathBuf]) -> bool {
    let Some(keygen) = openssh("ssh-keygen") else {
        return false;
    };
    files.iter().filter(|f| f.is_file()).any(|f| {
        #[cfg(windows)]
        let flags = {
            use std::os::windows::process::CommandExt;
            let mut c = std::process::Command::new(&keygen);
            c.creation_flags(0x0800_0000);
            c
        };
        #[cfg(not(windows))]
        let flags = std::process::Command::new(&keygen);
        let mut c = flags;
        c.args(["-F", &target.known_name(), "-f"])
            .arg(f)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
    })
}

/// Step 35: which way a port forward goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FwdKind {
    /// `L`: a local port reaches `host:port` from the server.
    Local,
    /// `R`: a port on the server reaches `host:port` from this PC.
    Remote,
    /// `D`: a local SOCKS proxy through the server.
    Dynamic,
}

/// Step 35: `--forward L:[bind:]port:host:hostport`, `R:…` or `D:[bind:]port`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward {
    pub kind: FwdKind,
    pub bind: Option<String>,
    pub port: u16,
    pub to: Option<(String, u16)>,
}

fn fwd_host(h: &str) -> Result<String, String> {
    if h == "*"
        || (!h.is_empty()
            && h.len() <= 253
            && h.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c)))
    {
        Ok(h.to_string())
    } else {
        Err(format!("`{h}` is not a host name or IPv4 address"))
    }
}

fn fwd_port(p: &str) -> Result<u16, String> {
    p.parse::<u16>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("`{p}` is not a port"))
}

/// Whether a bind address stays on this PC (or, for `R`, on the server).
pub fn loopback(b: &str) -> bool {
    b == "localhost" || b.starts_with("127.") || b == "::1"
}

impl Forward {
    pub fn parse(s: &str) -> Result<Self, String> {
        let bad = || {
            format!(
                "`{s}` is not L:[bind:]port:host:port, R:[bind:]port:host:port or D:[bind:]port"
            )
        };
        let (k, rest) = s.split_once(':').ok_or_else(bad)?;
        let kind = match k.to_ascii_uppercase().as_str() {
            "L" => FwdKind::Local,
            "R" => FwdKind::Remote,
            "D" => FwdKind::Dynamic,
            _ => return Err(bad()),
        };
        let p: Vec<&str> = rest.split(':').collect();
        let (bind, port, to) = match (kind, p.len()) {
            (FwdKind::Dynamic, 1) => (None, fwd_port(p[0])?, None),
            (FwdKind::Dynamic, 2) => (Some(fwd_host(p[0])?), fwd_port(p[1])?, None),
            (FwdKind::Local | FwdKind::Remote, 3) => (
                None,
                fwd_port(p[0])?,
                Some((fwd_host(p[1])?, fwd_port(p[2])?)),
            ),
            (FwdKind::Local | FwdKind::Remote, 4) => (
                Some(fwd_host(p[0])?),
                fwd_port(p[1])?,
                Some((fwd_host(p[2])?, fwd_port(p[3])?)),
            ),
            _ => return Err(bad()),
        };
        if to.as_ref().is_some_and(|(h, _)| h == "*") {
            return Err(bad());
        }
        Ok(Self {
            kind,
            bind,
            port,
            to,
        })
    }

    /// The address it listens on: an explicit bind, else loopback (`L`, `D`) or the server's
    /// default, which is loopback unless its `GatewayPorts` says otherwise (`R`).
    pub fn listen(&self) -> Option<String> {
        self.bind
            .clone()
            .or_else(|| (self.kind != FwdKind::Remote).then(|| "127.0.0.1".to_string()))
    }

    /// Whether it listens beyond loopback (needs `--allow-public-bind`).
    pub fn public(&self) -> bool {
        self.bind.as_deref().is_some_and(|b| !loopback(b))
    }

    /// ssh's flag and value.
    pub fn arg(&self) -> (&'static str, String) {
        let mut v = self.listen().map(|b| format!("{b}:")).unwrap_or_default();
        v.push_str(&self.port.to_string());
        if let Some((h, p)) = &self.to {
            v.push_str(&format!(":{h}:{p}"));
        }
        let f = match self.kind {
            FwdKind::Local => "-L",
            FwdKind::Remote => "-R",
            FwdKind::Dynamic => "-D",
        };
        (f, v)
    }

    pub fn json(&self) -> Value {
        json!({
            "kind": match self.kind { FwdKind::Local => "local", FwdKind::Remote => "remote", FwdKind::Dynamic => "dynamic" },
            "listen": self.listen(),
            "port": self.port,
            "to": self.to.as_ref().map(|(h, p)| format!("{h}:{p}")),
            "public": self.public(),
        })
    }
}

/// Step 35: `--proxy socks5://host:port` or `http://host:port` (a CONNECT proxy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proxy {
    pub socks: bool,
    pub host: String,
    pub port: u16,
}

impl Proxy {
    pub fn parse(s: &str) -> Result<Self, String> {
        let (scheme, rest) = s
            .split_once("://")
            .ok_or_else(|| format!("`{s}` is not socks5://host:port or http://host:port"))?;
        let socks = match scheme.to_ascii_lowercase().as_str() {
            "socks5" | "socks5h" => true,
            "http" => false,
            _ => {
                return Err(format!(
                    "`{scheme}` proxies are not supported (socks5 or http)"
                ));
            }
        };
        if rest.contains('@') {
            return Err("proxy user names and passwords are not supported".into());
        }
        let rest = rest.trim_end_matches('/');
        let (h, p) = rest
            .rsplit_once(':')
            .ok_or_else(|| format!("`{s}` needs a port"))?;
        Ok(Self {
            socks,
            host: fwd_host(h)?,
            port: fwd_port(p)?,
        })
    }

    pub fn shown(&self) -> String {
        format!(
            "{}://{}:{}",
            if self.socks { "socks5" } else { "http" },
            self.host,
            self.port
        )
    }
}

/// Step 35: everything an SSH connection is made of besides its destination.
#[derive(Debug, Clone, Default)]
pub struct Conn {
    pub identity: Option<String>,
    /// `-F <file>`: an ssh config file used instead of `~/.ssh/config` (jump hosts read it too).
    pub config: Option<String>,
    pub options: Vec<String>,
    pub jumps: Vec<Target>,
    pub forwards: Vec<Forward>,
    pub agent_forward: bool,
    pub x11: bool,
    /// `-N`: forwards only, no remote command.
    pub no_shell: bool,
    pub proxy: Option<Proxy>,
}

impl Conn {
    /// The connection the options of `a` describe; `pane` allows the interactive-only parts
    /// (forwards, agent and X11 forwarding).
    pub fn from_args(a: &Args, pane: bool) -> Result<Self, CliError> {
        let mut c = Self {
            identity: a.identity.clone(),
            config: a.ssh_config.clone(),
            options: a.ssh_options.clone(),
            agent_forward: a.agent_forward,
            x11: a.x11,
            no_shell: pane && a.no_shell,
            ..Self::default()
        };
        for j in &a.jumps {
            c.jumps.push(Target::parse(j).map_err(|m| {
                CliError::usage(format!("--jump: {m}"))
                    .hint("give each jump host as [user@]host[:port]")
            })?);
        }
        for f in &a.forwards {
            c.forwards
                .push(Forward::parse(f).map_err(|m| CliError::usage(format!("--forward: {m}")))?);
        }
        if let Some(p) = &a.proxy {
            c.proxy = Some(Proxy::parse(p).map_err(|m| CliError::usage(format!("--proxy: {m}")))?);
        }
        c.check(a.allow_public_bind)?;
        Ok(c)
    }

    /// Adds an SSH profile's parts (Step 35 fields) to what the command line gave.
    pub fn with_profile(mut self, p: &crate::config::Profile) -> Result<Self, CliError> {
        if self.identity.is_none() {
            self.identity.clone_from(&p.identity);
        }
        for j in &p.jump {
            self.jumps.push(Target::parse(j).map_err(CliError::usage)?);
        }
        for f in &p.forwards {
            self.forwards
                .push(Forward::parse(f).map_err(CliError::usage)?);
        }
        self.agent_forward |= p.agent_forward;
        self.check(false)?;
        Ok(self)
    }

    fn check(&self, allow_public: bool) -> Result<(), CliError> {
        if self.proxy.is_some() && !self.jumps.is_empty() {
            return Err(CliError::usage(
                "give --proxy or --jump, not both (a jump host reaches the next hop itself)",
            ));
        }
        if self.jumps.len() > 8 {
            return Err(CliError::usage("at most 8 jump hosts"));
        }
        if self.forwards.len() > 32 {
            return Err(CliError::usage("at most 32 forwards"));
        }
        if let Some(f) = self.forwards.iter().find(|f| f.public())
            && !allow_public
        {
            return Err(CliError::usage(format!(
                "--forward {}: listening on `{}` lets other computers use the forward",
                f.arg().1,
                f.bind.as_deref().unwrap_or("")
            ))
            .hint("bind 127.0.0.1 (the default), or pass --allow-public-bind if that is what you want"));
        }
        if let Some(c) = &self.config
            && !std::path::Path::new(c).is_file()
        {
            return Err(CliError::new(
                Code::Io,
                format!("the ssh config file `{c}` does not exist"),
            ));
        }
        if let Some(i) = &self.identity
            && !std::path::Path::new(i).is_file()
        {
            return Err(CliError::new(
                Code::Io,
                format!("the key file `{i}` does not exist"),
            ));
        }
        Ok(())
    }

    /// Local ports of `L` and `D` forwards that something else already listens on.
    pub fn busy_ports(&self) -> Option<&Forward> {
        self.forwards.iter().find(|f| {
            f.kind != FwdKind::Remote && {
                let b = f.listen().unwrap_or_default();
                let b = if b == "*" { "0.0.0.0".to_string() } else { b };
                std::net::TcpListener::bind((b.as_str(), f.port)).is_err()
            }
        })
    }

    /// Warnings the user must see (agent and X11 forwarding, public forwards).
    pub fn warnings(&self, target: &Target) -> Vec<String> {
        let mut w = Vec::new();
        if self.agent_forward {
            w.push(format!(
                "agent forwarding is on: while connected, anyone with administrator rights on {} can use your loaded SSH keys to log in elsewhere as you",
                target.host
            ));
        }
        if self.x11 {
            w.push(format!(
                "X11 forwarding is on (untrusted): programs on {} can open windows on your X server",
                target.host
            ));
        }
        for f in self.forwards.iter().filter(|f| f.public()) {
            w.push(format!(
                "forward {} listens beyond loopback: other computers can use it",
                f.arg().1
            ));
        }
        w
    }

    fn files(&self) -> Vec<PathBuf> {
        known_hosts_files(&self.options, self.config.as_deref())
    }
}

/// What `subak ssh connect` (or an SSH profile) starts.
#[derive(Debug, Clone)]
pub struct Plan {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Environment for the pane (names and values; no secret: the askpass helper reads the
    /// secret itself from Credential Manager).
    pub env: Vec<(String, String)>,
    pub askpass: bool,
    pub why: String,
}

/// The ssh arguments both kinds of connection share, up to (not including) the host.
fn common(target: &Target, c: &Conn, strict: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-o".into(),
        format!("StrictHostKeyChecking={strict}"),
        "-o".into(),
        format!(
            "ForwardAgent={}",
            if c.agent_forward { "yes" } else { "no" }
        ),
        "-p".into(),
        target.port.to_string(),
    ];
    if let Some(u) = &target.user {
        args.extend(["-l".into(), u.clone()]);
    }
    if let Some(i) = &c.identity {
        args.extend(["-i".into(), i.clone()]);
    }
    if let Some(f) = &c.config {
        args.extend(["-F".into(), f.clone()]);
    }
    if !c.jumps.is_empty() {
        let j: Vec<String> = c.jumps.iter().map(Target::shown).collect();
        args.extend(["-J".into(), j.join(",")]);
    }
    if let Some(p) = &c.proxy
        && let Ok(me) = std::env::current_exe()
    {
        args.extend([
            "-o".into(),
            format!(
                "ProxyCommand=\"{}\" __proxy-relay {} {}:{} %h %p",
                me.display(),
                if p.socks { "socks5" } else { "http" },
                p.host,
                p.port
            ),
        ]);
    }
    for o in &c.options {
        args.extend(["-o".into(), o.clone()]);
    }
    args
}

/// The Credential Manager entries a connection may use: the destination's and each jump
/// host's, each only when saved and its host is in known_hosts.
fn usable_secrets(target: &Target, c: &Conn, has: &dyn Fn(&Target) -> bool) -> (Vec<String>, bool) {
    let files = c.files();
    let dest_ok = verified(target, &files);
    let mut creds = Vec::new();
    if dest_ok && has(target) {
        creds.push(target.credential());
    }
    for j in &c.jumps {
        if has(j) && verified(j, &files) {
            creds.push(j.credential());
        }
    }
    (creds, dest_ok)
}

fn askpass_env(creds: &[String], env: &mut Vec<(String, String)>) {
    if let Ok(me) = std::env::current_exe() {
        env.push(("SSH_ASKPASS".into(), me.display().to_string()));
        env.push(("SSH_ASKPASS_REQUIRE".into(), "force".into()));
        env.push(("SUBAK_ASKPASS".into(), "1".into()));
        env.push(("SUBAK_SSH_CRED".into(), creds.join("|")));
    }
}

/// The command for `target`: strict host keys, no agent forwarding unless asked, the user's
/// own options; saved secrets through the askpass helper only for verified hosts that have
/// one (`creds`).
pub fn plan(target: &Target, c: &Conn, creds: &[String], verified: bool) -> Option<Plan> {
    let program = openssh("ssh")?;
    let mut args = common(target, c, "ask");
    if c.x11 {
        args.extend([
            "-o".into(),
            "ForwardX11=yes".into(),
            "-o".into(),
            "ForwardX11Trusted=no".into(),
        ]);
    }
    if !c.forwards.is_empty() {
        args.extend(["-o".into(), "ExitOnForwardFailure=yes".into()]);
        for f in &c.forwards {
            let (flag, v) = f.arg();
            args.extend([flag.into(), v]);
        }
    }
    if c.no_shell {
        args.push("-N".into());
    }
    args.push(target.host.clone());
    let askpass = !creds.is_empty();
    let why = match (askpass, verified) {
        (true, true) => "the saved secret answers the password or passphrase prompt once",
        (true, false) => {
            "a jump host's saved secret answers its prompt once; ssh asks you the rest"
        }
        (false, true) => "no saved secret: ssh asks you",
        (false, false) => "the host is not in known_hosts: ssh shows its fingerprint and asks you",
    }
    .to_string();
    let mut env = Vec::new();
    if askpass {
        askpass_env(creds, &mut env);
    }
    Some(Plan {
        program,
        args,
        env,
        askpass,
        why,
    })
}

/// The program, arguments and environment of a non-interactive ssh.
pub type Batch = (PathBuf, Vec<String>, Vec<(String, String)>);

/// Step 35: a non-interactive connection running `subsystem` (SFTP): the host must already be
/// in known_hosts (`StrictHostKeyChecking=yes`), nothing is forwarded, and ssh never prompts
/// (the askpass helper answers with a saved secret or refuses).
pub fn batch(target: &Target, c: &Conn, subsystem: &str) -> Option<Batch> {
    let program = openssh("ssh")?;
    let mut args = common(target, c, "yes");
    let has_opt = |k: &str| {
        c.options.iter().any(|o| {
            o.to_ascii_lowercase()
                .starts_with(&format!("{}=", k.to_ascii_lowercase()))
        })
    };
    for (k, v) in [
        ("ConnectTimeout", "20"),
        ("ServerAliveInterval", "10"),
        ("ServerAliveCountMax", "3"),
    ] {
        if !has_opt(k) {
            args.extend(["-o".into(), format!("{k}={v}")]);
        }
    }
    args.extend([
        "-o".into(),
        "ClearAllForwardings=yes".into(),
        "-o".into(),
        "ForwardX11=no".into(),
        "-o".into(),
        "BatchMode=no".into(),
        "-s".into(),
        target.host.clone(),
        subsystem.into(),
    ]);
    let (creds, _) = usable_secrets(target, c, &has_secret);
    let mut env = Vec::new();
    askpass_env(&creds, &mut env);
    Some((program, args, env))
}

/// The prompts the askpass helper answers: a password or a key passphrase — never a yes/no or
/// fingerprint question.
pub fn answers(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    let secret = p.contains("password") || p.contains("passphrase");
    let question = p.contains("yes/no")
        || p.contains("fingerprint")
        || p.contains("continue connecting")
        || p.contains("(y/n)");
    secret && !question
}

/// Step 35: which saved secret answers `prompt` (`creds`: the destination's first, then the
/// jump hosts'). A password goes to a server, so only the entry whose user and host the prompt
/// names answers it (`me@host's password:`, `(me@host) Password:`); a key passphrase never
/// leaves this PC, so the destination's entry answers it.
pub fn pick<'a>(prompt: &str, creds: &[&'a str]) -> Option<&'a str> {
    if !answers(prompt) {
        return None;
    }
    if prompt.to_ascii_lowercase().contains("passphrase") {
        return creds.first().copied();
    }
    creds.iter().copied().find(|c| {
        let Some(rest) = c.strip_prefix("mouv-subak:ssh:") else {
            return false;
        };
        let Some((uh, _port)) = rest.rsplit_once(':') else {
            return false;
        };
        let (user, host) = uh.rsplit_once('@').unwrap_or(("", uh));
        let who = format!("{user}@{host}");
        let named = |w: &str| {
            prompt.contains(&format!("{w}'s password")) || prompt.contains(&format!("({w})"))
        };
        if user.is_empty() {
            // No user saved: the prompt names ssh's default user; the host must match.
            prompt.contains(&format!("@{host}'s password")) || prompt.contains(&format!("@{host})"))
        } else {
            named(&who)
        }
    })
}

/// `SUBAK_ASKPASS=1`: `subak.exe <prompt>` called by ssh. Prints the saved secret for an
/// answerable prompt, once per connection and entry; exits 1 otherwise (ssh then fails or asks
/// nothing).
pub fn askpass_main(prompt: &str) -> std::process::ExitCode {
    use std::io::Write;
    let Ok(list) = std::env::var("SUBAK_SSH_CRED") else {
        return std::process::ExitCode::from(1);
    };
    let creds: Vec<&str> = list
        .split('|')
        .filter(|c| c.starts_with("mouv-subak:ssh:"))
        .collect();
    let Some(cred) = pick(prompt, &creds) else {
        return std::process::ExitCode::from(1);
    };
    #[cfg(windows)]
    let secret = {
        // Once per ssh process (its id and start time): a second prompt means the secret was
        // wrong or expired, and ssh then asks the user or fails instead of retrying it.
        let Some((pid, started)) = crate::win::parent_process() else {
            return std::process::ExitCode::from(1);
        };
        let dir = std::env::temp_dir().join("subak-askpass");
        let _ = std::fs::create_dir_all(&dir);
        // Markers of ssh processes older than a day are done with.
        let day = std::time::Duration::from_secs(86_400);
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if e.metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t.elapsed().is_ok_and(|a| a > day))
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let tag = cred.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        });
        let marker = dir.join(format!("{pid}-{started}-{tag:x}"));
        if std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .is_err()
        {
            return std::process::ExitCode::from(1);
        }
        crate::win::creds::read(cred)
    };
    #[cfg(not(windows))]
    let secret: Option<String> = None;
    let Some(secret) = secret else {
        return std::process::ExitCode::from(1);
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{secret}");
    std::process::ExitCode::SUCCESS
}

/// The plan as JSON (no secret, only whether one is used).
pub fn plan_json(target: &Target, p: &Plan) -> Value {
    json!({
        "target": target.shown(),
        "program": p.program.display().to_string(),
        "args": p.args,
        "saved_secret_used": p.askpass,
        "why": p.why,
        "env": p.env.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
    })
}

/// `--option Key=Value`: an OpenSSH option name and a value without line breaks. Options that
/// would run local commands or weaken host-key checking are refused (use `ssh` directly).
pub fn option_ok(o: &str) -> bool {
    let Some((k, v)) = o.split_once('=') else {
        return false;
    };
    let k = k.trim().to_ascii_lowercase();
    let refused = [
        "proxycommand",
        "localcommand",
        "permitlocalcommand",
        "knownhostscommand",
        "stricthostkeychecking",
        "forwardagent",
        "match",
        "include",
        // Step 35: these have their own checked flags (--jump, --forward, --x11).
        "proxyjump",
        "localforward",
        "remoteforward",
        "dynamicforward",
        "gatewayports",
        "forwardx11",
        "forwardx11trusted",
        "controlmaster",
        "controlpath",
    ];
    !k.is_empty()
        && k.chars().all(|c| c.is_ascii_alphanumeric())
        && !refused.contains(&k.as_str())
        && !v.contains(['\r', '\n', '\0'])
}

pub fn target_of(a: &Args) -> Result<Target, CliError> {
    let t = a.positional.first().map_or("", String::as_str);
    Target::parse(t)
        .map_err(|m| CliError::usage(m).hint("give the destination as [user@]host[:port]"))
}

pub fn has_secret(t: &Target) -> bool {
    #[cfg(windows)]
    return crate::win::creds::read(&t.credential()).is_some();
    #[cfg(not(windows))]
    return false;
}

/// `a` turned into the `new` of an SSH pane for `target` (profile or `ssh connect`).
pub fn apply(a: &Args, target: &Target, c: &Conn) -> Result<(Args, Value), CliError> {
    // Step 35: a taken local port fails here, before anything starts.
    if let Some(f) = c.busy_ports() {
        return Err(CliError::new(
            Code::PortInUse,
            format!(
                "port {} on {} is already in use; nothing was started",
                f.port,
                f.listen().unwrap_or_default()
            ),
        )
        .hint("pick another local port for --forward"));
    }
    if c.x11
        && std::env::var_os("DISPLAY").is_none()
        && !a
            .env
            .iter()
            .any(|e| matches!(e, EnvArg::Set(k, _) if k.eq_ignore_ascii_case("DISPLAY")))
    {
        return Err(CliError::new(
            Code::InvalidState,
            "X11 forwarding needs a running X server and DISPLAY",
        )
        .hint("start the X server you installed (for example VcXsrv) and pass --env DISPLAY=127.0.0.1:0.0; Subak never installs one"));
    }
    let (creds, verified) = usable_secrets(target, c, &has_secret);
    let p = plan(target, c, &creds, verified).ok_or_else(|| {
        CliError::new(Code::SpawnFailed, "Windows' OpenSSH client (System32\\OpenSSH\\ssh.exe) is not installed")
            .hint("add the optional feature \"OpenSSH Client\" in Windows Settings; Subak never installs it")
    })?;
    let mut eff = a.clone();
    eff.command = std::iter::once(p.program.display().to_string())
        .chain(p.args.iter().cloned())
        .collect();
    eff.command_line = None;
    eff.no_profile = true;
    eff.profile = None;
    // A failed or ended connection stays visible (its message), never a vanished tab.
    eff.remain_on_exit = true;
    let mut env: Vec<EnvArg> = p
        .env
        .iter()
        .map(|(k, v)| EnvArg::Set(k.clone(), v.clone()))
        .collect();
    env.extend(a.env.iter().cloned());
    eff.env = env;
    let mut info = plan_json(target, &p);
    info["host_verified"] = json!(verified);
    info["jumps"] = json!(c.jumps.iter().map(Target::shown).collect::<Vec<_>>());
    info["forwards"] = json!(c.forwards.iter().map(Forward::json).collect::<Vec<_>>());
    info["agent_forward"] = json!(c.agent_forward);
    info["x11"] = json!(c.x11);
    info["proxy"] = json!(c.proxy.as_ref().map(Proxy::shown));
    let warnings = c.warnings(target);
    for w in &warnings {
        crate::warn_later(w.clone());
    }
    info["warnings"] = json!(warnings);
    Ok((eff, info))
}

/// `subak ssh connect <[user@]host[:port]> [<name>] [--identity <key>] [--option K=V]…
/// [--jump <host>]… [--forward <spec>]… [--agent-forward] [--x11] [--proxy <url>]
/// [--login-script <file>]`: a Terminal whose pane runs ssh (tabs, panes, detach and the Host
/// lifecycle as any other).
pub fn connect(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let target = target_of(a)?;
    let conn = Conn::from_args(a, true)?;
    let script = match &a.login_script {
        Some(f) => Some(LoginScript::load(std::path::Path::new(f))?),
        None => None,
    };
    if script.is_some() && a.attach {
        return Err(CliError::usage(
            "--login-script runs before you attach: connect without --attach, then `subak attach`",
        ));
    }
    let (mut eff, info) = apply(a, &target, &conn)?;
    eff.positional = a.positional.get(1).cloned().into_iter().collect();
    let mut out = crate::terminal::new(&eff, endpoint)?;
    out["ssh"] = info;
    if let Some(s) = script
        && !a.dry_run
    {
        out["login_script"] = s.run(endpoint, &out["terminal_id"])?;
    }
    Ok(out)
}

/// Step 35: one expect/send step of a login script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub expect: String,
    pub send: String,
    pub enter: bool,
    pub timeout_s: u64,
}

/// Step 35: a login script (`[[step]] expect = "…" send = "…"`): text typed into the SSH pane
/// after ssh shows the expected text. Auditable: the result lists every step, what matched and
/// what was sent. A script never holds a secret (secret-looking text is refused); a saved
/// password answers ssh's own prompt through the askpass helper instead.
#[derive(Debug, Clone)]
pub struct LoginScript {
    pub path: String,
    pub steps: Vec<Step>,
}

impl LoginScript {
    pub fn load(path: &std::path::Path) -> Result<Self, CliError> {
        let bad = |m: String| {
            CliError::new(
                Code::ConfigInvalid,
                format!("login script `{}`: {m}", path.display()),
            )
        };
        let meta = std::fs::metadata(path).map_err(|e| {
            CliError::new(Code::Io, format!("cannot read `{}`: {e}", path.display()))
        })?;
        if meta.len() > 64 * 1024 {
            return Err(bad("larger than 64 KiB".into()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| {
            CliError::new(Code::Io, format!("cannot read `{}`: {e}", path.display()))
        })?;
        Self::parse(&text, &path.display().to_string()).map_err(bad)
    }

    pub fn parse(text: &str, path: &str) -> Result<Self, String> {
        let doc: toml::Table = toml::from_str(text).map_err(|e| e.message().to_string())?;
        if let Some(k) = doc.keys().find(|k| *k != "step") {
            return Err(format!("`{k}` is not a login script field (step)"));
        }
        let steps = doc
            .get("step")
            .and_then(toml::Value::as_array)
            .ok_or("needs [[step]] entries")?;
        if steps.is_empty() || steps.len() > 32 {
            return Err("needs 1 to 32 [[step]] entries".into());
        }
        let mut out = Vec::new();
        for (i, st) in steps.iter().enumerate() {
            let n = i + 1;
            let t = st.as_table().ok_or(format!("step {n} must be a table"))?;
            if let Some(k) = t
                .keys()
                .find(|k| !["expect", "send", "enter", "timeout"].contains(&k.as_str()))
            {
                return Err(format!(
                    "step {n}: `{k}` is not a step field (expect, send, enter, timeout)"
                ));
            }
            let s = |k: &str| t.get(k).and_then(toml::Value::as_str).map(String::from);
            let expect = s("expect")
                .filter(|e| !e.is_empty() && e.len() <= 256)
                .ok_or(format!(
                    "step {n}: `expect` must be 1 to 256 characters of text to wait for"
                ))?;
            let send = s("send").ok_or(format!("step {n}: `send` is required"))?;
            if send.len() > 1024 || send.chars().any(char::is_control) {
                return Err(format!(
                    "step {n}: `send` must be at most 1024 characters without control characters"
                ));
            }
            if crate::redact::text(&send) != send {
                return Err(format!(
                    "step {n}: `send` looks like a secret; login scripts never hold secrets (save a password with `subak ssh credential save`)"
                ));
            }
            let enter = match t.get("enter") {
                None => true,
                Some(v) => v
                    .as_bool()
                    .ok_or(format!("step {n}: `enter` must be true or false"))?,
            };
            let timeout_s = match t.get("timeout") {
                None => 15,
                Some(v) => v
                    .as_integer()
                    .and_then(|x| u64::try_from(x).ok())
                    .filter(|x| (1..=300).contains(x))
                    .ok_or(format!("step {n}: `timeout` must be 1 to 300 seconds"))?,
            };
            out.push(Step {
                expect,
                send,
                enter,
                timeout_s,
            });
        }
        Ok(Self {
            path: path.to_string(),
            steps: out,
        })
    }

    /// Runs the steps against Terminal `terminal`: waits for each `expect` (a new occurrence on
    /// the screen or in its history), then types `send`. Stops at the first step that times
    /// out; the Terminal stays as it is.
    pub fn run(&self, endpoint: &str, terminal: &Value) -> Result<Value, CliError> {
        let mut s = crate::terminal::require(endpoint)?;
        let screen = |s: &mut crate::client::Session| -> Result<String, CliError> {
            let r = s.request(
                "pane.capture",
                &json!({"target": terminal, "start": "-", "end": Value::Null, "format": "text", "join": true, "redact": false}),
            )?;
            Ok(r["lines"]
                .as_array()
                .map(|l| {
                    l.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default())
        };
        let mut seen = 0usize;
        let mut done = Vec::new();
        let mut completed = true;
        for (i, st) in self.steps.iter().enumerate() {
            let t0 = std::time::Instant::now();
            let deadline = t0 + std::time::Duration::from_secs(st.timeout_s);
            let matched = loop {
                if screen(&mut s)?.matches(&st.expect).count() > seen {
                    break true;
                }
                if std::time::Instant::now() >= deadline {
                    break false;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            };
            let waited = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
            if !matched {
                done.push(json!({"expect": st.expect, "matched": false, "waited_ms": waited}));
                completed = false;
                crate::warn_later(format!(
                    "login script step {}: `{}` did not appear within {} s; the remaining steps were not run",
                    i + 1,
                    st.expect,
                    st.timeout_s
                ));
                break;
            }
            s.request(
                "pane.send_keys",
                &json!({"target": terminal, "keys": [st.send], "literal": true}),
            )?;
            if st.enter {
                s.request(
                    "pane.send_keys",
                    &json!({"target": terminal, "keys": ["Enter"]}),
                )?;
            }
            done.push(json!({"expect": st.expect, "matched": true, "sent": st.send, "enter": st.enter, "waited_ms": waited}));
            if let Some(next) = self.steps.get(i + 1) {
                seen = screen(&mut s)?.matches(&next.expect).count();
            }
        }
        Ok(json!({"path": self.path, "completed": completed, "steps": done}))
    }
}

/// Step 35: `subak.exe __proxy-relay socks5|http <proxy host:port> <host> <port>`, the
/// `ProxyCommand` Subak itself sets for `--proxy`: connects through the proxy and relays ssh's
/// stdin and stdout. No credentials; nothing else is run.
pub fn proxy_relay(a: &[String]) -> std::process::ExitCode {
    use std::io::{Read, Write};
    let fail = |m: &str| {
        eprintln!("subak proxy: {m}");
        std::process::ExitCode::from(1)
    };
    let [kind, proxy, host, port] = a else {
        return fail("usage: __proxy-relay socks5|http <proxy host:port> <host> <port>");
    };
    let Ok(port) = port.parse::<u16>() else {
        return fail("bad port");
    };
    if host.len() > 255 {
        return fail("host name too long");
    }
    let Ok(mut s) = std::net::TcpStream::connect(proxy.as_str()) else {
        return fail(&format!("cannot reach the proxy {proxy}"));
    };
    let _ = s.set_nodelay(true);
    if kind == "socks5" {
        let mut r = [0u8; 2];
        if s.write_all(&[5, 1, 0]).is_err() || s.read_exact(&mut r).is_err() || r != [5, 0] {
            return fail("the SOCKS proxy wants authentication or is not SOCKS5");
        }
        let mut req = vec![5, 1, 0, 3, u8::try_from(host.len()).unwrap_or(255)];
        req.extend_from_slice(host.as_bytes());
        req.extend_from_slice(&port.to_be_bytes());
        let mut h = [0u8; 4];
        if s.write_all(&req).is_err() || s.read_exact(&mut h).is_err() {
            return fail("the SOCKS proxy closed the connection");
        }
        if h[1] != 0 {
            return fail(&format!(
                "the SOCKS proxy refused the connection (reply {})",
                h[1]
            ));
        }
        let skip = match h[3] {
            1 => 4,
            4 => 16,
            3 => {
                let mut l = [0u8; 1];
                if s.read_exact(&mut l).is_err() {
                    return fail("a short SOCKS reply");
                }
                usize::from(l[0])
            }
            _ => return fail("a bad SOCKS reply"),
        };
        let mut rest = vec![0u8; skip + 2];
        if s.read_exact(&mut rest).is_err() {
            return fail("a short SOCKS reply");
        }
    } else {
        let req = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n");
        if s.write_all(req.as_bytes()).is_err() {
            return fail("the HTTP proxy closed the connection");
        }
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() > 8192 || s.read_exact(&mut b).is_err() {
                return fail("a bad HTTP proxy reply");
            }
            head.push(b[0]);
        }
        let line = String::from_utf8_lossy(&head);
        if line.split_whitespace().nth(1) != Some("200") {
            return fail(&format!(
                "the HTTP proxy refused: {}",
                line.lines().next().unwrap_or("")
            ));
        }
    }
    let Ok(mut up) = s.try_clone() else {
        return fail("socket");
    };
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut up);
        let _ = up.shutdown(std::net::Shutdown::Write);
    });
    let mut out = std::io::stdout().lock();
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if out.write_all(&buf[..n]).and_then(|()| out.flush()).is_err() {
                    break;
                }
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

/// `subak ssh credential status|save|forget <[user@]host[:port]>`: the saved password or
/// passphrase in Credential Manager. Saving needs a host already in known_hosts (connected
/// and confirmed once); the secret comes from a no-echo console prompt or one stdin line and is
/// never printed.
pub fn credential(cmd: Cmd, a: &Args) -> Result<(Value, String), CliError> {
    let t = target_of(a)?;
    let verified = verified(
        &t,
        &known_hosts_files(&a.ssh_options, a.ssh_config.as_deref()),
    );
    let saved = has_secret(&t);
    let base = json!({"target": t.shown(), "credential": t.credential(), "host_verified": verified, "saved": saved});
    match cmd {
        Cmd::SshCredentialStatus => {
            let h = format!(
                "{}: {} secret saved; host {} known_hosts\n",
                t.shown(),
                if saved { "a" } else { "no" },
                if verified { "in" } else { "not in" }
            );
            Ok((base, h))
        }
        Cmd::SshCredentialSave => {
            if !verified {
                return Err(CliError::new(
                    Code::InvalidState,
                    format!(
                        "{} is not in known_hosts: a secret is saved only for a verified host",
                        t.shown()
                    ),
                )
                .hint(format!(
                    "connect once (`subak ssh connect {}`), check the fingerprint and answer yes",
                    t.shown()
                )));
            }
            let mut d = base;
            d["dry_run"] = json!(a.dry_run);
            if a.dry_run {
                return Ok((d, format!("would save a secret for {}\n", t.shown())));
            }
            #[cfg(windows)]
            let secret = match crate::win::console::read_secret_line(&format!(
                "password or passphrase for {}: ",
                t.shown()
            )) {
                Some(s) => s,
                None => {
                    let mut l = String::new();
                    std::io::stdin().read_line(&mut l).map_err(|e| {
                        CliError::new(Code::Io, format!("cannot read the secret: {e}"))
                    })?;
                    l.trim_end_matches(['\r', '\n']).to_string()
                }
            };
            #[cfg(not(windows))]
            let secret = String::new();
            if secret.is_empty() {
                return Err(CliError::new(
                    Code::Cancelled,
                    "no secret given; nothing was saved",
                ));
            }
            #[cfg(windows)]
            crate::win::creds::write(&t.credential(), t.user.as_deref().unwrap_or(""), &secret)
                .map_err(|e| {
                    CliError::new(
                        Code::Io,
                        format!("Credential Manager refused the secret: {e}"),
                    )
                })?;
            d["saved"] = json!(true);
            Ok((
                d,
                format!(
                    "saved in Windows Credential Manager as {}\n",
                    t.credential()
                ),
            ))
        }
        _ => {
            let mut d = base;
            d["dry_run"] = json!(a.dry_run);
            if a.dry_run {
                return Ok((d, format!("would forget the secret of {}\n", t.shown())));
            }
            #[cfg(windows)]
            let gone = crate::win::creds::delete(&t.credential());
            #[cfg(not(windows))]
            let gone = false;
            d["saved"] = json!(false);
            d["removed"] = json!(gone);
            Ok((
                d,
                format!(
                    "{}: {}\n",
                    t.shown(),
                    if gone {
                        "secret forgotten"
                    } else {
                        "no secret was saved"
                    }
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_parse_strictly() {
        let t = Target::parse("me@example.com:2222").unwrap();
        assert_eq!(
            (t.user.as_deref(), t.host.as_str(), t.port),
            (Some("me"), "example.com", 2222)
        );
        assert_eq!(t.credential(), "mouv-subak:ssh:me@example.com:2222");
        assert_eq!(t.known_name(), "[example.com]:2222");
        assert_eq!(Target::parse("host").unwrap().known_name(), "host");
        for bad in [
            "",
            "a b",
            "me@",
            "@h",
            "h:0",
            "h:x",
            "h;rm",
            "h`x`",
            "me@h -oProxyCommand=x",
        ] {
            assert!(Target::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_helper_answers_secrets_only_never_host_keys() {
        assert!(answers("me@host's password: "));
        assert!(answers(
            "Enter passphrase for key 'C:\\Users\\me/.ssh/id_ed25519': "
        ));
        assert!(!answers(
            "Are you sure you want to continue connecting (yes/no/[fingerprint])? "
        ));
        assert!(!answers("Allow use of key? (y/n)"));
        assert!(!answers("anything else"));
    }

    #[test]
    fn the_plan_is_strict_and_uses_a_secret_only_for_a_verified_host() {
        let t = Target::parse("me@h:2200").unwrap();
        let c = Conn {
            identity: Some("C:/k".into()),
            options: vec!["ServerAliveInterval=5".into()],
            ..Conn::default()
        };
        let Some(p) = plan(&t, &c, &[], false) else {
            return; // no OpenSSH client on this machine
        };
        assert!(
            p.args
                .windows(2)
                .any(|w| w == ["-o", "StrictHostKeyChecking=ask"])
        );
        assert!(p.args.windows(2).any(|w| w == ["-o", "ForwardAgent=no"]));
        assert_eq!(p.args.last().unwrap(), "h");
        assert!(
            !p.askpass && p.env.is_empty(),
            "an unverified host gets no saved secret"
        );
        // A saved secret is offered only for a host in known_hosts.
        let dir = std::env::temp_dir().join(format!("subak-ssh-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let kh = dir.join("known_hosts");
        std::fs::write(
            &kh,
            "[h]:2200 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl\n",
        )
        .unwrap();
        let known = Conn {
            options: vec![format!("UserKnownHostsFile={}", kh.display())],
            ..Conn::default()
        };
        let (creds, ok) = usable_secrets(&t, &known, &|_| true);
        assert!(ok && creds == vec![t.credential()], "{creds:?}");
        let (creds, ok) = usable_secrets(&t, &Conn::default(), &|_| true);
        assert!(!ok && creds.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        let p = plan(&t, &c, &[t.credential()], true).unwrap();
        assert!(p.askpass);
        let names: Vec<&str> = p.env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"SSH_ASKPASS_REQUIRE"));
        assert!(
            !format!("{:?}", p.env).to_lowercase().contains("secret="),
            "no secret in the env"
        );
    }

    #[test]
    fn forwards_bind_loopback_unless_told_otherwise() {
        let f = Forward::parse("L:8080:db.internal:5432").unwrap();
        assert_eq!(f.arg(), ("-L", "127.0.0.1:8080:db.internal:5432".into()));
        assert!(!f.public());
        let f = Forward::parse("d:1080").unwrap();
        assert_eq!(f.arg(), ("-D", "127.0.0.1:1080".into()));
        let f = Forward::parse("R:9000:localhost:3000").unwrap();
        assert_eq!(f.arg(), ("-R", "9000:localhost:3000".into()));
        let f = Forward::parse("L:0.0.0.0:8080:h:80").unwrap();
        assert!(f.public());
        assert!(!Forward::parse("L:127.0.0.1:8080:h:80").unwrap().public());
        for bad in [
            "",
            "X:1:h:2",
            "L:0:h:2",
            "L:8080",
            "D:a:b:c",
            "L:8080:h;rm:2",
            "L:8080:*:2",
            "R:1:h:70000",
        ] {
            assert!(Forward::parse(bad).is_err(), "{bad}");
        }
        let a = Args {
            forwards: vec!["L:0.0.0.0:18080:h:80".into()],
            ..Args::default()
        };
        let e = Conn::from_args(&a, true).unwrap_err();
        assert_eq!(e.code, Code::Usage);
        let a = Args {
            allow_public_bind: true,
            ..a
        };
        let c = Conn::from_args(&a, true).unwrap();
        assert_eq!(c.warnings(&Target::parse("h").unwrap()).len(), 1);
    }

    #[test]
    fn jumps_forwards_and_agent_forwarding_reach_ssh_as_asked() {
        let t = Target::parse("me@dest:2200").unwrap();
        let a = Args {
            jumps: vec!["ops@bastion:2222".into(), "inner".into()],
            forwards: vec!["L:15432:db:5432".into(), "D:11080".into()],
            agent_forward: true,
            no_shell: true,
            ..Args::default()
        };
        let c = Conn::from_args(&a, true).unwrap();
        let Some(p) = plan(&t, &c, &[], false) else {
            return;
        };
        let s = p.args.join(" ");
        assert!(s.contains("-J ops@bastion:2222,inner:22"), "{s}");
        assert!(
            s.contains("-L 127.0.0.1:15432:db:5432") && s.contains("-D 127.0.0.1:11080"),
            "{s}"
        );
        assert!(
            s.contains("ExitOnForwardFailure=yes") && s.contains("ForwardAgent=yes"),
            "{s}"
        );
        assert!(p.args.ends_with(&["-N".into(), "dest".into()]), "{s}");
        assert!(c.warnings(&t)[0].contains("agent forwarding"));
        // Not given: nothing forwarded.
        let p = plan(&t, &Conn::default(), &[], false).unwrap();
        assert!(
            !p.args
                .iter()
                .any(|x| x == "-L" || x == "-J" || x == "-N" || x == "ForwardAgent=yes")
        );
        // SFTP: strict, no forwarding, never a prompt.
        let (_, b, env) = batch(&t, &c, "sftp").unwrap();
        let s = b.join(" ");
        assert!(
            s.contains("StrictHostKeyChecking=yes") && s.contains("ClearAllForwardings=yes"),
            "{s}"
        );
        assert!(s.ends_with("-s dest sftp") && !s.contains("-L "), "{s}");
        assert!(
            env.iter()
                .any(|(k, v)| k == "SSH_ASKPASS_REQUIRE" && v == "force")
        );
        // A proxy is relayed by subak itself; with a jump host it is refused.
        let a = Args {
            proxy: Some("socks5://127.0.0.1:1080".into()),
            ..Args::default()
        };
        let c = Conn::from_args(&a, false).unwrap();
        let s = plan(&t, &c, &[], false).unwrap().args.join(" ");
        assert!(
            s.contains("__proxy-relay socks5 127.0.0.1:1080 %h %p"),
            "{s}"
        );
        let a = Args {
            jumps: vec!["b".into()],
            ..a
        };
        assert!(Conn::from_args(&a, false).is_err());
        for bad in ["ftp://h:1", "socks5://u:p@h:1", "http://h", "socks5://h:0"] {
            assert!(Proxy::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_saved_password_answers_only_the_host_it_was_saved_for() {
        let dest = "mouv-subak:ssh:me@dest:22";
        let jump = "mouv-subak:ssh:ops@bastion:2222";
        let creds = [dest, jump];
        assert_eq!(pick("me@dest's password: ", &creds), Some(dest));
        assert_eq!(pick("ops@bastion's password: ", &creds), Some(jump));
        assert_eq!(pick("(ops@bastion) Password: ", &creds), Some(jump));
        // Another host's prompt never gets a secret: the jump host cannot learn the
        // destination's password.
        assert_eq!(pick("me@evil's password: ", &creds), None);
        assert_eq!(pick("ops@bastion's password: ", &[dest]), None);
        // A passphrase unlocks a local key: the destination's entry.
        assert_eq!(
            pick("Enter passphrase for key 'C:\\k': ", &creds),
            Some(dest)
        );
        assert_eq!(
            pick(
                "Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
                &creds
            ),
            None
        );
        assert_eq!(
            pick("x@h2's password: ", &["mouv-subak:ssh:@h2:22"]),
            Some("mouv-subak:ssh:@h2:22")
        );
    }

    #[test]
    fn login_scripts_are_checked_and_never_hold_secrets() {
        let s = LoginScript::parse(
            "[[step]]\nexpect = \"$ \"\nsend = \"cd /srv\"\n[[step]]\nexpect = \"srv\"\nsend = \"ls\"\nenter = false\ntimeout = 5\n",
            "x.toml",
        )
        .unwrap();
        assert_eq!(s.steps.len(), 2);
        assert!(s.steps[0].enter && !s.steps[1].enter && s.steps[1].timeout_s == 5);
        for bad in [
            "",
            "[[step]]\nsend = \"x\"\n",
            "[[step]]\nexpect = \"a\"\nsend = \"x\"\nother = 1\n",
            "[[step]]\nexpect = \"a\"\nsend = \"export TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789\"\n",
            "[[step]]\nexpect = \"a\"\nsend = \"a\\u0003\"\n",
            "[[step]]\nexpect = \"a\"\nsend = \"x\"\ntimeout = 0\n",
            "top = 1\n",
        ] {
            assert!(LoginScript::parse(bad, "x").is_err(), "{bad:?}");
        }
    }

    #[test]
    fn options_that_run_commands_or_weaken_checks_are_refused() {
        assert!(option_ok("ServerAliveInterval=5"));
        assert!(option_ok("UserKnownHostsFile=C:/x/kh"));
        for bad in [
            "ProxyCommand=calc",
            "StrictHostKeyChecking=no",
            "forwardagent=yes",
            "LocalCommand=x",
            "noequals",
            "A=b\nProxyCommand=x",
            "Bad Key=x",
            "ProxyJump=evil",
            "LocalForward=1 h:2",
            "ForwardX11=yes",
            "GatewayPorts=yes",
        ] {
            assert!(!option_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn credential_manager_round_trip() {
        let target = format!(
            "mouv-subak:ssh:test@unit.invalid:{}",
            std::process::id() % 60000 + 1
        );
        crate::win::creds::write(&target, "test", "s3cret ✓").unwrap();
        assert_eq!(
            crate::win::creds::read(&target).as_deref(),
            Some("s3cret ✓")
        );
        assert!(crate::win::creds::delete(&target));
        assert_eq!(crate::win::creds::read(&target), None);
        assert!(!crate::win::creds::delete(&target));
    }
}
