//! `cargo xtask package <kim|subak|noeul|ddalgak> [--skip-build] [--signed]` (Step 40, docs/RELEASING.md): the Windows
//! distributable of one tool, built from this checkout.
//!
//! - Builds the release binaries (`cargo build --release -p <crates>`), unless `--skip-build`.
//! - Writes `target/dist/<tool>-<version>-windows-x86_64.zip`: the programs, the tool's
//!   `capabilities.json` (from `<tool> capabilities --json`, so Kim can read it without starting
//!   the tool), `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD-PARTY-NOTICES.txt` (every crate linked
//!   in, with its license, from `cargo tree`) and `README.txt`.
//! - The zip is **deterministic**: sorted entries, fixed timestamps (1980-01-01) and
//!   permissions, so the same binaries always give the same bytes (checked by packing twice).
//! - Writes `target/dist/<tool>-<version>-release.json`: size, SHA-256, entrypoint, the build
//!   (commit, rustc, target) and the `release` entry to put into `catalog/catalog.json` once the
//!   zip is uploaded. Nothing is uploaded, signed or published here.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Tool {
    id: &'static str,
    crates: &'static [&'static str],
    exes: &'static [&'static str],
    entrypoint: &'static str,
    summary: &'static str,
}

const TOOLS: [Tool; 4] = [
    Tool {
        id: "kim",
        crates: &["kim", "kim-manager", "kim-setup"],
        exes: &["kim.exe", "kim-manager.exe"],
        entrypoint: "kim.exe",
        summary: "Kim: the explicit catalog and installer for Kim tools (kim.exe CLI, kim-manager.exe window)",
    },
    Tool {
        id: "subak",
        crates: &["subak"],
        exes: &["subak.exe", "subak-gui.exe"],
        entrypoint: "subak.exe",
        summary: "Subak: a persistent Windows terminal with a detached Host (subak.exe CLI and Host, subak-gui.exe window)",
    },
    // Step 62: Noeul is packaged like the others so Kim can install it (one program: CLI,
    // resident instance, MCP adapter and interop provider).
    Tool {
        id: "noeul",
        crates: &["noeul"],
        exes: &["noeul.exe"],
        entrypoint: "noeul.exe",
        summary: "Noeul: screenshots and floating pins (noeul.exe: the resident app, its CLI, the MCP adapter and the Kim interop provider)",
    },
    // Step 81: Ddalgak, so Kim can install it (the headless CLI with its MCP adapter and interop
    // provider, the tray app that owns the engine, and the rule editor).
    Tool {
        id: "ddalgak",
        crates: &["ddalgak", "ddalgak-editor"],
        exes: &["ddalgak.exe", "ddalgak-gui.exe", "ddalgak-editor.exe"],
        entrypoint: "ddalgak.exe",
        summary: "Ddalgak: key remapping and automation (ddalgak.exe: the CLI, MCP adapter and Kim interop provider; ddalgak-gui.exe: the tray app that owns the engine; ddalgak-editor.exe: the rule editor)",
    },
];

fn out(cmd: &mut Command) -> Result<String, String> {
    let o = cmd.output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("{cmd:?}: {}", String::from_utf8_lossy(&o.stderr)));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// `name version license` of every crate linked into `crates` (normal dependencies), sorted.
