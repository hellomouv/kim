//! Step 35 (ADR 0068): SFTP file transfers over Windows' OpenSSH client.
//!
//! - **Transport:** `ssh.exe -s <host> sftp` (the SSH connection, host-key check, keys, agent and
//!   the saved secret exactly as Step 34's panes, but non-interactive: `StrictHostKeyChecking=yes`,
//!   never a prompt). Subak speaks SFTP version 3 over its pipes, so it sees every byte: real
//!   progress, cancellation between chunks, resuming from a partial file, and no shell parsing
//!   of file names.
//! - **Safe writes:** a transfer writes `<name>.subak-part` and renames it when complete; an
//!   existing destination is replaced only with `--overwrite`; `--resume` continues a partial
//!   file. Remote names are checked before anything is written locally (no `..`, separators,
//!   drive letters or Windows device names), symbolic links are never followed when copying a
//!   directory, and uploads are created `0644`/`0755` (`--private`: `0600`/`0700`).
//! - **Jobs:** `--background` runs the same command detached and reports to
//!   `%APPDATA%\mouv\subak\transfers\<id>.json` (no secret, ever); `subak transfer
//!   list|status|cancel` read and stop it.

use crate::cli::{Args, Cmd};
use crate::output::{CliError, Code};
use crate::ssh::{self, Target};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INIT: u8 = 1;
const VERSION: u8 = 2;
const OPEN: u8 = 3;
const CLOSE: u8 = 4;
const READ: u8 = 5;
const WRITE: u8 = 6;
const LSTAT: u8 = 7;
const OPENDIR: u8 = 11;
const READDIR: u8 = 12;
const REMOVE: u8 = 13;
const MKDIR: u8 = 14;
const RMDIR: u8 = 15;
const REALPATH: u8 = 16;
const STAT: u8 = 17;
const RENAME: u8 = 18;
const EXTENDED: u8 = 200;
const STATUS: u8 = 101;
const HANDLE: u8 = 102;
const DATA: u8 = 103;
const NAME: u8 = 104;
const ATTRS: u8 = 105;

const FX_EOF: u32 = 1;
const FX_NO_SUCH_FILE: u32 = 2;

const PF_READ: u32 = 1;
const PF_WRITE: u32 = 2;
const PF_CREAT: u32 = 8;
const PF_TRUNC: u32 = 0x10;

const ATTR_SIZE: u32 = 1;
const ATTR_UIDGID: u32 = 2;
const ATTR_PERMISSIONS: u32 = 4;
const ATTR_ACMODTIME: u32 = 8;
const ATTR_EXTENDED: u32 = 0x8000_0000;

const S_IFMT: u32 = 0o170_000;
const S_IFDIR: u32 = 0o040_000;
const S_IFREG: u32 = 0o100_000;
const S_IFLNK: u32 = 0o120_000;

/// Bytes per read or write request, and how many are in flight.
const CHUNK: u32 = 32 * 1024;
const WINDOW: usize = 16;
/// The largest packet accepted from the server.
const MAX_PACKET: usize = 256 * 1024 + 1024;
/// The suffix of a file being written.
pub const PART: &str = ".subak-part";

/// Why an SFTP operation failed.
#[derive(Debug)]
pub enum SErr {
    /// ssh could not connect, authenticate or verify the host, or the connection broke off:
    /// ssh's own message.
    Conn(String),
    /// The server refused: its status code and message.
    Status(u32, String),
    Proto(String),
    Local(String),
    Exists(String),
    Unsafe(String),
    Cancelled,
}

impl SErr {
    pub fn into_cli(self, target: &Target) -> CliError {
        match self {
            Self::Conn(m) => {
                let e = CliError::new(
                    Code::SshFailed,
                    format!("the SSH connection to {} failed: {m}", target.shown()),
                );
                if m.contains("Host key verification failed")
                    || m.contains("No ") && m.contains("host key is known")
                {
                    e.hint(format!(
                        "connect once with `subak ssh connect {}`, check the fingerprint and answer yes; transfers never trust a host by themselves",
                        target.shown()
                    ))
                } else if m.contains("Permission denied") {
                    e.hint(format!(
                        "use a key (--identity) or save the password or passphrase: `subak ssh credential save {}`",
                        target.shown()
                    ))
                } else {
                    e
                }
            }
            Self::Status(c, m) => {
                CliError::new(Code::RemoteError, format!("the server refused: {m}"))
                    .detail(format!("status: {}", status_name(c)))
            }
            Self::Proto(m) => CliError::new(
                Code::RemoteError,
                format!("the SFTP server misbehaved: {m}"),
            ),
            Self::Local(m) => CliError::new(Code::Io, m),
            Self::Exists(p) => CliError::new(Code::FileExists, format!("`{p}` already exists"))
                .hint("pass --overwrite to replace it, or --resume to continue a partial transfer"),
            Self::Unsafe(n) => CliError::new(
                Code::UnsafePath,
                format!(
                    "the server listed `{}`, which is not a safe local file name; nothing was transferred",
                    n.escape_debug()
                ),
            ),
            Self::Cancelled => CliError::new(
                Code::Cancelled,
                format!(
                    "the transfer was cancelled; the partial file (`*{PART}`) is kept for --resume"
                ),
            ),
        }
    }
}

fn status_name(c: u32) -> &'static str {
    match c {
        1 => "eof",
        2 => "no_such_file",
        3 => "permission_denied",
        4 => "failure",
        5 => "bad_message",
        6 => "no_connection",
        7 => "connection_lost",
        8 => "op_unsupported",
        _ => "other",
    }
}

fn io_err(what: &str, e: &std::io::Error) -> SErr {
    SErr::Local(format!("{what}: {e}"))
}

/// File attributes (the parts Subak uses).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Attrs {
    pub size: Option<u64>,
    pub perm: Option<u32>,
    pub mtime: Option<u32>,
}

impl Attrs {
    pub fn kind(&self) -> &'static str {
        match self.perm.map(|p| p & S_IFMT) {
            Some(S_IFDIR) => "dir",
            Some(S_IFREG) => "file",
            Some(S_IFLNK) => "link",
            Some(_) => "other",
            None => "unknown",
        }
    }

    fn is_dir(&self) -> bool {
        self.kind() == "dir"
    }
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], SErr> {
        let s = self
            .b
            .get(self.i..self.i + n)
            .ok_or_else(|| SErr::Proto("a short packet".into()))?;
        self.i += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, SErr> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64, SErr> {
        Ok((u64::from(self.u32()?) << 32) | u64::from(self.u32()?))
    }
    fn bytes(&mut self) -> Result<Vec<u8>, SErr> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self) -> Result<String, SErr> {
        Ok(String::from_utf8_lossy(&self.bytes()?).into_owned())
    }
    fn attrs(&mut self) -> Result<Attrs, SErr> {
        let f = self.u32()?;
        let mut a = Attrs::default();
        if f & ATTR_SIZE != 0 {
            a.size = Some(self.u64()?);
        }
        if f & ATTR_UIDGID != 0 {
            self.u32()?;
            self.u32()?;
        }
        if f & ATTR_PERMISSIONS != 0 {
            a.perm = Some(self.u32()?);
        }
        if f & ATTR_ACMODTIME != 0 {
            self.u32()?;
            a.mtime = Some(self.u32()?);
        }
        if f & ATTR_EXTENDED != 0 {
            for _ in 0..self.u32()? {
                self.bytes()?;
                self.bytes()?;
            }
        }
        Ok(a)
    }
}

