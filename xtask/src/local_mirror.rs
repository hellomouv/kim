//! `cargo xtask local-mirror --key <file> [--out <dir>] [--skip-build]` (ADR 0089): a local
//! mirror of the Kim tools built from this checkout, so Kim Manager can install and open them
//! on this PC without a published release and without the network.
//!
//! - Builds the release programs once, then `cargo xtask package <tool> --skip-build` for kim,
//!   subak, noeul and ddalgak (deterministic zips, `target/dist`).
//! - Copies the zips into `<out>` (default `kim-local` at the repository root, next to the
//!   root `kim.exe`) and writes `<out>/catalog.json`: the bundled catalog with one release per
//!   tool whose package is a `file:///` URL into `<out>`, `catalog_version` one above the
//!   bundled one, then signs it (`catalog.json.sig`) with the given key.
//! - Nothing is installed, uploaded or published. A double-click on the `kim.exe` next to
//!   `<out>` opens Kim Manager with this mirror; Kim refuses `file:///` packages in a catalog
//!   that did not itself come from a `file://` source, and never caches a mirror.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The tools in the mirror, with the crates their packages need.
const TOOLS: [(&str, &[&str]); 4] = [
    ("kim", &["kim", "kim-manager", "kim-setup"]),
    ("subak", &["subak"]),
    ("noeul", &["noeul"]),
    ("ddalgak", &["ddalgak", "ddalgak-editor"]),
];

fn file_url(p: &Path) -> String {
    kim::start::file_url(p)
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let usage = "usage: cargo xtask local-mirror --key <file> [--out <dir>] [--skip-build]";
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let key = value("--key").ok_or_else(|| usage.to_string())?;
    let out = value("--out").map_or_else(|| root.join("kim-local"), PathBuf::from);
    let out = if out.is_absolute() {
        out
    } else {
        root.join(out)
    };
    if !args.iter().any(|a| a == "--skip-build") {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut c = Command::new(cargo);
        c.args(["build", "--release"]).current_dir(root);
        for (_, crates) in TOOLS {
            for k in crates {
                c.args(["-p", k]);
            }
        }
        if !c.status().map_err(|e| e.to_string())?.success() {
            return Err("cargo build --release failed".into());
        }
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    // Only the mirror's own files are replaced: old packages, the catalog and its signature.
    for e in std::fs::read_dir(&out)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.ends_with("-windows-x86_64.zip") || n == "catalog.json" || n == "catalog.json.sig" {
            std::fs::remove_file(e.path()).map_err(|e| format!("{n}: {e}"))?;
        }
    }

    let bundled = root.join("catalog").join("catalog.json");
    let text =
        std::fs::read_to_string(&bundled).map_err(|e| format!("{}: {e}", bundled.display()))?;
    let mut catalog: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let now = kim::time::now_utc();
    let today = format!("{}T00:00:00Z", kim::time::format_date(now));
    let expires = format!("{}T00:00:00Z", kim::time::format_date(now + 90 * 86_400));
    let dist = root.join("target").join("dist");
    for (id, _) in TOOLS {
        crate::package::run(root, &[id.to_string(), "--skip-build".into()])?;
        let manifest = std::fs::read_dir(&dist)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n.starts_with(&format!("{id}-")) && n.ends_with("-release.json")
                })
            })
            .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
            .ok_or_else(|| format!("no release manifest for {id} in {}", dist.display()))?;
        let m: Value =
            serde_json::from_slice(&std::fs::read(&manifest).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let file = m["asset"]["file"]
            .as_str()
            .ok_or("manifest without asset.file")?;
        let zip = out.join(file);
        std::fs::copy(dist.join(file), &zip).map_err(|e| format!("{file}: {e}"))?;
        let mut release = m["catalog_release"].clone();
        release["published_at"] = json!(today);
        release["release_notes"] = json!(format!(
            "Built on this PC from commit {} (local mirror, not a published release)",
            m["build"]["commit"].as_str().unwrap_or("?")
        ));
        release["assets"][0]["url"] = json!(file_url(&zip));
        let tool = catalog["tools"]
            .as_array_mut()
            .and_then(|ts| ts.iter_mut().find(|t| t["id"] == id))
            .ok_or_else(|| format!("{id} is not in the bundled catalog"))?;
        let releases = tool["releases"]
            .as_array_mut()
            .ok_or("tool without releases")?;
        releases.retain(|r| r["version"] != release["version"]);
        releases.insert(0, release);
        println!("local-mirror: {id} {} -> {}", m["version"], zip.display());
    }
    let version = catalog["catalog_version"]
        .as_u64()
        .ok_or("no catalog_version")?
        + 1;
    catalog["catalog_version"] = json!(version);
    catalog["generated_at"] = json!(today);
    catalog["expires_at"] = json!(expires);
    let path = out.join("catalog.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&catalog).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    crate::catalog_tools::sign(Path::new(&key), &path)?;
    println!(
        "local-mirror: {} (catalog v{version}); double-click the kim.exe next to {} to open Kim Manager with it",
        path.display(),
        out.display()
    );
    Ok(())
}