fn notices(root: &Path, crates: &[&str]) -> Result<String, String> {
    let mut lines = std::collections::BTreeSet::new();
    for c in crates {
        let t = out(
            Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
                .args([
                    "tree", "-p", c, "-e", "normal", "--prefix", "none", "--format", "{p}\t{l}",
                ])
                .current_dir(root),
        )?;
        for l in t.lines() {
            let (p, lic) = l.split_once('\t').unwrap_or((l, ""));
            // `cargo tree` marks repeated crates with ` (*)`.
            let lic = lic.trim_end_matches(" (*)").trim();
            // `name v1.2.3 (path)` → `name 1.2.3`; workspace crates are ours.
            let mut w = p.split_whitespace();
            let (Some(name), Some(ver)) = (w.next(), w.next()) else {
                continue;
            };
            if p.contains("(C:") || p.contains("(/") || p.contains("(\\\\") {
                continue;
            }
            lines.insert(format!(
                "{name} {} — {}",
                ver.trim_start_matches('v'),
                if lic.is_empty() { "see the crate" } else { lic }
            ));
        }
    }
    let mut s = String::from(
        "Third-party software included in this package\n\
         ==============================================\n\n\
         Each crate below is linked into the programs of this package and is distributed under\n\
         the license named next to it. Their license texts are published with each crate on\n\
         https://crates.io and in its source repository.\n\n",
    );
    for l in lines {
        s.push_str(&l);
        s.push('\n');
    }
    Ok(s)
}