fn put_u32(v: &mut Vec<u8>, n: u32) {
    v.extend_from_slice(&n.to_be_bytes());
}
fn put_u64(v: &mut Vec<u8>, n: u64) {
    v.extend_from_slice(&n.to_be_bytes());
}
fn put_str(v: &mut Vec<u8>, s: &[u8]) {
    put_u32(v, u32::try_from(s.len()).unwrap_or(u32::MAX));
    v.extend_from_slice(s);
}
fn put_perm(v: &mut Vec<u8>, perm: Option<u32>) {
    match perm {
        Some(p) => {
            put_u32(v, ATTR_PERMISSIONS);
            put_u32(v, p);
        }
        None => put_u32(v, 0),
    }
}

/// One directory entry.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub attrs: Attrs,
}

/// An SFTP version 3 session over a byte stream (ssh's pipes, or a test server).
pub struct Sftp {
    tx: Box<dyn Write + Send>,
    rx: Box<dyn Read + Send>,
    child: Option<std::process::Child>,
    stderr: Arc<Mutex<String>>,
    next: u32,
    posix_rename: bool,
}

impl Drop for Sftp {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            // Closing stdin ends the subsystem; a stuck ssh is ended.
            self.tx = Box::new(std::io::sink());
            let end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < end {
                if c.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Sftp {
    /// Starts the session over `rx`/`tx` (INIT, VERSION).
    pub fn over(
        tx: Box<dyn Write + Send>,
        rx: Box<dyn Read + Send>,
        child: Option<std::process::Child>,
        stderr: Arc<Mutex<String>>,
    ) -> Result<Self, SErr> {
        let mut s = Self {
            tx,
            rx,
            child,
            stderr,
            next: 1,
            posix_rename: false,
        };
        let mut b = vec![INIT];
        put_u32(&mut b, 3);
        s.packet(&b)?;
        let (t, body) = s.read_packet()?;
        if t != VERSION {
            return Err(SErr::Proto(format!("expected VERSION, got packet {t}")));
        }
        let mut r = Rd { b: &body, i: 0 };
        let v = r.u32()?;
        if v < 3 {
            return Err(SErr::Proto(format!("SFTP version {v} is too old")));
        }
        while r.i < body.len() {
            let name = r.string()?;
            r.bytes()?;
            if name == "posix-rename@openssh.com" {
                s.posix_rename = true;
            }
        }
        Ok(s)
    }

    /// `ssh.exe … -s <host> sftp` for `target`.
    pub fn connect(target: &Target, conn: &ssh::Conn) -> Result<Self, SErr> {
        let (program, args, env) = ssh::batch(target, conn, "sftp").ok_or_else(|| {
            SErr::Conn(
                "Windows' OpenSSH client (System32\\OpenSSH\\ssh.exe) is not installed".into(),
            )
        })?;
        let mut c = std::process::Command::new(program);
        c.args(&args)
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let mut child = c
            .spawn()
            .map_err(|e| SErr::Conn(format!("ssh could not be started: {e}")))?;
        let tx = child
            .stdin
            .take()
            .ok_or_else(|| SErr::Conn("no pipe".into()))?;
        let rx = child
            .stdout
            .take()
            .ok_or_else(|| SErr::Conn("no pipe".into()))?;
        let err = child.stderr.take();
        let stderr = Arc::new(Mutex::new(String::new()));
        if let Some(mut e) = err {
            let sink = stderr.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                while let Ok(n) = e.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut s) = sink.lock()
                        && s.len() < 8192
                    {
                        s.push_str(&String::from_utf8_lossy(&buf[..n]));
                    }
                }
            });
        }
        Self::over(Box::new(tx), Box::new(rx), Some(child), stderr)
    }

    /// ssh's message after the stream ended.
    fn ended(&mut self) -> SErr {
        if let Some(c) = self.child.as_mut() {
            let end = Instant::now() + Duration::from_secs(5);
            while Instant::now() < end && c.try_wait().ok().flatten().is_none() {
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let text = self.stderr.lock().map(|s| s.clone()).unwrap_or_default();
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let tail = lines[lines.len().saturating_sub(3)..].join(" ");
        SErr::Conn(if tail.is_empty() {
            "the connection ended".into()
        } else {
            tail
        })
    }

    fn packet(&mut self, body: &[u8]) -> Result<(), SErr> {
        let mut p = Vec::with_capacity(body.len() + 4);
        put_str(&mut p, body);
        if self
            .tx
            .write_all(&p)
            .and_then(|()| self.tx.flush())
            .is_err()
        {
            return Err(self.ended());
        }
        Ok(())
    }

    fn read_packet(&mut self) -> Result<(u8, Vec<u8>), SErr> {
        let mut len = [0u8; 4];
        if self.rx.read_exact(&mut len).is_err() {
            return Err(self.ended());
        }
        let n = u32::from_be_bytes(len) as usize;
        if n == 0 || n > MAX_PACKET {
            return Err(SErr::Proto(format!("a packet of {n} bytes")));
        }
        let mut b = vec![0u8; n];
        if self.rx.read_exact(&mut b).is_err() {
            return Err(self.ended());
        }
        let t = b[0];
        b.remove(0);
        Ok((t, b))
    }

    /// Sends request `t` with `body` after the id; returns the id.
    fn send(&mut self, t: u8, body: &[u8]) -> Result<u32, SErr> {
        let id = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        let mut b = Vec::with_capacity(body.len() + 5);
        b.push(t);
        put_u32(&mut b, id);
        b.extend_from_slice(body);
        self.packet(&b)?;
        Ok(id)
    }

    /// The next reply: its type, id and the rest.
    fn reply(&mut self) -> Result<(u8, u32, Vec<u8>), SErr> {
        let (t, b) = self.read_packet()?;
        let mut r = Rd { b: &b, i: 0 };
        let id = r.u32()?;
        Ok((t, id, b[4..].to_vec()))
    }

    fn call(&mut self, t: u8, body: &[u8]) -> Result<(u8, Vec<u8>), SErr> {
        let id = self.send(t, body)?;
        let (rt, rid, rest) = self.reply()?;
        if rid != id {
            return Err(SErr::Proto("a reply to another request".into()));
        }
        Ok((rt, rest))
    }

    fn status(t: u8, rest: &[u8]) -> Result<(), SErr> {
        if t != STATUS {
            return Err(SErr::Proto(format!("expected STATUS, got packet {t}")));
        }
        let mut r = Rd { b: rest, i: 0 };
        let code = r.u32()?;
        if code == 0 {
            return Ok(());
        }
        let msg = r.string().unwrap_or_default();
        Err(SErr::Status(
            code,
            if msg.is_empty() {
                status_name(code).into()
            } else {
                msg
            },
        ))
    }

    fn path_call(&mut self, t: u8, path: &str) -> Result<(u8, Vec<u8>), SErr> {
        let mut b = Vec::new();
        put_str(&mut b, path.as_bytes());
        self.call(t, &b)
    }

    fn handle(t: u8, rest: &[u8]) -> Result<Vec<u8>, SErr> {
        if t == HANDLE {
            Rd { b: rest, i: 0 }.bytes()
        } else {
            Self::status(t, rest).and(Err(SErr::Proto("no handle".into())))
        }
    }

    pub fn realpath(&mut self, path: &str) -> Result<String, SErr> {
        let (t, rest) = self.path_call(REALPATH, path)?;
        if t != NAME {
            Self::status(t, &rest)?;
            return Err(SErr::Proto("no name".into()));
        }
        let mut r = Rd { b: &rest, i: 0 };
        if r.u32()? == 0 {
            return Err(SErr::Proto("no name".into()));
        }
        r.string()
    }

    fn attrs_call(&mut self, t: u8, path: &str) -> Result<Option<Attrs>, SErr> {
        let (rt, rest) = self.path_call(t, path)?;
        if rt == ATTRS {
            return Ok(Some(Rd { b: &rest, i: 0 }.attrs()?));
        }
        match Self::status(rt, &rest) {
            Err(SErr::Status(FX_NO_SUCH_FILE, _)) => Ok(None),
            Err(e) => Err(e),
            Ok(()) => Err(SErr::Proto("no attributes".into())),
        }
    }

    /// The attributes of `path` (following links); `None` when it does not exist.
    pub fn stat(&mut self, path: &str) -> Result<Option<Attrs>, SErr> {
        self.attrs_call(STAT, path)
    }

    pub fn lstat(&mut self, path: &str) -> Result<Option<Attrs>, SErr> {
        self.attrs_call(LSTAT, path)
    }

    fn close(&mut self, h: &[u8]) -> Result<(), SErr> {
        let mut b = Vec::new();
        put_str(&mut b, h);
        let (t, rest) = self.call(CLOSE, &b)?;
        Self::status(t, &rest)
    }

    /// The entries of directory `path` (without `.` and `..`), as the server names them.
    pub fn list(&mut self, path: &str) -> Result<Vec<Entry>, SErr> {
        let (t, rest) = self.path_call(OPENDIR, path)?;
        let h = Self::handle(t, &rest)?;
        let mut out = Vec::new();
        loop {
            let mut b = Vec::new();
            put_str(&mut b, &h);
            let (t, rest) = self.call(READDIR, &b)?;
            if t == STATUS {
                match Self::status(t, &rest) {
                    Err(SErr::Status(FX_EOF, _)) | Ok(()) => break,
                    Err(e) => {
                        let _ = self.close(&h);
                        return Err(e);
                    }
                }
            }
            if t != NAME {
                return Err(SErr::Proto(format!("expected NAME, got packet {t}")));
            }
            let mut r = Rd { b: &rest, i: 0 };
            for _ in 0..r.u32()? {
                let name = r.string()?;
                r.string()?;
                let attrs = r.attrs()?;
                if name != "." && name != ".." {
                    out.push(Entry { name, attrs });
                }
            }
            if out.len() > 100_000 {
                return Err(SErr::Proto("more than 100000 entries".into()));
            }
        }
        self.close(&h)?;
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn mkdir(&mut self, path: &str, perm: u32) -> Result<(), SErr> {
        let mut b = Vec::new();
        put_str(&mut b, path.as_bytes());
        put_perm(&mut b, Some(perm));
        let (t, rest) = self.call(MKDIR, &b)?;
        Self::status(t, &rest)
    }

    pub fn remove(&mut self, path: &str) -> Result<(), SErr> {
        let (t, rest) = self.path_call(REMOVE, path)?;
        Self::status(t, &rest)
    }

    pub fn rmdir(&mut self, path: &str) -> Result<(), SErr> {
        let (t, rest) = self.path_call(RMDIR, path)?;
        Self::status(t, &rest)
    }

    /// Renames `from` to `to`, replacing `to` when `replace` (OpenSSH's `posix-rename`, else a
    /// remove first).
    pub fn rename(&mut self, from: &str, to: &str, replace: bool) -> Result<(), SErr> {
        let mut b = Vec::new();
        if replace && self.posix_rename {
            put_str(&mut b, b"posix-rename@openssh.com");
            put_str(&mut b, from.as_bytes());
            put_str(&mut b, to.as_bytes());
            let (t, rest) = self.call(EXTENDED, &b)?;
            return Self::status(t, &rest);
        }
        if replace && self.stat(to)?.is_some() {
            self.remove(to)?;
        }
        put_str(&mut b, from.as_bytes());
        put_str(&mut b, to.as_bytes());
        let (t, rest) = self.call(RENAME, &b)?;
        Self::status(t, &rest)
    }

    fn open(&mut self, path: &str, flags: u32, perm: Option<u32>) -> Result<Vec<u8>, SErr> {
        let mut b = Vec::new();
        put_str(&mut b, path.as_bytes());
        put_u32(&mut b, flags);
        put_perm(&mut b, perm);
        let (t, rest) = self.call(OPEN, &b)?;
        Self::handle(t, &rest)
    }

    /// Reads remote `path` from `start` into `file` (at the same offsets), `size` bytes in all.
    fn download(
        &mut self,
        path: &str,
        file: &mut std::fs::File,
        start: u64,
        size: u64,
        tick: &mut dyn FnMut(u64) -> bool,
    ) -> Result<u64, SErr> {
        let h = self.open(path, PF_READ, None)?;
        let mut inflight: HashMap<u32, (u64, u32)> = HashMap::new();
        let mut next = start;
        let mut end = size;
        let mut done = start;
        let mut retry: Vec<(u64, u32)> = Vec::new();
        let result = loop {
            while inflight.len() < WINDOW {
                let (off, len) = if let Some(r) = retry.pop() {
                    r
                } else if next < end {
                    let len = u32::try_from((end - next).min(u64::from(CHUNK))).unwrap_or(CHUNK);
                    let o = next;
                    next += u64::from(len);
                    (o, len)
                } else {
                    break;
                };
                let mut b = Vec::new();
                put_str(&mut b, &h);
                put_u64(&mut b, off);
                put_u32(&mut b, len);
                let id = self.send(READ, &b)?;
                inflight.insert(id, (off, len));
            }
            if inflight.is_empty() {
                break Ok(done);
            }
            let (t, id, rest) = self.reply()?;
            let Some((off, len)) = inflight.remove(&id) else {
                break Err(SErr::Proto("a reply to another request".into()));
            };
            match t {
                DATA => {
                    let data = Rd { b: &rest, i: 0 }.bytes()?;
                    let n = u32::try_from(data.len()).unwrap_or(u32::MAX).min(len);
                    if n == 0 {
                        break Err(SErr::Proto("an empty read".into()));
                    }
                    if let Err(e) = file
                        .seek(SeekFrom::Start(off))
                        .and_then(|_| file.write_all(&data[..n as usize]))
                    {
                        break Err(io_err("cannot write the local file", &e));
                    }
                    done += u64::from(n);
                    if n < len {
                        retry.push((off + u64::from(n), len - n));
                    }
                    if !tick(done) {
                        break Err(SErr::Cancelled);
                    }
                }
                _ => match Self::status(t, &rest) {
                    // The file is shorter than it was: it ends here.
                    Err(SErr::Status(FX_EOF, _)) => {
                        end = end.min(off);
                        next = next.min(end);
                        retry.retain(|(o, _)| *o < end);
                    }
                    Err(e) => break Err(e),
                    Ok(()) => break Err(SErr::Proto("STATUS ok for a read".into())),
                },
            }
        };
        // Replies still in flight are read so the session stays usable.
        while !inflight.is_empty() {
            match self.reply() {
                Ok((_, id, _)) => {
                    inflight.remove(&id);
                }
                Err(_) => break,
            }
        }
        let _ = self.close(&h);
        result.map(|d| d.min(end))
    }

    /// Writes `file` from `start` to remote `path` (created with `perm`).
    fn upload(
        &mut self,
        path: &str,
        file: &mut std::fs::File,
        start: u64,
        perm: u32,
        tick: &mut dyn FnMut(u64) -> bool,
    ) -> Result<u64, SErr> {
        let flags = PF_WRITE | PF_CREAT | if start == 0 { PF_TRUNC } else { 0 };
        let h = self.open(path, flags, Some(perm))?;
        file.seek(SeekFrom::Start(start))
            .map_err(|e| io_err("cannot read the local file", &e))?;
        let mut off = start;
        let mut inflight: HashMap<u32, u32> = HashMap::new();
        let mut eof = false;
        let mut done = start;
        let mut buf = vec![0u8; CHUNK as usize];
        let result = loop {
            while !eof && inflight.len() < WINDOW {
                let n = match file.read(&mut buf) {
                    Ok(n) => n,
                    Err(e) => return Err(io_err("cannot read the local file", &e)),
                };
                if n == 0 {
                    eof = true;
                    break;
                }
                let mut b = Vec::with_capacity(n + 64);
                put_str(&mut b, &h);
                put_u64(&mut b, off);
                put_str(&mut b, &buf[..n]);
                let id = self.send(WRITE, &b)?;
                let n = u32::try_from(n).unwrap_or(CHUNK);
                inflight.insert(id, n);
                off += u64::from(n);
            }
            if inflight.is_empty() {
                break Ok(done);
            }
            let (t, id, rest) = self.reply()?;
            let Some(n) = inflight.remove(&id) else {
                break Err(SErr::Proto("a reply to another request".into()));
            };
            if let Err(e) = Self::status(t, &rest) {
                break Err(e);
            }
            done += u64::from(n);
            if !tick(done) {
                break Err(SErr::Cancelled);
            }
        };
        while !inflight.is_empty() {
            match self.reply() {
                Ok((_, id, _)) => {
                    inflight.remove(&id);
                }
                Err(_) => break,
            }
        }
        let closed = self.close(&h);
        let d = result?;
        closed?;
        Ok(d)
    }
}

// ---------------------------------------------------------------------------------------------
// Names and paths.

/// Whether a name the server listed is safe as one local file name: no `.`/`..`, no separator
/// or drive, no control character, no Windows device name, no trailing dot or space.
pub fn safe_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
    {
        return false;
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    !device
}

fn rjoin(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn rbase(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

fn part_of(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(PART);
    PathBuf::from(s)
}

// ---------------------------------------------------------------------------------------------
// Transfers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Get,
    Put,
}

/// What a transfer does.
#[derive(Debug, Clone)]
pub struct Plan {
    pub dir: Dir,
    pub remote: String,
    pub local: PathBuf,
    pub recursive: bool,
    pub overwrite: bool,
    pub resume: bool,
    pub private: bool,
}

/// One file of a transfer: remote path, local path, size.
#[derive(Debug, Clone)]
struct Item {
    remote: String,
    local: PathBuf,
    size: u64,
}

/// Progress of a transfer.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub files_total: u64,
    pub files_done: u64,
    pub current: String,
}

impl Progress {
    pub fn json(&self) -> Value {
        json!({
            "bytes_total": self.bytes_total,
            "bytes_done": self.bytes_done,
            "files_total": self.files_total,
            "files_done": self.files_done,
            "current": self.current,
        })
    }
}

/// Who watches a transfer: called after every chunk; `false` cancels it.
pub trait Watch {
    fn tick(&mut self, p: &Progress) -> bool;
}

struct Quiet;
impl Watch for Quiet {
    fn tick(&mut self, _: &Progress) -> bool {
        true
    }
}

/// The files of a download, checked before anything is written: every remote name is safe,
/// links are skipped, and existing local files are refused unless `--overwrite`/`--resume`.
fn plan_get(s: &mut Sftp, p: &Plan, skipped: &mut Vec<String>) -> Result<Vec<Item>, SErr> {
    let a = s
        .stat(&p.remote)?
        .ok_or_else(|| SErr::Status(FX_NO_SUCH_FILE, format!("`{}` does not exist", p.remote)))?;
    let name = rbase(&p.remote).to_string();
    let into_dir = p.local.is_dir();
    if a.is_dir() {
        if !p.recursive {
            return Err(SErr::Local(format!(
                "`{}` is a directory; pass --recursive to download it",
                p.remote
            )));
        }
        if into_dir && !safe_name(&name) {
            return Err(SErr::Unsafe(name));
        }
        let root = if into_dir {
            p.local.join(&name)
        } else {
            p.local.clone()
        };
        let mut out = Vec::new();
        let mut stack = vec![(p.remote.clone(), root)];
        while let Some((rdir, ldir)) = stack.pop() {
            for e in s.list(&rdir)? {
                if !safe_name(&e.name) {
                    return Err(SErr::Unsafe(e.name));
                }
                let (r, l) = (rjoin(&rdir, &e.name), ldir.join(&e.name));
                match e.attrs.kind() {
                    "dir" => stack.push((r, l)),
                    "file" => out.push(Item {
                        remote: r,
                        local: l,
                        size: e.attrs.size.unwrap_or(0),
                    }),
                    // Links and special files are never followed or created.
                    _ => skipped.push(r),
                }
                if out.len() > 100_000 {
                    return Err(SErr::Local("more than 100000 files".into()));
                }
            }
        }
        Ok(out)
    } else {
        let local = if into_dir {
            if !safe_name(&name) {
                return Err(SErr::Unsafe(name));
            }
            p.local.join(&name)
        } else {
            p.local.clone()
        };
        Ok(vec![Item {
            remote: p.remote.clone(),
            local,
            size: a.size.unwrap_or(0),
        }])
    }
}

/// The files of an upload: a local walk that never follows links or junctions.
fn plan_put(
    s: &mut Sftp,
    p: &Plan,
    skipped: &mut Vec<String>,
) -> Result<(Vec<Item>, Vec<String>), SErr> {
    let meta = std::fs::symlink_metadata(&p.local)
        .map_err(|e| io_err(&format!("cannot read `{}`", p.local.display()), &e))?;
    let name = p
        .local
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let remote_dir = s.stat(&p.remote)?.is_some_and(|a| a.is_dir());
    let root = if remote_dir {
        rjoin(&p.remote, &name)
    } else {
        p.remote.clone()
    };
    if !meta.is_dir() {
        return Ok((
            vec![Item {
                remote: root,
                local: p.local.clone(),
                size: meta.len(),
            }],
            Vec::new(),
        ));
    }
    if !p.recursive {
        return Err(SErr::Local(format!(
            "`{}` is a folder; pass --recursive to upload it",
            p.local.display()
        )));
    }
    let mut dirs = vec![root.clone()];
    let mut out = Vec::new();
    let mut stack = vec![(p.local.clone(), root)];
    while let Some((ldir, rdir)) = stack.pop() {
        let rd = std::fs::read_dir(&ldir)
            .map_err(|e| io_err(&format!("cannot list `{}`", ldir.display()), &e))?;
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let n = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            let (l, r) = (e.path(), rjoin(&rdir, &n));
            if ft.is_symlink() {
                skipped.push(l.display().to_string());
            } else if ft.is_dir() {
                dirs.push(r.clone());
                stack.push((l, r));
            } else if ft.is_file() {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(Item {
                    remote: r,
                    local: l,
                    size,
                });
            }
        }
    }
    Ok((out, dirs))
}

/// Runs a transfer; the JSON summary.
pub fn transfer(s: &mut Sftp, p: &Plan, watch: &mut dyn Watch) -> Result<Value, SErr> {
    let mut skipped = Vec::new();
    let (items, dirs) = match p.dir {
        Dir::Get => (plan_get(s, p, &mut skipped)?, Vec::new()),
        Dir::Put => plan_put(s, p, &mut skipped)?,
    };
    // Every conflict is found before anything is written.
    if !p.overwrite {
        for it in &items {
            let exists = match p.dir {
                Dir::Get => it.local.exists(),
                Dir::Put => s.lstat(&it.remote)?.is_some(),
            };
            if exists {
                return Err(SErr::Exists(match p.dir {
                    Dir::Get => it.local.display().to_string(),
                    Dir::Put => it.remote.clone(),
                }));
            }
        }
    }
    let (fperm, dperm) = if p.private {
        (0o600, 0o700)
    } else {
        (0o644, 0o755)
    };
    for d in &dirs {
        match s.stat(d)? {
            Some(a) if a.is_dir() => {}
            Some(_) => return Err(SErr::Exists(d.clone())),
            None => s.mkdir(d, dperm)?,
        }
    }
    let mut prog = Progress {
        bytes_total: items.iter().map(|i| i.size).sum(),
        files_total: items.len() as u64,
        ..Progress::default()
    };
    let mut resumed = 0u64;
    let started = Instant::now();
    // Tests only (debug builds): a pause per chunk, so a cancel or a dropped connection lands
    // in the middle of a transfer.
    #[cfg(debug_assertions)]
    let delay = std::env::var("SUBAK_TEST_SFTP_DELAY_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis);
    #[cfg(not(debug_assertions))]
    let delay: Option<Duration> = None;
    for it in &items {
        prog.current = match p.dir {
            Dir::Get => it.remote.clone(),
            Dir::Put => it.local.display().to_string(),
        };
        let base = prog.bytes_done;
        let mut last = Instant::now() - Duration::from_secs(1);
        let mut tick = |done: u64| {
            if let Some(d) = delay {
                std::thread::sleep(d);
            }
            prog.bytes_done = base + done;
            if last.elapsed() >= Duration::from_millis(100) {
                last = Instant::now();
                return watch.tick(&prog);
            }
            true
        };
        match p.dir {
            Dir::Get => {
                if let Some(parent) = it.local.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        io_err(&format!("cannot create `{}`", parent.display()), &e)
                    })?;
                }
                let part = part_of(&it.local);
                let have = if p.resume {
                    std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                };
                let start = if have <= it.size { have } else { 0 };
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(start == 0)
                    .open(&part)
                    .map_err(|e| io_err(&format!("cannot write `{}`", part.display()), &e))?;
                resumed += start;
                let got = s.download(&it.remote, &mut f, start, it.size, &mut tick)?;
                f.set_len(got)
                    .and_then(|()| f.sync_all())
                    .map_err(|e| io_err("cannot finish the local file", &e))?;
                drop(f);
                if it.local.exists() {
                    if !p.overwrite {
                        return Err(SErr::Exists(it.local.display().to_string()));
                    }
                    #[cfg(windows)]
                    crate::win::replace_file(&part, &it.local)
                        .map_err(|e| io_err("cannot replace the local file", &e))?;
                    #[cfg(not(windows))]
                    std::fs::rename(&part, &it.local).map_err(|e| io_err("cannot rename", &e))?;
                } else {
                    std::fs::rename(&part, &it.local)
                        .map_err(|e| io_err("cannot rename the finished file", &e))?;
                }
            }
            Dir::Put => {
                let part = format!("{}{PART}", it.remote);
                let have = if p.resume {
                    s.stat(&part)?.and_then(|a| a.size).unwrap_or(0)
                } else {
                    0
                };
                let start = if have <= it.size { have } else { 0 };
                let mut f = std::fs::File::open(&it.local)
                    .map_err(|e| io_err(&format!("cannot read `{}`", it.local.display()), &e))?;
                resumed += start;
                s.upload(&part, &mut f, start, fperm, &mut tick)?;
                s.rename(&part, &it.remote, p.overwrite)?;
            }
        }
        prog.bytes_done = base + it.size;
        prog.files_done += 1;
        if !watch.tick(&prog) {
            return Err(SErr::Cancelled);
        }
    }
    let secs = started.elapsed().as_secs_f64().max(0.001);
    Ok(json!({
        "direction": if p.dir == Dir::Get { "get" } else { "put" },
        "remote": p.remote,
        "local": p.local.display().to_string(),
        "files": prog.files_done,
        "bytes": prog.bytes_done,
        "resumed_bytes": resumed,
        "skipped": skipped,
        "seconds": (secs * 1000.0).round() / 1000.0,
    }))
}

