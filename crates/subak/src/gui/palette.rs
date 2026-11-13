//! #98 T1 (`CORE-63`): the command palette (`command.palette`, `Ctrl+Shift+P`): one fuzzy list
//! over the window's actions, its Terminals and the shell profiles. Pure: the entries, the
//! ranking and the text the window draws; running an entry is the window's.

/// What an entry does when it is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A window action, by its manifest id.
    Action(String),
    /// Show this Terminal (its tab, or a hidden one).
    Terminal(String),
    /// A new Terminal from the `k`-th profile (`Settings::all_profiles`).
    Profile(usize),
}

/// One line of the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub target: Target,
    /// What the user reads and types against (`Split pane left/right`, `Terminal: build`).
    pub label: String,
    /// The keys that run it now, if any (shown on the right).
    pub keys: String,
}

/// How well `query` matches `text`: every character of the query in order (case-insensitive),
/// better when they are adjacent, at word starts and early. `None`: no match. An empty query
/// matches everything equally.
pub fn score(query: &str, text: &str) -> Option<u32> {
    let q: Vec<char> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let mut qi = 0;
    let mut score = 0u32;
    let mut last: Option<usize> = None;
    for (i, c) in t.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            let word_start = i == 0 || !t[i - 1].is_alphanumeric();
            score += 10;
            if word_start {
                score += 8;
            }
            if last.is_some_and(|l| l + 1 == i) {
                score += 12;
            }
            if i < 8 {
                score += 2;
            }
            last = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then_some(score)
}

/// The entries matching `query`, best first (ties keep the entries' order).
pub fn rank<'a>(query: &str, entries: &'a [Entry]) -> Vec<&'a Entry> {
    let mut hits: Vec<(u32, usize, &Entry)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| score(query, &e.label).map(|s| (s, i, e)))
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, _, e)| e).collect()
}

/// The palette's text for a `cols × rows` view: the query line, then the matches with the
/// selected one in reverse video and its keys on the right; a line saying so when none match.
pub fn vt(
    query: &str,
    cursor: usize,
    hits: &[&Entry],
    selected: usize,
    cols: u16,
    rows: u16,
) -> Vec<u8> {
    let cols = usize::from(cols);
    let fit = |s: &str, w: usize| -> String { s.chars().take(w).collect() };
    let (before, after): (String, String) = (
        query.chars().take(cursor).collect(),
        query.chars().skip(cursor).collect(),
    );
    // (No cursor: the caret is drawn as a bar in the query.)
    let mut out = format!(
        "\x1b[?25l\x1b[1m> {}\u{258f}{}\x1b[0m   \x1b[2m(type to filter; Up/Down choose, Enter runs, Esc closes)\x1b[0m",
        before, after
    );
    if hits.is_empty() {
        out.push_str("\r\n\r\n  No action, Terminal or profile matches.");
        return out.into_bytes();
    }
    let room = usize::from(rows).saturating_sub(2).max(1);
    let first = selected.saturating_sub(room - 1);
    for (i, e) in hits.iter().enumerate().skip(first).take(room) {
        let keys = if e.keys.is_empty() {
            String::new()
        } else {
            format!("  {}", e.keys)
        };
        let width = cols.saturating_sub(4);
        let label_w = width.saturating_sub(keys.chars().count());
        let label = fit(&e.label, label_w);
        let pad = " ".repeat(label_w.saturating_sub(label.chars().count()));
        let line = format!("{label}{pad}{keys}");
        if i == selected {
            out.push_str(&format!("\r\n \x1b[7m {line} \x1b[0m"));
        } else {
            out.push_str(&format!("\r\n  {line}"));
        }
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(label: &str) -> Entry {
        Entry {
            target: Target::Action(label.into()),
            label: label.into(),
            keys: String::new(),
        }
    }

    /// #98 T1 (`CORE-63`): subsequence matching, word starts and adjacency rank higher.
    #[test]
    fn fuzzy_ranking_prefers_word_starts_and_runs() {
        assert_eq!(score("", "anything"), Some(0));
        assert!(score("spr", "Split pane left/right").is_some());
        assert!(score("xyz", "Split pane left/right").is_none());
        let list = [
            e("Scroll one page forward"),
            e("Split pane left/right"),
            e("Terminal: split-test"),
        ];
        let hits: Vec<&str> = rank("split", &list)
            .iter()
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(hits, ["Split pane left/right", "Terminal: split-test"]);
        let hits: Vec<&str> = rank("sp", &list).iter().map(|e| e.label.as_str()).collect();
        assert_eq!(hits[0], "Split pane left/right", "{hits:?}");
        assert_eq!(rank("", &list).len(), 3, "all, in order");
    }

    #[test]
    fn the_palette_text_shows_the_query_matches_and_selection() {
        let list = [e("New Terminal"), e("Split pane left/right")];
        let hits: Vec<&Entry> = list.iter().collect();
        let text = String::from_utf8(vt("ne", 2, &hits, 1, 60, 10)).unwrap();
        assert!(text.starts_with("\x1b[?25l\x1b[1m> ne\u{258f}"), "{text}");
        assert!(
            text.contains("\x1b[7m Split pane left/right"),
            "the selection: {text}"
        );
        let none = String::from_utf8(vt("zz", 2, &[], 0, 60, 10)).unwrap();
        assert!(none.contains("No action, Terminal or profile matches."));
    }
}
