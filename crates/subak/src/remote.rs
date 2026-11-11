//! Step 36 (ADR 0069): Telnet and serial panes.
//!
//! A Telnet or serial Terminal is an ordinary pane whose program is `subak.exe` itself in a
//! bridge mode (`__telnet`, `__serial`): it puts its pseudo console in raw VT mode and relays
//! bytes between the pane and the socket or port. The Host runs nothing new and never blocks
//! on a device; when the connection ends the pane keeps its last screen.
//!
//! - **Telnet is off by default.** Everything, passwords included, travels unencrypted, so
//!   `telnet connect` needs `--allow-insecure` and always warns. Option negotiation accepts
//!   only what a terminal needs (echo, suppress-go-ahead, terminal type, window size, binary)
//!   and refuses the rest.
//! - **Serial** ports are opened with the line settings given (baud, data bits, parity, stop
//!   bits, RTS/CTS or XON/XOFF), with newline conversion and optional local echo. Reads wait at
//!   most 50 ms; no loop spins while nothing arrives.

use crate::cli::{Args, EnvArg};
use crate::output::{CliError, Code};
use serde_json::{Value, json};

pub const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const OPT_BINARY: u8 = 0;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_TTYPE: u8 = 24;
const OPT_NAWS: u8 = 31;
const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;

/// The Telnet protocol state of one connection (RFC 854/855, 1073, 1091).
#[derive(Debug, Default)]
pub struct Telnet {
    state: St,
    sb: Vec<u8>,
    /// The server agreed to receive window sizes.
    pub naws: bool,
    pub size: (u16, u16),
    /// Options negotiated, for the audit line (`DO ECHO` …).
    pub log: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum St {
    #[default]
    Data,
    Iac,
    Verb(u8),
    Sb,
    SbIac,
}

fn opt_name(o: u8) -> String {
    match o {
        OPT_BINARY => "BINARY".into(),
        OPT_ECHO => "ECHO".into(),
        OPT_SGA => "SGA".into(),
        OPT_TTYPE => "TTYPE".into(),
        OPT_NAWS => "NAWS".into(),
        _ => format!("option {o}"),
    }
}

impl Telnet {
    pub fn new(size: (u16, u16)) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    /// The NAWS subnegotiation for `self.size`.
    pub fn naws_bytes(&self) -> Vec<u8> {
        let mut v = vec![IAC, SB, OPT_NAWS];
        for n in [self.size.0, self.size.1] {
            for b in n.to_be_bytes() {
                v.push(b);
                if b == IAC {
                    v.push(IAC);
                }
            }
        }
        v.extend([IAC, SE]);
        v
    }

    /// Splits bytes from the server into data for the screen and the replies to send back.
    pub fn feed(&mut self, input: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let (mut data, mut reply) = (Vec::new(), Vec::new());
        for &b in input {
            self.state = match self.state {
                St::Data if b == IAC => St::Iac,
                St::Data => {
                    data.push(b);
                    St::Data
                }
                St::Iac => match b {
                    IAC => {
                        data.push(IAC);
                        St::Data
                    }
                    DO | DONT | WILL | WONT => St::Verb(b),
                    SB => {
                        self.sb.clear();
                        St::Sb
                    }
                    _ => St::Data, // NOP, GA, and other commands: nothing to show
                },
                St::Verb(v) => {
                    self.answer(v, b, &mut reply);
                    St::Data
                }
                St::Sb if b == IAC => St::SbIac,
                St::Sb => {
                    if self.sb.len() < 1024 {
                        self.sb.push(b);
                    }
                    St::Sb
                }
                St::SbIac if b == SE => {
                    self.subneg(&mut reply);
                    St::Data
                }
                St::SbIac => {
                    if b == IAC && self.sb.len() < 1024 {
                        self.sb.push(IAC);
                    }
                    St::Sb
                }
            };
        }
        (data, reply)
    }

