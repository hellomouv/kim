//! Step 26: the tmux compatibility matrix (`docs/TMUX_COMPATIBILITY.md` §1–§22) checked against
//! the repository, so the matrix reflects working code (part of `docs-check`):
//! - every row has six cells, a known Status and a Target Step (`N`, `N, M` or `post-26`);
//! - an `implemented` or `partial` row cites evidence: at least one `path.rs` or
//!   `path.rs::item` that exists, whose last item (`fn`, `const`, `struct`, …) is defined there;
//! - a `deferred` or `unsupported` row says why (`*Rationale` or `*Step 25 inventory:`);
//! - no `planned` row targets a Step that is already done (≤ [`DONE_THROUGH`]);
//! - the §27 summary counts equal the rows.

use std::collections::BTreeMap;
use std::path::Path;

/// The last Step whose matrix rows must be classified (no longer `planned`).
pub const DONE_THROUGH: u32 = 25;

pub const STATUSES: [&str; 5] = [
    "planned",
    "partial",
    "implemented",
    "deferred",
    "unsupported",
];

#[derive(Debug, Default)]
pub struct Matrix {
    pub rows: usize,
    pub by_status: BTreeMap<&'static str, usize>,
    pub evidence_refs: usize,
}

/// One matrix row: (section, the six cells).
fn rows(text: &str) -> Vec<(u32, Vec<String>)> {
    let mut out = Vec::new();
    let mut sec = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            sec = rest
                .split('.')
                .next()
                .and_then(|n| n.parse::<u32>().ok())
                .filter(|n| (1..=22).contains(n));
            continue;
        }
        let Some(sec) = sec else { continue };
        if !line.starts_with("| ")
            || line.starts_with("| ---")
            || line.starts_with("| tmux feature")
        {
            continue;
        }
        let cells: Vec<String> = line
            .trim()
            .trim_matches('|')
            .split(" | ")
            .map(|c| c.trim().to_string())
            .collect();
        out.push((sec, cells));
    }
    out
}

/// The evidence references of a cell: `` `path.rs` `` or `` `path.rs::a::b` ``.
pub fn evidence(cell: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for (i, part) in cell.split('`').enumerate() {
        if i % 2 == 0 {
            continue;
        }
        let (path, item) = match part.split_once("::") {
            Some((p, rest)) => (
                p,
                Some(rest.rsplit("::").next().unwrap_or(rest).to_string()),
            ),
            None => (part, None),
        };
        if path.ends_with(".rs") && !path.contains(' ') {
            out.push((path.to_string(), item));
        }
    }
    out
}