// ---------------------------------------------------------------------------------------------
// Jobs.

fn jobs_dir() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("transfers"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0)
}

fn valid_job_id(id: &str) -> bool {
    id.len() <= 64
        && id.starts_with("xfer-")
        && id[5..].chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn write_job(dir: &Path, doc: &Value) {
    let Some(id) = doc["id"].as_str() else { return };
    let path = dir.join(format!("{id}.json"));
    let tmp = dir.join(format!("{id}.json.tmp"));
    if std::fs::write(&tmp, serde_json::to_vec_pretty(doc).unwrap_or_default()).is_ok() {
        #[cfg(windows)]
        let _ = crate::win::replace_file(&tmp, &path);
        #[cfg(not(windows))]
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn read_job(dir: &Path, id: &str) -> Option<Value> {
    // A reader may meet the file while it is replaced: try again shortly.
    for _ in 0..5 {
        if let Ok(b) = std::fs::read(dir.join(format!("{id}.json")))
            && let Ok(v) = serde_json::from_slice::<Value>(&b)
        {
            return Some(v);
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    None
}

fn alive(pid: u64) -> bool {
    #[cfg(windows)]
    return u32::try_from(pid)
        .ok()
        .and_then(crate::win::open_process)
        .is_some_and(|h| crate::win::exit_code(h.raw()).is_none());
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

/// A job's record with a dead worker marked `interrupted`.
fn checked(dir: &Path, mut v: Value) -> Value {
    let running = matches!(v["state"].as_str(), Some("queued" | "running"));
    if running && !v["pid"].as_u64().is_some_and(alive) {
        v["state"] = json!("interrupted");
        v["error"] = json!({"code": "cancelled", "message": "the transfer process ended unexpectedly; run it again with --resume"});
        v["finished"] = json!(now_ms());
        write_job(dir, &v);
    }
    v
}

/// The worker's watcher: progress into the job file, a `.cancel` file stops it.
struct JobWatch {
    dir: PathBuf,
    doc: Value,
    last: Instant,
}

impl Watch for JobWatch {
    fn tick(&mut self, p: &Progress) -> bool {
        if self.last.elapsed() < Duration::from_millis(250) && p.files_done < p.files_total {
            return true;
        }
        self.last = Instant::now();
        let id = self.doc["id"].as_str().unwrap_or("").to_string();
        if self.dir.join(format!("{id}.cancel")).exists() {
            return false;
        }
        self.doc["state"] = json!("running");
        self.doc["progress"] = p.json();
        self.doc["updated"] = json!(now_ms());
        write_job(&self.dir, &self.doc);
        true
    }
}

/// Foreground progress on a console's stderr.
struct ConsoleWatch;

impl Watch for ConsoleWatch {
    fn tick(&mut self, p: &Progress) -> bool {
        let pct = p
            .bytes_done
            .saturating_mul(100)
            .checked_div(p.bytes_total)
            .unwrap_or(100);
        eprint!(
            "\r{pct:>3}%  {:.1}/{:.1} MiB  {}/{} files   ",
            p.bytes_done as f64 / 1_048_576.0,
            p.bytes_total as f64 / 1_048_576.0,
            p.files_done,
            p.files_total
        );
        true
    }
}

/// Starts `subak` again, detached, as the worker of job `id` (the same command line without
/// `--background`).
fn spawn_worker(id: &str) -> Result<u32, CliError> {
    let me = std::env::current_exe()
        .map_err(|e| CliError::new(Code::SpawnFailed, format!("cannot find subak.exe: {e}")))?;
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--background")
        .collect();
    // Detached, no window, out of the caller's job when allowed, and inheriting no handle: a
    // caller reading this command's output is not kept waiting for the whole transfer.
    #[cfg(windows)]
    {
        let env = crate::win::EnvBlock::from_process().with_overrides(
            &crate::pty::EnvOverrides::default(),
            &[("SUBAK_TRANSFER_JOB", id)],
        );
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
        crate::win::spawn::spawn_detached(&me, |_| args.clone(), &cwd, Some(&env))
            .map(|s| s.pid)
            .map_err(|e| {
                CliError::new(Code::SpawnFailed, format!("cannot start the transfer: {e}"))
            })
    }
    #[cfg(not(windows))]
    {
        let _ = (me, args, id);
        Err(CliError::new(
            Code::UnsupportedPlatform,
            "background transfers need Windows",
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Commands.

fn remote_arg(a: &Args, i: usize, what: &str) -> Result<String, CliError> {
    let r = a
        .positional
        .get(i)
        .cloned()
        .ok_or_else(|| CliError::usage(format!("give the {what}")))?;
    if r.is_empty() || r.contains('\0') || r.contains('\n') || r.contains('\r') {
        return Err(CliError::usage(format!(
            "`{}` is not a remote path",
            r.escape_debug()
        )));
    }
    Ok(r)
}

fn local_arg(a: &Args, i: usize) -> Result<PathBuf, CliError> {
    let p = a.positional.get(i).map_or_else(
        || std::env::current_dir().unwrap_or_default(),
        PathBuf::from,
    );
    Ok(if p.is_absolute() {
        p
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    })
}

fn open(target: &Target, conn: &ssh::Conn) -> Result<Sftp, CliError> {
    Sftp::connect(target, conn).map_err(|e| e.into_cli(target))
}

fn entry_json(e: &Entry) -> Value {
    json!({
        "name": e.name,
        "type": e.attrs.kind(),
        "size": e.attrs.size,
        "mode": e.attrs.perm.map(|p| format!("{:o}", p & 0o7777)),
        "mtime": e.attrs.mtime,
    })
}

/// `subak sftp ls|get|put|mkdir|rm` and `subak transfer list|status|cancel` (no Host).
pub fn run(cmd: Cmd, a: &Args) -> Result<(Value, String), CliError> {
    match cmd {
        Cmd::TransferList | Cmd::TransferStatus | Cmd::TransferCancel => return jobs(cmd, a),
        _ => {}
    }
    let target = ssh::target_of(a)?;
    let conn = ssh::Conn::from_args(a, false)?;
    match cmd {
        Cmd::SftpLs => {
            let path = a.positional.get(1).cloned().unwrap_or_else(|| ".".into());
            let mut s = open(&target, &conn)?;
            let real = s.realpath(&path).map_err(|e| e.into_cli(&target))?;
            let attrs = s
                .stat(&real)
                .map_err(|e| e.into_cli(&target))?
                .ok_or_else(|| {
                    SErr::Status(FX_NO_SUCH_FILE, format!("`{path}` does not exist"))
                        .into_cli(&target)
                })?;
            let entries = if attrs.is_dir() {
                s.list(&real).map_err(|e| e.into_cli(&target))?
            } else {
                vec![Entry {
                    name: rbase(&real).to_string(),
                    attrs,
                }]
            };
            let mut h = String::new();
            for e in &entries {
                h.push_str(&format!(
                    "{:<5} {:>12}  {}\n",
                    e.attrs.kind(),
                    e.attrs.size.map_or(String::new(), |s| s.to_string()),
                    e.name
                ));
            }
            Ok((
                json!({"target": target.shown(), "path": real, "entries": entries.iter().map(entry_json).collect::<Vec<_>>()}),
                h,
            ))
        }
        Cmd::SftpMkdir | Cmd::SftpRm => {
            let path = remote_arg(a, 1, "remote path")?;
            let rm = cmd == Cmd::SftpRm;
            if rm && !a.yes && !a.dry_run {
                return Err(CliError::new(
                    Code::ConfirmationRequired,
                    format!("deleting `{path}` on {} needs --yes", target.shown()),
                ));
            }
            let mut s = open(&target, &conn)?;
            let attrs = s.lstat(&path).map_err(|e| e.into_cli(&target))?;
            let d = json!({"target": target.shown(), "path": path, "dry_run": a.dry_run});
            if rm {
                let a0 = attrs.ok_or_else(|| {
                    SErr::Status(FX_NO_SUCH_FILE, format!("`{path}` does not exist"))
                        .into_cli(&target)
                })?;
                if a.dry_run {
                    return Ok((d, format!("would delete `{path}`\n")));
                }
                if a0.is_dir() {
                    s.rmdir(&path)
                } else {
                    s.remove(&path)
                }
                .map_err(|e| e.into_cli(&target))?;
                Ok((d, format!("deleted `{path}`\n")))
            } else {
                if attrs.is_some() {
                    return Err(SErr::Exists(path).into_cli(&target));
                }
                if a.dry_run {
                    return Ok((d, format!("would create `{path}`\n")));
                }
                s.mkdir(&path, if a.private { 0o700 } else { 0o755 })
                    .map_err(|e| e.into_cli(&target))?;
                Ok((d, format!("created `{path}`\n")))
            }
        }
        _ => {
            let dir = if cmd == Cmd::SftpGet {
                Dir::Get
            } else {
                Dir::Put
            };
            let (remote, local) = match dir {
                Dir::Get => (remote_arg(a, 1, "remote path")?, local_arg(a, 2)?),
                Dir::Put => {
                    if a.positional.len() < 2 {
                        return Err(CliError::usage("give the local file or folder"));
                    }
                    let l = local_arg(a, 1)?;
                    let r = a.positional.get(2).cloned().unwrap_or_else(|| ".".into());
                    (r, l)
                }
            };
            if a.overwrite && a.resume {
                return Err(CliError::usage("give --overwrite or --resume, not both"));
            }
            let plan = Plan {
                dir,
                remote,
                local,
                recursive: a.recursive,
                overwrite: a.overwrite || a.resume,
                resume: a.resume,
                private: a.private,
            };
            let base = json!({
                "target": target.shown(),
                "direction": if dir == Dir::Get { "get" } else { "put" },
                "remote": plan.remote,
                "local": plan.local.display().to_string(),
                "recursive": plan.recursive,
                "overwrite": a.overwrite,
                "resume": plan.resume,
            });
            if a.dry_run {
                let mut d = base;
                d["dry_run"] = json!(true);
                return Ok((
                    d,
                    "would transfer (dry run: nothing was contacted)\n".into(),
                ));
            }
            if a.background {
                return background(a, base);
            }
            // The worker of a background job.
            if let Ok(id) = std::env::var("SUBAK_TRANSFER_JOB")
                && valid_job_id(&id)
                && let Some(jd) = jobs_dir()
            {
                let doc = read_job(&jd, &id).unwrap_or(base);
                let mut w = JobWatch {
                    dir: jd.clone(),
                    doc,
                    last: Instant::now() - Duration::from_secs(1),
                };
                w.doc["state"] = json!("running");
                w.doc["pid"] = json!(std::process::id());
                write_job(&jd, &w.doc);
                let r =
                    Sftp::connect(&target, &conn).and_then(|mut s| transfer(&mut s, &plan, &mut w));
                let mut doc = w.doc;
                doc["finished"] = json!(now_ms());
                match r {
                    Ok(sum) => {
                        doc["state"] = json!("done");
                        doc["result"] = sum;
                        if let Some(p) = doc.get_mut("progress") {
                            p["bytes_done"] = p["bytes_total"].clone();
                            p["files_done"] = p["files_total"].clone();
                        }
                    }
                    Err(SErr::Cancelled) => doc["state"] = json!("cancelled"),
                    Err(e) => {
                        let ce = e.into_cli(&target);
                        doc["state"] = json!("failed");
                        doc["error"] = json!({"code": ce.code.as_str(), "message": ce.message, "hint": ce.hint});
                    }
                }
                write_job(&jd, &doc);
                let _ = std::fs::remove_file(jd.join(format!("{id}.cancel")));
                return Ok((doc, String::new()));
            }
            let mut s = open(&target, &conn)?;
            let console =
                std::io::IsTerminal::is_terminal(&std::io::stderr()) && !a.json && !a.quiet;
            let r = if console {
                transfer(&mut s, &plan, &mut ConsoleWatch)
            } else {
                transfer(&mut s, &plan, &mut Quiet)
            };
            if console {
                eprintln!();
            }
            let sum = r.map_err(|e| e.into_cli(&target))?;
            let h = format!(
                "{} {} file(s), {} bytes in {} s\n",
                if dir == Dir::Get {
                    "downloaded"
                } else {
                    "uploaded"
                },
                sum["files"],
                sum["bytes"],
                sum["seconds"]
            );
            let mut d = base;
            d["result"] = sum;
            Ok((d, h))
        }
    }
}

fn background(a: &Args, base: Value) -> Result<(Value, String), CliError> {
    let _ = a;
    let jd = jobs_dir().ok_or_else(|| {
        CliError::new(
            Code::Io,
            "%APPDATA% is not set, so transfer jobs have no place",
        )
    })?;
    std::fs::create_dir_all(&jd)
        .map_err(|e| CliError::new(Code::Io, format!("cannot create `{}`: {e}", jd.display())))?;
    // Finished jobs older than a week are forgotten.
    let week = Duration::from_secs(7 * 86_400);
    for e in std::fs::read_dir(&jd).into_iter().flatten().flatten() {
        if e.metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|x| x > week))
        {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let id = format!("xfer-{:x}-{:x}", now_ms(), std::process::id());
    let mut doc = base;
    doc["id"] = json!(id);
    doc["state"] = json!("queued");
    doc["started"] = json!(now_ms());
    doc["progress"] = Progress::default().json();
    write_job(&jd, &doc);
    let pid = spawn_worker(&id)?;
    if let Some(mut v) = read_job(&jd, &id)
        && v["state"] == "queued"
    {
        v["pid"] = json!(pid);
        write_job(&jd, &v);
        doc = v;
    } else {
        doc["pid"] = json!(pid);
    }
    Ok((
        doc,
        format!("started transfer {id}; see `subak transfer status {id}`\n"),
    ))
}

fn jobs(cmd: Cmd, a: &Args) -> Result<(Value, String), CliError> {
    let jd = jobs_dir().ok_or_else(|| CliError::new(Code::Io, "%APPDATA% is not set"))?;
    if cmd == Cmd::TransferList {
        let mut all: Vec<Value> = std::fs::read_dir(&jd)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                let id = n.strip_suffix(".json")?;
                valid_job_id(id).then(|| read_job(&jd, id)).flatten()
            })
            .map(|v| checked(&jd, v))
            .collect();
        all.sort_by_key(|v| v["started"].as_u64().unwrap_or(0));
        let mut h = String::new();
        for v in &all {
            h.push_str(&format!(
                "{}  {:<11} {} {} {}\n",
                v["id"].as_str().unwrap_or("?"),
                v["state"].as_str().unwrap_or("?"),
                v["direction"].as_str().unwrap_or("?"),
                v["target"].as_str().unwrap_or("?"),
                v["remote"].as_str().unwrap_or("?")
            ));
        }
        if all.is_empty() {
            h.push_str("no transfers\n");
        }
        return Ok((json!({"transfers": all}), h));
    }
    let id = a.positional.first().cloned().unwrap_or_default();
    let not_found = || {
        CliError::new(Code::TargetNotFound, format!("no transfer `{id}`"))
            .hint("`subak transfer list` shows the transfers")
    };
    if !valid_job_id(&id) {
        return Err(not_found());
    }
    let v = checked(&jd, read_job(&jd, &id).ok_or_else(not_found)?);
    if cmd == Cmd::TransferStatus {
        let p = &v["progress"];
        let h = format!(
            "{id}: {} — {}/{} bytes, {}/{} files\n",
            v["state"].as_str().unwrap_or("?"),
            p["bytes_done"],
            p["bytes_total"],
            p["files_done"],
            p["files_total"]
        );
        return Ok((v, h));
    }
    // cancel
    let running = matches!(v["state"].as_str(), Some("queued" | "running"));
    let mut d = json!({"id": id, "state": v["state"], "dry_run": a.dry_run, "cancelled": false});
    if !running {
        return Ok((
            d,
            format!(
                "{id} is not running ({})\n",
                v["state"].as_str().unwrap_or("?")
            ),
        ));
    }
    if a.dry_run {
        return Ok((d, format!("would cancel {id}\n")));
    }
    std::fs::write(jd.join(format!("{id}.cancel")), b"")
        .map_err(|e| CliError::new(Code::Io, format!("cannot cancel: {e}")))?;
    // The worker stops between chunks and reports `cancelled`.
    let end = Instant::now() + Duration::from_secs(15);
    let mut state = v["state"].clone();
    while Instant::now() < end {
        if let Some(now) = read_job(&jd, &id).map(|x| checked(&jd, x)) {
            state = now["state"].clone();
            if !matches!(state.as_str(), Some("queued" | "running")) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    d["state"] = state.clone();
    d["cancelled"] = json!(state == "cancelled" || state == "interrupted");
    Ok((d, format!("{id}: {}\n", state.as_str().unwrap_or("?"))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn remote_names_that_could_escape_the_folder_are_unsafe() {
        for ok in ["a.txt", "report 2026.pdf", "한글.md", ".bashrc", "icon.txt"] {
            assert!(safe_name(ok), "{ok}");
        }
        for bad in [
            "",
            ".",
            "..",
            "../x",
            "..\\..\\evil",
            "a/b",
            "C:x",
            "nul",
            "CON",
            "com1.txt",
            "LPT9",
            "x.",
            "x ",
            "a\u{7}",
            "a|b",
        ] {
            assert!(!safe_name(bad), "{bad:?}");
        }
    }

    /// A tiny SFTP server: a directory `/d` whose listing is `names`; files have 3 bytes.
    fn fake_server(names: Vec<&'static str>) -> Sftp {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut listed = false;
            loop {
                let mut len = [0u8; 4];
                if s.read_exact(&mut len).is_err() {
                    return;
                }
                let mut b = vec![0u8; u32::from_be_bytes(len) as usize];
                s.read_exact(&mut b).unwrap();
                let t = b[0];
                let mut out = Vec::new();
                if t == INIT {
                    out.push(VERSION);
                    put_u32(&mut out, 3);
                } else {
                    let id = u32::from_be_bytes([b[1], b[2], b[3], b[4]]);
                    let mut r = Rd { b: &b[5..], i: 0 };
                    let path = r.string().unwrap_or_default();
                    match t {
                        STAT | LSTAT => {
                            out.push(ATTRS);
                            put_u32(&mut out, id);
                            put_u32(&mut out, ATTR_PERMISSIONS | ATTR_SIZE);
                            put_u64(&mut out, 3);
                            put_u32(
                                &mut out,
                                if path == "/d" {
                                    S_IFDIR | 0o755
                                } else {
                                    S_IFREG | 0o644
                                },
                            );
                        }
                        OPENDIR => {
                            out.push(HANDLE);
                            put_u32(&mut out, id);
                            put_str(&mut out, b"h");
                        }
                        READDIR if !listed => {
                            listed = true;
                            out.push(NAME);
                            put_u32(&mut out, id);
                            put_u32(&mut out, u32::try_from(names.len()).unwrap());
                            for n in &names {
                                put_str(&mut out, n.as_bytes());
                                put_str(&mut out, n.as_bytes());
                                put_u32(&mut out, ATTR_PERMISSIONS | ATTR_SIZE);
                                put_u64(&mut out, 3);
                                put_u32(&mut out, S_IFREG | 0o644);
                            }
                        }
                        _ => {
                            out.push(STATUS);
                            put_u32(&mut out, id);
                            put_u32(&mut out, if t == READDIR { FX_EOF } else { 0 });
                            put_str(&mut out, b"");
                            put_str(&mut out, b"");
                        }
                    }
                }
                let mut p = Vec::new();
                put_str(&mut p, &out);
                s.write_all(&p).unwrap();
            }
        });
        let c = TcpStream::connect(addr).unwrap();
        Sftp::over(
            Box::new(c.try_clone().unwrap()),
            Box::new(c),
            None,
            Arc::new(Mutex::new(String::new())),
        )
        .unwrap()
    }

    #[test]
    fn a_hostile_listing_is_refused_before_anything_is_written() {
        let dir = std::env::temp_dir().join(format!("subak-sftp-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for evil in ["..\\..\\evil.txt", "...", "C:evil", "NUL", "sub/evil"] {
            let mut s = fake_server(vec!["fine.txt", evil]);
            let p = Plan {
                dir: Dir::Get,
                remote: "/d".into(),
                local: dir.clone(),
                recursive: true,
                overwrite: false,
                resume: false,
                private: false,
            };
            let e = transfer(&mut s, &p, &mut Quiet).unwrap_err();
            assert!(matches!(e, SErr::Unsafe(_)), "{evil}: {e:?}");
            assert_eq!(
                std::fs::read_dir(&dir).unwrap().count(),
                0,
                "{evil}: nothing was written"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