    fn answer(&mut self, verb: u8, opt: u8, reply: &mut Vec<u8>) {
        let (v, name) = match verb {
            // The server offers to do something: welcome echo, SGA and binary, refuse the rest.
            WILL => (
                if matches!(opt, OPT_ECHO | OPT_SGA | OPT_BINARY) {
                    DO
                } else {
                    DONT
                },
                "WILL",
            ),
            // The server asks us to do something: terminal type, window size, SGA, binary.
            DO => {
                let ok = matches!(opt, OPT_TTYPE | OPT_NAWS | OPT_SGA | OPT_BINARY);
                if opt == OPT_NAWS && ok {
                    self.naws = true;
                }
                (if ok { WILL } else { WONT }, "DO")
            }
            WONT => (DONT, "WONT"),
            _ => (WONT, "DONT"),
        };
        self.log.push(format!("{name} {}", opt_name(opt)));
        reply.extend([IAC, v, opt]);
        if v == WILL && opt == OPT_NAWS {
            reply.extend(self.naws_bytes());
        }
    }

    fn subneg(&mut self, reply: &mut Vec<u8>) {
        if self.sb.first() == Some(&OPT_TTYPE) && self.sb.get(1) == Some(&TTYPE_SEND) {
            reply.extend([IAC, SB, OPT_TTYPE, TTYPE_IS]);
            reply.extend_from_slice(b"XTERM-256COLOR");
            reply.extend([IAC, SE]);
        }
    }

    /// Bytes the user typed, ready for the wire: `IAC` doubled, Enter as CR LF.
    pub fn outgoing(input: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(input.len() + 4);
        for &b in input {
            match b {
                IAC => v.extend([IAC, IAC]),
                b'\r' => v.extend(b"\r\n"),
                _ => v.push(b),
            }
        }
        v
    }
}

/// What Enter sends on a serial line, and how received line ends are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Newline {
    Cr,
    Lf,
    CrLf,
}

impl Newline {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "cr" => Self::Cr,
            "lf" => Self::Lf,
            "crlf" => Self::CrLf,
            _ => return None,
        })
    }
    fn word(self) -> &'static str {
        match self {
            Self::Cr => "cr",
            Self::Lf => "lf",
            Self::CrLf => "crlf",
        }
    }
}

/// Serial: what Enter becomes (the terminal sends CR).
pub fn serial_out(input: &[u8], nl: Newline) -> Vec<u8> {
    let mut v = Vec::with_capacity(input.len());
    for &b in input {
        if b == b'\r' {
            match nl {
                Newline::Cr => v.push(b'\r'),
                Newline::Lf => v.push(b'\n'),
                Newline::CrLf => v.extend(b"\r\n"),
            }
        } else {
            v.push(b);
        }
    }
    v
}

/// Serial: a bare LF from the device starts a new line on screen (CR LF), as most devices
/// expect of a terminal; `last_cr` carries a CR across reads.
pub fn serial_in(input: &[u8], last_cr: &mut bool) -> Vec<u8> {
    let mut v = Vec::with_capacity(input.len() + 8);
    for &b in input {
        if b == b'\n' && !*last_cr {
            v.push(b'\r');
        }
        v.push(b);
        *last_cr = b == b'\r';
    }
    v
}

/// The settings of `subak serial connect`, in the bridge's argument order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialPlan {
    pub port: String,
    pub baud: u32,
    pub data_bits: u8,
    pub parity: String,
    pub stop_bits: String,
    pub flow: String,
    pub newline: Newline,
    pub local_echo: bool,
}