fn valid_target(t: &str) -> bool {
    t == "post-26"
        || t.split(", ")
            .all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

fn defines(source: &str, item: &str) -> bool {
    if item == "tests" {
        return source.contains("mod tests");
    }
    [
        "fn ", "const ", "static ", "struct ", "enum ", "mod ", "trait ", "type ",
    ]
    .iter()
    .any(|k| {
        source.match_indices(&format!("{k}{item}")).any(|(i, m)| {
            let next = source[i + m.len()..].chars().next();
            !next.is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    })
}

pub fn check(root: &Path, problems: &mut Vec<String>) -> Result<Matrix, String> {
    let path = root.join("docs/TMUX_COMPATIBILITY.md");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut m = Matrix::default();
    let mut sources: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut per_sec: BTreeMap<u32, [usize; 5]> = BTreeMap::new();
    for (sec, c) in rows(&text) {
        let name = c.first().cloned().unwrap_or_default();
        let at = format!("TMUX_COMPATIBILITY §{sec} `{name}`");
        if c.len() != 6 {
            problems.push(format!("{at}: expected 6 cells, found {}", c.len()));
            continue;
        }
        let Some(si) = STATUSES.iter().position(|s| *s == c[3]) else {
            problems.push(format!("{at}: unknown status `{}`", c[3]));
            continue;
        };
        m.rows += 1;
        *m.by_status.entry(STATUSES[si]).or_default() += 1;
        per_sec.entry(sec).or_default()[si] += 1;
        // `—`: no Step will deliver it (`unsupported`).
        if !(valid_target(&c[4]) || (c[4] == "—" && STATUSES[si] == "unsupported")) {
            problems.push(format!("{at}: invalid target Step `{}`", c[4]));
        }
        match STATUSES[si] {
            "implemented" | "partial" => {
                let refs = evidence(&c[5]);
                if refs.is_empty() {
                    problems.push(format!("{at}: {} without test evidence", c[3]));
                }
                for (file, item) in refs {
                    m.evidence_refs += 1;
                    let src = sources
                        .entry(file.clone())
                        .or_insert_with(|| std::fs::read_to_string(root.join(&file)).ok());
                    match (src, item) {
                        (None, _) => {
                            problems.push(format!("{at}: evidence `{file}` does not exist"));
                        }
                        (Some(s), Some(item)) if !defines(s, &item) => {
                            problems.push(format!("{at}: `{file}` defines no `{item}`"));
                        }
                        _ => {}
                    }
                }
            }
            "deferred" | "unsupported" => {
                if !c[2].contains("*Rationale") && !c[2].contains("*Step 25 inventory:") {
                    problems.push(format!("{at}: {} without a rationale", c[3]));
                }
            }
            _ => {
                let done = c[4]
                    .split(", ")
                    .filter_map(|n| n.parse::<u32>().ok())
                    .all(|n| n <= DONE_THROUGH);
                if done && c[4] != "post-26" {
                    problems.push(format!(
                        "{at}: still `planned` for Step {}, which is done",
                        c[4]
                    ));
                }
            }
        }
    }
    // §27 must count what is there.
    for (si, st) in STATUSES.iter().enumerate() {
        let want = format!("| `{st}` | {} |", m.by_status.get(st).copied().unwrap_or(0));
        if !text.contains(&want) {
            problems.push(format!(
                "TMUX_COMPATIBILITY §27: the `{st}` count is not {}",
                m.by_status.get(st).copied().unwrap_or(0)
            ));
        }
        let _ = si;
    }
    if !text.contains(&format!("| **Total** | **{}** |", m.rows)) {
        problems.push(format!(
            "TMUX_COMPATIBILITY §27: the total is not {}",
            m.rows
        ));
    }
    for (sec, c) in &per_sec {
        let total: usize = c.iter().sum();
        let want = format!(
            " | {} | {} | {} | {} | {} | {} |",
            c[0], c[1], c[2], c[3], c[4], total
        );
        let ok = text
            .lines()
            .any(|l| l.starts_with(&format!("| §{sec} ")) && l.ends_with(&want));
        if !ok {
            problems.push(format!(
                "TMUX_COMPATIBILITY §27: the §{sec} row is not{want}"
            ));
        }
    }
    Ok(m)
}

/// Step 32: `docs/TABBY_FEATURE_MATRIX.md` against the repository: every row (`| `ID-n` …`) has
/// six cells and a known status; `implemented`/`partial` rows cite evidence that exists; no
/// `planned` row targets a finished Step or nothing; the summary counts equal the rows.
pub const TABBY_DONE_THROUGH: u32 = 32;
pub const TABBY_STATUSES: [&str; 5] = ["planned", "partial", "implemented", "excluded", "blocked"];

pub fn check_tabby(root: &Path, problems: &mut Vec<String>) -> Result<Matrix, String> {
    let path = root.join("docs/TABBY_FEATURE_MATRIX.md");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut m = Matrix::default();
    let mut sources: BTreeMap<String, Option<String>> = BTreeMap::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("| `") else {
            continue;
        };
        let Some(id) = rest.split('`').next() else {
            continue;
        };
        if !id.contains('-') || !id.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            continue;
        }
        let c: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split(" | ")
            .map(str::trim)
            .collect();
        let at = format!("TABBY_FEATURE_MATRIX `{id}`");
        if c.len() != 6 {
            problems.push(format!("{at}: expected 6 cells, found {}", c.len()));
            continue;
        }
        let Some(si) = TABBY_STATUSES.iter().position(|s| *s == c[3]) else {
            problems.push(format!("{at}: unknown status `{}`", c[3]));
            continue;
        };
        m.rows += 1;
        *m.by_status.entry(TABBY_STATUSES[si]).or_default() += 1;
        match TABBY_STATUSES[si] {
            "implemented" | "partial" => {
                let refs = evidence(c[5]);
                if refs.is_empty() {
                    problems.push(format!("{at}: {} without test evidence", c[3]));
                }
                for (file, item) in refs {
                    m.evidence_refs += 1;
                    let src = sources
                        .entry(file.clone())
                        .or_insert_with(|| std::fs::read_to_string(root.join(&file)).ok());
                    match (src, item) {
                        (None, _) => {
                            problems.push(format!("{at}: evidence `{file}` does not exist"));
                        }
                        (Some(s), Some(item)) if !defines(s, &item) => {
                            problems.push(format!("{at}: `{file}` defines no `{item}`"));
                        }
                        _ => {}
                    }
                }
            }
            "planned" => {
                let steps: Vec<u32> = if c[4].starts_with("Step") {
                    c[4].split(|ch: char| !ch.is_ascii_digit())
                        .filter_map(|n| n.parse().ok())
                        .collect()
                } else {
                    Vec::new()
                };
                if c[4] == "—" || c[4].is_empty() {
                    problems.push(format!("{at}: planned without a target Step"));
                } else if !steps.is_empty() && steps.iter().all(|n| *n <= TABBY_DONE_THROUGH) {
                    problems.push(format!("{at}: still `planned` for {}, which is done", c[4]));
                }
            }
            _ => {}
        }
    }
    for st in TABBY_STATUSES {
        let n = m.by_status.get(st).copied().unwrap_or(0);
        if !text.contains(&format!("| `{st}` | {n}")) {
            problems.push(format!(
                "TABBY_FEATURE_MATRIX summary: the `{st}` count is not {n}"
            ));
        }
    }
    if !text.contains(&format!("| **Total** | **{}** |", m.rows)) {
        problems.push(format!(
            "TABBY_FEATURE_MATRIX summary: the total is not {}",
            m.rows
        ));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_names_files_and_items() {
        assert_eq!(
            evidence(
                "`crates/a/tests/x.rs::journey` (2026-11-11), `crates/b/src/y.rs::tests::unit`, `subak new`"
            ),
            [
                (
                    "crates/a/tests/x.rs".to_string(),
                    Some("journey".to_string())
                ),
                ("crates/b/src/y.rs".to_string(), Some("unit".to_string())),
            ]
        );
        assert!(defines("fn journey() {}", "journey"));
        assert!(!defines("fn journey_two() {}", "journey"));
        assert!(defines("pub const HOOKS: &[] = &[];", "HOOKS"));
        assert!(valid_target("10, 12") && valid_target("post-26") && !valid_target("soon"));
    }

    #[test]
    fn the_tabby_matrix_reflects_the_code() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut problems = Vec::new();
        let m = check_tabby(root, &mut problems).expect("matrix readable");
        assert!(m.rows > 200, "{} rows", m.rows);
        assert!(
            problems.is_empty(),
            "{}",
            problems.join(
                "
"
            )
        );
    }

    #[test]
    fn the_repository_matrix_reflects_the_code() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut problems = Vec::new();
        let m = check(root, &mut problems).expect("matrix readable");
        assert!(m.rows > 500, "{} rows", m.rows);
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