/// A deterministic zip of `files` (archive name, bytes).
fn zip_bytes(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(0o644);
        for (name, data) in sorted {
            z.start_file(name.as_str(), opts)
                .map_err(|e| e.to_string())?;
            z.write_all(data).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let id = args.first().map(String::as_str).unwrap_or("");
    let Some(tool) = TOOLS.iter().find(|t| t.id == id) else {
        return Err("usage: cargo xtask package <kim|subak|noeul|ddalgak> [--skip-build]".into());
    };
    if !cfg!(windows) {
        return Err("package builds the Windows distributable; run it on Windows".into());
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    if !args.iter().any(|a| a == "--skip-build") {
        let mut c = Command::new(&cargo);
        c.args(["build", "--release"]).current_dir(root);
        for k in tool.crates {
            c.args(["-p", k]);
        }
        let st = c.status().map_err(|e| e.to_string())?;
        if !st.success() {
            return Err("cargo build --release failed".into());
        }
    }
    let rel = root.join("target").join("release");
    let entry = rel.join(tool.entrypoint);
    // `--signed` (the release workflow, after signtool): every program must carry a valid
    // Authenticode signature, checked here, or the package is refused.
    let signed = args.iter().any(|a| a == "--signed");
    if signed {
        for exe in tool.exes {
            let p = rel.join(exe);
            let status = out(Command::new("powershell").args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "(Get-AuthenticodeSignature -LiteralPath '{}').Status",
                    p.display()
                ),
            ]))?;
            if status.trim() != "Valid" {
                return Err(format!(
                    "{exe} has no valid Authenticode signature ({})",
                    status.trim()
                ));
            }
        }
    }
    let caps: Value =
        serde_json::from_str(&out(Command::new(&entry).args(["capabilities", "--json"]))?)
            .map_err(|e| format!("capabilities: {e}"))?;
    let version = caps["data"]["version"]
        .as_str()
        .ok_or("no version in capabilities")?
        .to_string();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for exe in tool.exes {
        let p = rel.join(exe);
        files.push((
            (*exe).to_string(),
            std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?,
        ));
    }
    files.push((
        "capabilities.json".into(),
        serde_json::to_vec_pretty(&caps["data"]).unwrap_or_default(),
    ));
    for l in ["LICENSE-MIT", "LICENSE-APACHE"] {
        files.push((
            l.into(),
            std::fs::read(root.join(l)).map_err(|e| format!("{l}: {e}"))?,
        ));
    }
    files.push((
        "THIRD-PARTY-NOTICES.txt".into(),
        notices(root, tool.crates)?.into_bytes(),
    ));
    let commit = out(Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root))?
    .trim()
    .to_string();
    let rustc = out(Command::new("rustc").arg("-V"))?.trim().to_string();
    let target = out(Command::new("rustc").arg("-vV"))?
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(String::from))
        .unwrap_or_default();
    files.push((
        "README.txt".into(),
        format!(
            "{summary}\r\nVersion {version}, built from commit {commit} with {rustc} ({target}).\r\n\r\n\
             Install with Kim: `kim install {id}` (from the signed catalog). This zip is also usable\r\n\
             directly: unpack it anywhere and run {entry}. Nothing is installed, registered or\r\n\
             updated by itself; see https://github.com/hellomouv/kim/blob/main/docs/INSTALL.md.\r\n\r\n\
             License: MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE); third-party notices in\r\n\
             THIRD-PARTY-NOTICES.txt.\r\n",
            summary = tool.summary,
            entry = tool.entrypoint,
        )
        .into_bytes(),
    ));
    let zipped = zip_bytes(&files)?;
    // Deterministic: the same inputs give the same bytes.
    if zip_bytes(&files)? != zipped {
        return Err("packing twice gave different bytes".into());
    }
    let dist = root.join("target").join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    let name = format!("{id}-{version}-windows-x86_64.zip");
    let zpath: PathBuf = dist.join(&name);
    std::fs::write(&zpath, &zipped).map_err(|e| e.to_string())?;
    let sha = sha_hex(&zipped);
    let manifest = json!({
        "tool": id,
        "version": version,
        "asset": {"file": name, "size": zipped.len(), "sha256": sha, "entrypoint": tool.entrypoint, "os": "windows", "arch": "x86_64", "kind": "zip"},
        "build": {"commit": commit, "rustc": rustc, "target": target, "profile": "release", "deterministic_zip": true},
        "files": files.iter().map(|(n, b)| json!({"name": n, "size": b.len(), "sha256": sha_hex(b)})).collect::<Vec<_>>(),
        "signed": signed,
        "catalog_release": {
            "version": version, "channel": "stable", "published_at": "<set when published>",
            "release_notes": format!("https://github.com/hellomouv/kim/releases/tag/{id}-v{version}"),
            "assets": [{"os": "windows", "arch": "x86_64", "kind": "zip", "entrypoint": tool.entrypoint,
                        "url": format!("https://github.com/hellomouv/kim/releases/download/{id}-v{version}/{name}"),
                        "size": zipped.len(), "sha256": sha}],
        },
    });
    // Kim: also the one-step installer (Step 43, ADR 0072) — kim-setup.exe with this zip
    // appended (footer: magic, length, SHA-256 checked before anything is unpacked).
    let mut manifest = manifest;
    if id == "kim" {
        let stub =
            std::fs::read(rel.join("kim-setup.exe")).map_err(|e| format!("kim-setup.exe: {e}"))?;
        let mut with_payload = Vec::with_capacity(stub.len() + zipped.len() + 48);
        with_payload.extend_from_slice(&stub);
        with_payload.extend_from_slice(&zipped);
        with_payload.extend_from_slice(b"KIMSETUP");
        with_payload.extend_from_slice(&(zipped.len() as u64).to_le_bytes());
        with_payload.extend_from_slice(&Sha256::digest(&zipped));
        let sname = format!("Kim-Setup-{version}-x86_64.exe");
        std::fs::write(dist.join(&sname), &with_payload).map_err(|e| e.to_string())?;
        manifest["setup"] = json!({"file": sname, "size": with_payload.len(), "sha256": sha_hex(&with_payload),
            "note": "signing Kim-Setup (after this file is written) changes its SHA-256; publish the signed file's"});
        println!(
            "package: {} ({} bytes)",
            dist.join(&sname).display(),
            with_payload.len()
        );
    }
    let mpath = dist.join(format!("{id}-{version}-release.json"));
    std::fs::write(
        &mpath,
        serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
    )
    .map_err(|e| e.to_string())?;
    println!(
        "package: {} ({} bytes, sha256 {sha})",
        zpath.display(),
        zipped.len()
    );
    println!(
        "package: {} ({})",
        mpath.display(),
        if signed {
            "Authenticode signatures verified"
        } else {
            "unsigned; Authenticode only with a real certificate, docs/RELEASING.md"
        }
    );
    Ok(())
}