impl SerialPlan {
    pub fn from_args(a: &Args) -> Result<Self, CliError> {
        let port = a
            .positional
            .first()
            .cloned()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if !valid_port(&port) {
            return Err(
                CliError::usage(format!("`{port}` is not a serial port (COM1…COM256)"))
                    .hint("`subak serial list` shows the ports Windows knows"),
            );
        }
        let s = &a.serial;
        let pick =
            |v: &Option<String>, d: &str, ok: &[&str], what: &str| -> Result<String, CliError> {
                let v = v.clone().unwrap_or_else(|| d.to_string());
                if ok.contains(&v.as_str()) {
                    Ok(v)
                } else {
                    Err(CliError::usage(format!(
                        "--{what} must be one of {}",
                        ok.join(", ")
                    )))
                }
            };
        let baud = s.baud.unwrap_or(9600);
        if !(50..=4_000_000).contains(&baud) {
            return Err(CliError::usage("--baud must be 50 to 4000000"));
        }
        let data_bits = s.data_bits.unwrap_or(8);
        if !(5..=8).contains(&data_bits) {
            return Err(CliError::usage("--data-bits must be 5 to 8"));
        }
        Ok(Self {
            port,
            baud,
            data_bits,
            parity: pick(
                &s.parity,
                "none",
                &["none", "odd", "even", "mark", "space"],
                "parity",
            )?,
            stop_bits: pick(&s.stop_bits, "1", &["1", "1.5", "2"], "stop-bits")?,
            flow: pick(&s.flow, "none", &["none", "rtscts", "xonxoff"], "flow")?,
            newline: s
                .newline
                .as_deref()
                .map_or(Some(Newline::Cr), Newline::parse)
                .ok_or_else(|| CliError::usage("--newline must be cr, lf or crlf"))?,
            local_echo: s.local_echo,
        })
    }

    fn bridge_args(&self) -> Vec<String> {
        vec![
            "__serial".into(),
            self.port.clone(),
            self.baud.to_string(),
            self.data_bits.to_string(),
            self.parity.clone(),
            self.stop_bits.clone(),
            self.flow.clone(),
            self.newline.word().into(),
            if self.local_echo { "echo" } else { "noecho" }.into(),
        ]
    }

    fn json(&self) -> Value {
        json!({
            "port": self.port, "baud": self.baud, "data_bits": self.data_bits,
            "parity": self.parity, "stop_bits": self.stop_bits, "flow": self.flow,
            "newline": self.newline.word(), "local_echo": self.local_echo,
        })
    }
}

pub fn valid_port(p: &str) -> bool {
    p.len() > 3
        && p[..3].eq_ignore_ascii_case("COM")
        && p[3..].parse::<u32>().is_ok_and(|n| (1..=256).contains(&n))
}

/// `a` as the `new` of a pane running `subak.exe <bridge…>` (remain-on-exit).
fn bridge_pane(a: &Args, bridge: Vec<String>, name: Option<String>) -> Result<Args, CliError> {
    let me = std::env::current_exe()
        .map_err(|e| CliError::new(Code::SpawnFailed, format!("cannot find subak.exe: {e}")))?;
    let mut eff = a.clone();
    eff.command = std::iter::once(me.display().to_string())
        .chain(bridge)
        .collect();
    eff.command_line = None;
    eff.no_profile = true;
    eff.profile = None;
    eff.remain_on_exit = true;
    eff.positional = name.into_iter().collect();
    eff.env.push(EnvArg::Set("SUBAK_BRIDGE".into(), "1".into()));
    Ok(eff)
}

/// `subak telnet connect <host[:port]> [<name>] --allow-insecure`.
pub fn telnet_connect(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let t = crate::ssh::Target::parse(a.positional.first().map_or("", String::as_str))
        .map_err(|m| CliError::usage(m).hint("give the destination as host[:port]"))?;
    if t.user.is_some() {
        return Err(CliError::usage(
            "Telnet has no user in the address; the server asks for it",
        ));
    }
    let port = if a.positional.first().is_some_and(|p| p.contains(':')) {
        t.port
    } else {
        23
    };
    let warning = format!(
        "Telnet is unencrypted: everything you type on {}:{port}, passwords included, can be read on the network; prefer `subak ssh connect`",
        t.host
    );
    if !a.allow_insecure {
        return Err(CliError::new(
            Code::InvalidState,
            format!("Telnet is off by default. {warning}"),
        )
        .hint("pass --allow-insecure if this device only speaks Telnet"));
    }
    crate::warn_later(warning.clone());
    let eff = bridge_pane(
        a,
        vec!["__telnet".into(), t.host.clone(), port.to_string()],
        a.positional.get(1).cloned(),
    )?;
    let mut out = crate::terminal::new(&eff, endpoint)?;
    out["telnet"] = json!({"host": t.host, "port": port, "warnings": [warning]});
    Ok(out)
}

