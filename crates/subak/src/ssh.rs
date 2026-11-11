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
/// option, else `~/.ssh/known_hosts`.
pub fn known_hosts_files(options: &[String]) -> Vec<PathBuf> {
    for o in options {
        if let Some((k, f)) = o.split_once('=')
            && k.trim().eq_ignore_ascii_case("UserKnownHostsFile")
        {
            return f.split_whitespace().map(PathBuf::from).collect();
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

/// The command for `target`: strict host keys, no agent forwarding, the user's own options;
/// the saved secret through the askpass helper only for a verified host that has one.
pub fn plan(
    target: &Target,
    identity: Option<&str>,
    options: &[String],
    has_secret: bool,
    verified: bool,
) -> Option<Plan> {
    let program = openssh("ssh")?;
    let mut args: Vec<String> = vec![
        "-o".into(),
        "StrictHostKeyChecking=ask".into(),
        "-o".into(),
        "ForwardAgent=no".into(),
        "-p".into(),
        target.port.to_string(),
    ];
    if let Some(u) = &target.user {
        args.extend(["-l".into(), u.clone()]);
    }
    if let Some(i) = identity {
        args.extend(["-i".into(), i.to_string()]);
    }
    for o in options {
        args.extend(["-o".into(), o.clone()]);
    }
    args.push(target.host.clone());
    let askpass = has_secret && verified;
    let why = match (has_secret, verified) {
        (true, true) => {
            "the saved secret answers the password or passphrase prompt once".to_string()
        }
        (true, false) => {
            "a secret is saved but the host is not in known_hosts: ssh asks you".to_string()
        }
        (false, _) => "no saved secret: ssh asks you".to_string(),
    };
    let mut env = Vec::new();
    if askpass && let Ok(me) = std::env::current_exe() {
        env.push(("SSH_ASKPASS".into(), me.display().to_string()));
        env.push(("SSH_ASKPASS_REQUIRE".into(), "force".into()));
        env.push(("SUBAK_ASKPASS".into(), "1".into()));
        env.push(("SUBAK_SSH_CRED".into(), target.credential()));
    }
    Some(Plan {
        program,
        args,
        env,
        askpass,
        why,
    })
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

/// `SUBAK_ASKPASS=1`: `subak.exe <prompt>` called by ssh. Prints the saved secret for an
/// answerable prompt, once per connection; exits 1 otherwise (ssh then fails or asks nothing).
pub fn askpass_main(prompt: &str) -> std::process::ExitCode {
    use std::io::Write;
    if !answers(prompt) {
        return std::process::ExitCode::from(1);
    }
    let Ok(cred) = std::env::var("SUBAK_SSH_CRED") else {
        return std::process::ExitCode::from(1);
    };
    if !cred.starts_with("mouv-subak:ssh:") {
        return std::process::ExitCode::from(1);
    }
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
        let marker = dir.join(format!("{pid}-{started}"));
        if std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .is_err()
        {
            return std::process::ExitCode::from(1);
        }
        crate::win::creds::read(&cred)
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
    ];
    !k.is_empty()
        && k.chars().all(|c| c.is_ascii_alphanumeric())
        && !refused.contains(&k.as_str())
        && !v.contains(['\r', '\n', '\0'])
}

fn target_of(a: &Args) -> Result<Target, CliError> {
    let t = a.positional.first().map_or("", String::as_str);
    Target::parse(t)
        .map_err(|m| CliError::usage(m).hint("give the destination as [user@]host[:port]"))
}

fn has_secret(t: &Target) -> bool {
    #[cfg(windows)]
    return crate::win::creds::read(&t.credential()).is_some();
    #[cfg(not(windows))]
    return false;
}

/// `a` turned into the `new` of an SSH pane for `target` (profile or `ssh connect`).
pub fn apply(a: &Args, target: &Target, identity: Option<&str>) -> Result<(Args, Value), CliError> {
    let verified = verified(target, &known_hosts_files(&a.ssh_options));
    let p = plan(target, identity, &a.ssh_options, has_secret(target), verified).ok_or_else(|| {
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
    Ok((eff, info))
}

/// `subak ssh connect <[user@]host[:port]> [<name>] [--identity <key>] [--option K=V]…`: a
/// Terminal whose pane runs ssh (tabs, panes, detach and the Host lifecycle as any other).
pub fn connect(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let target = target_of(a)?;
    if let Some(i) = &a.identity
        && !std::path::Path::new(i).is_file()
    {
        return Err(CliError::new(
            Code::Io,
            format!("the key file `{i}` does not exist"),
        ));
    }
    let (mut eff, info) = apply(a, &target, a.identity.as_deref())?;
    eff.positional = a.positional.get(1).cloned().into_iter().collect();
    let mut out = crate::terminal::new(&eff, endpoint)?;
    out["ssh"] = info;
    Ok(out)
}

/// `subak ssh credential status|save|forget <[user@]host[:port]>`: the saved password or
/// passphrase in Credential Manager. Saving needs a host already in known_hosts (connected
/// and confirmed once); the secret comes from a no-echo console prompt or one stdin line and is
/// never printed.
pub fn credential(cmd: Cmd, a: &Args) -> Result<(Value, String), CliError> {
    let t = target_of(a)?;
    let verified = verified(&t, &known_hosts_files(&a.ssh_options));
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
        let Some(p) = plan(
            &t,
            Some("C:/k"),
            &["ServerAliveInterval=5".into()],
            true,
            false,
        ) else {
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
        let p = plan(&t, None, &[], true, true).unwrap();
        assert!(p.askpass);
        let names: Vec<&str> = p.env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"SSH_ASKPASS_REQUIRE"));
        assert!(
            !format!("{:?}", p.env).to_lowercase().contains("secret="),
            "no secret in the env"
        );
        assert!(!plan(&t, None, &[], false, true).unwrap().askpass);
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