/// `subak serial connect <COMn> [<name>] [--baud …] …`.
pub fn serial_connect(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let p = SerialPlan::from_args(a)?;
    let eff = bridge_pane(a, p.bridge_args(), a.positional.get(1).cloned())?;
    let mut out = crate::terminal::new(&eff, endpoint)?;
    out["serial"] = p.json();
    Ok(out)
}

/// `subak serial list` (no Host; read only).
pub fn serial_list() -> (Value, String) {
    #[cfg(windows)]
    let ports = crate::win::serial::ports();
    #[cfg(not(windows))]
    let ports: Vec<(String, String)> = Vec::new();
    let mut h = String::new();
    for (d, n) in &ports {
        h.push_str(&format!("{d}  {n}\n"));
    }
    if ports.is_empty() {
        h.push_str("no serial ports\n");
    }
    (
        json!({"ports": ports.iter().map(|(d, n)| json!({"port": d, "driver": n})).collect::<Vec<_>>()}),
        h,
    )
}

// ---------------------------------------------------------------------------------------------
// The bridges (the pane's program).

/// `subak.exe __telnet <host> <port>` and `__serial <port> <baud> …`: runs until the
/// connection ends.
pub fn bridge_main(raw: &[String]) -> std::process::ExitCode {
    #[cfg(windows)]
    {
        let r = match raw.first().map(String::as_str) {
            Some("__telnet") => telnet_bridge(&raw[1..]),
            Some("__serial") => serial_bridge(&raw[1..]),
            _ => Err("unknown bridge".to_string()),
        };
        match r {
            Ok(m) => {
                print!("\r\n[subak: {m}]\r\n");
                std::process::ExitCode::SUCCESS
            }
            Err(m) => {
                print!("\r\n[subak: {m}]\r\n");
                std::process::ExitCode::from(1)
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = raw;
        std::process::ExitCode::from(1)
    }
}

#[cfg(windows)]
fn out(bytes: &[u8]) {
    use std::io::Write;
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(bytes);
    let _ = o.flush();
}

/// The keyboard side of a bridge: until `done`, keys and resizes from the pane.
#[cfg(windows)]
fn keys(
    done: &std::sync::atomic::AtomicBool,
    mut on_key: impl FnMut(&[u8]) -> bool,
    mut on_resize: impl FnMut(u16, u16),
) {
    use crate::win::console::{Input, read_input, window_size};
    use std::sync::atomic::Ordering;
    let mut size = window_size().unwrap_or((80, 24));
    let mut high = None;
    while !done.load(Ordering::SeqCst) {
        match read_input(
            &mut size,
            &mut high,
            std::time::Duration::from_millis(100),
            false,
        ) {
            Ok(events) => {
                for e in events {
                    match e {
                        Input::Bytes(b) => {
                            if !on_key(&b) {
                                return;
                            }
                        }
                        Input::Resize(c, r) => on_resize(c, r),
                    }
                }
            }
            Err(_) => return,
        }
    }
}

#[cfg(windows)]
fn telnet_bridge(a: &[String]) -> Result<String, String> {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let [host, port] = a else {
        return Err("usage: __telnet <host> <port>".into());
    };
    let _raw = crate::win::console::RawMode::enable();
    out(format!("[subak: Telnet to {host}:{port} — unencrypted]\r\n").as_bytes());
    let addr = format!("{host}:{port}");
    let sock = std::net::TcpStream::connect(&addr)
        .map_err(|e| format!("cannot connect to {addr}: {e}"))?;
    let _ = sock.set_nodelay(true);
    let size = crate::win::console::window_size().unwrap_or((80, 24));
    let tn = Arc::new(Mutex::new(Telnet::new(size)));
    let done = Arc::new(AtomicBool::new(false));
    let mut rx = sock.try_clone().map_err(|e| e.to_string())?;
    let tx = Arc::new(Mutex::new(sock));
    let (tn2, tx2, done2) = (tn.clone(), tx.clone(), done.clone());
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match rx.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let (data, reply) = tn2
                        .lock()
                        .map(|mut t| t.feed(&buf[..n]))
                        .unwrap_or_default();
                    if !reply.is_empty()
                        && let Ok(mut s) = tx2.lock()
                    {
                        let _ = s.write_all(&reply);
                    }
                    out(&data);
                }
            }
        }
        done2.store(true, Ordering::SeqCst);
    });
    keys(
        &done,
        |k| {
            tx.lock()
                .is_ok_and(|mut s| s.write_all(&Telnet::outgoing(k)).is_ok())
        },
        |c, r| {
            if let Ok(mut t) = tn.lock() {
                t.size = (c, r);
                if t.naws
                    && let Ok(mut s) = tx.lock()
                {
                    let _ = s.write_all(&t.naws_bytes());
                }
            }
        },
    );
    if let Ok(s) = tx.lock() {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    let _ = reader.join();
    Ok(format!("the Telnet connection to {addr} ended"))
}

#[cfg(windows)]
fn serial_bridge(a: &[String]) -> Result<String, String> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let [port, baud, bits, parity, stop, flow, nl, echo] = a else {
        return Err(
            "usage: __serial <port> <baud> <bits> <parity> <stop> <flow> <newline> <echo>".into(),
        );
    };
    let line = crate::win::serial::Line {
        baud: baud.parse().map_err(|_| "bad baud")?,
        data_bits: bits.parse().map_err(|_| "bad data bits")?,
        parity: match parity.as_str() {
            "odd" => 1,
            "even" => 2,
            "mark" => 3,
            "space" => 4,
            _ => 0,
        },
        stop_bits: match stop.as_str() {
            "1.5" => 1,
            "2" => 2,
            _ => 0,
        },
        rtscts: flow == "rtscts",
        xonxoff: flow == "xonxoff",
    };
    let nl = Newline::parse(nl).unwrap_or(Newline::Cr);
    let echo = echo == "echo";
    let _raw = crate::win::console::RawMode::enable();
    let dev = Arc::new(
        crate::win::serial::Port::open(port, &line)
            .map_err(|e| format!("cannot open {port}: {e}"))?,
    );
    out(format!(
        "[subak: {port} at {baud} baud, {bits}{}{}, flow {flow}]\r\n",
        parity.chars().next().unwrap_or('n').to_ascii_uppercase(),
        stop
    )
    .as_bytes());
    let done = Arc::new(AtomicBool::new(false));
    let (dev2, done2) = (dev.clone(), done.clone());
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut last_cr = false;
        while !done2.load(Ordering::SeqCst) {
            match dev2.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => out(&serial_in(&buf[..n], &mut last_cr)),
                Err(_) => break,
            }
        }
        done2.store(true, Ordering::SeqCst);
    });
    keys(
        &done,
        |k| {
            if echo {
                let mut last = false;
                out(&serial_in(&serial_out(k, Newline::CrLf), &mut last));
            }
            dev.write_all(&serial_out(k, nl)).is_ok()
        },
        |_, _| {},
    );
    done.store(true, Ordering::SeqCst);
    let _ = reader.join();
    Ok(format!("{port} was closed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telnet_negotiates_only_what_a_terminal_needs() {
        let mut t = Telnet::new((120, 40));
        let input = [
            b"hi".as_slice(),
            &[
                IAC, DO, OPT_TTYPE, IAC, DO, OPT_NAWS, IAC, WILL, OPT_ECHO, IAC, DO, 39,
            ],
            &[IAC, SB, OPT_TTYPE, TTYPE_SEND, IAC, SE],
            &[IAC, IAC, b'!'],
        ]
        .concat();
        let (data, reply) = t.feed(&input);
        assert_eq!(data, [b'h', b'i', IAC, b'!']);
        let mut want = vec![IAC, WILL, OPT_TTYPE, IAC, WILL, OPT_NAWS];
        want.extend([IAC, SB, OPT_NAWS, 0, 120, 0, 40, IAC, SE]);
        want.extend([IAC, DO, OPT_ECHO, IAC, WONT, 39]);
        want.extend([IAC, SB, OPT_TTYPE, TTYPE_IS]);
        want.extend(b"XTERM-256COLOR");
        want.extend([IAC, SE]);
        assert_eq!(reply, want);
        assert!(t.naws);
        // An unknown offer is refused; split sequences work across reads.
        let (d, r) = t.feed(&[IAC]);
        assert!(d.is_empty() && r.is_empty());
        let (_, r) = t.feed(&[WILL, 99]);
        assert_eq!(r, [IAC, DONT, 99]);
        assert_eq!(
            Telnet::outgoing(&[b'a', IAC, b'\r']),
            [b'a', IAC, IAC, b'\r', b'\n']
        );
        // A window size byte of 255 is doubled.
        let t = Telnet::new((255, 24));
        assert_eq!(
            t.naws_bytes(),
            [IAC, SB, OPT_NAWS, 0, IAC, IAC, 0, 24, IAC, SE]
        );
    }

    #[test]
    fn serial_newlines_convert_both_ways() {
        assert_eq!(serial_out(b"ab\r", Newline::Cr), b"ab\r");
        assert_eq!(serial_out(b"ab\r", Newline::Lf), b"ab\n");
        assert_eq!(serial_out(b"ab\r", Newline::CrLf), b"ab\r\n");
        let mut cr = false;
        assert_eq!(serial_in(b"x\ny\r\nz\r", &mut cr), b"x\r\ny\r\nz\r");
        assert!(cr);
        assert_eq!(
            serial_in(b"\nw", &mut cr),
            b"\nw",
            "a CR LF split across reads"
        );
    }

    #[test]
    fn serial_settings_are_checked() {
        let mut a = Args {
            positional: vec!["com3".into()],
            ..Args::default()
        };
        let p = SerialPlan::from_args(&a).unwrap();
        assert_eq!((p.port.as_str(), p.baud, p.data_bits), ("COM3", 9600, 8));
        assert_eq!(p.bridge_args()[0], "__serial");
        a.serial.baud = Some(115_200);
        a.serial.parity = Some("even".into());
        a.serial.flow = Some("rtscts".into());
        assert_eq!(SerialPlan::from_args(&a).unwrap().parity, "even");
        for bad in [
            ("port", "LPT1"),
            ("baud", "10"),
            ("parity", "x"),
            ("flow", "dtr"),
            ("bits", "9"),
        ] {
            let mut b = a.clone();
            match bad.0 {
                "port" => b.positional = vec![bad.1.into()],
                "baud" => b.serial.baud = Some(10),
                "parity" => b.serial.parity = Some(bad.1.into()),
                "flow" => b.serial.flow = Some(bad.1.into()),
                _ => b.serial.data_bits = Some(9),
            }
            assert!(SerialPlan::from_args(&b).is_err(), "{bad:?}");
        }
        #[cfg(windows)]
        {
            use crate::win::serial::{Line, dcb_bits};
            let l = Line {
                baud: 9600,
                data_bits: 8,
                parity: 0,
                stop_bits: 0,
                rtscts: false,
                xonxoff: false,
            };
            assert_eq!(dcb_bits(&l), 1 | (1 << 4) | (1 << 12));
            let l = Line {
                parity: 2,
                rtscts: true,
                xonxoff: true,
                ..l
            };
            assert_eq!(
                dcb_bits(&l),
                1 | 2 | 4 | (1 << 4) | (1 << 8) | (1 << 9) | (2 << 12)
            );
        }
    }
}
