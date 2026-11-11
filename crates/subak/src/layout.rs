//! The layout of an internal window (Step 22, ADR 0056): a tree of splits over the window's
//! panes, in character cells, owned by the Host.
//!
//! Terms are tmux's: a **horizontal** split puts its children side by side, left to right
//! (`split-window -h`); a **vertical** split stacks them, top to bottom (`split-window -v`, the
//! default). Siblings are separated by one cell (the border), so the children of a horizontal
//! split of width `W` have widths that add up to `W - (n - 1)`. Every pane keeps at least
//! [`MIN_COLS`] × [`MIN_ROWS`] cells: a split, a resize or a smaller window never makes a pane
//! smaller than that (a window smaller than its layout's minimum is laid out at that minimum and
//! clients clip it).
//!
//! The tree is pure data: the Host applies the resulting pane sizes to the pseudo consoles.

use serde_json::{Value, json};

/// The smallest pane width in cells.
pub const MIN_COLS: u16 = 2;
/// The smallest pane height in cells.
pub const MIN_ROWS: u16 = 2;

/// How a split arranges its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Side by side, left to right (tmux `-h`).
    Horizontal,
    /// Stacked, top to bottom (tmux `-v`).
    Vertical,
}

impl Dir {
    pub fn name(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    /// The extent along this direction of a `(cols, rows)` size.
    fn along(self, (c, r): (u16, u16)) -> u16 {
        match self {
            Self::Horizontal => c,
            Self::Vertical => r,
        }
    }

    fn min(self) -> u16 {
        match self {
            Self::Horizontal => MIN_COLS,
            Self::Vertical => MIN_ROWS,
        }
    }
}

/// A direction to move towards (focus, resize).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

impl Side {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            _ => None,
        }
    }

    fn dir(self) -> Dir {
        match self {
            Self::Left | Self::Right => Dir::Horizontal,
            Self::Up | Self::Down => Dir::Vertical,
        }
    }

    /// Towards the end of the axis (right, down).
    fn forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }
}

/// Where a pane is in its window, in cells from the window's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
}

impl Rect {
    pub fn size(self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    pub fn json(self) -> Value {
        json!({"x": self.x, "y": self.y, "cols": self.cols, "rows": self.rows})
    }
}

/// A node of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Pane(String),
    Split { dir: Dir, children: Vec<Child> },
}

/// A child of a split and its extent along the split's direction, in cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Child {
    pub size: u16,
    pub node: Node,
}

/// Why a layout change was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The pane is not in this layout.
    NoPane,
    /// Not enough room: the pane would be smaller than the minimum.
    NoSpace { need: u16, have: u16 },
}

/// How big a new pane of a split is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Amount {
    Cells(u16),
    Percent(u8),
}

/// A basic layout every pane of a window is arranged in (tmux `select-layout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// All panes side by side, equally wide.
    EvenHorizontal,
    /// All panes stacked, equally high.
    EvenVertical,
    /// Rows of equal panes: as many columns as the square root of the count, rounded up.
    Tiled,
    /// Step 25: the first pane across the top ([`MAIN_PANE_HEIGHT`] rows), the others side by
    /// side below it (`main-horizontal`); mirrored: the main pane at the bottom.
    MainHorizontal,
    MainHorizontalMirrored,
    /// Step 25: the first pane on the left ([`MAIN_PANE_WIDTH`] columns), the others stacked on
    /// its right (`main-vertical`); mirrored: the main pane on the right.
    MainVertical,
    MainVerticalMirrored,
}

/// tmux's `main-pane-width` and `main-pane-height` defaults (cells), clamped to the window.
pub const MAIN_PANE_WIDTH: u16 = 80;
pub const MAIN_PANE_HEIGHT: u16 = 24;

impl Preset {
    /// tmux's order of `next-layout`.
    pub const CYCLE: [Preset; 7] = [
        Preset::EvenHorizontal,
        Preset::EvenVertical,
        Preset::MainHorizontal,
        Preset::MainHorizontalMirrored,
        Preset::MainVertical,
        Preset::MainVerticalMirrored,
        Preset::Tiled,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        Self::CYCLE.into_iter().find(|p| p.name() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::EvenHorizontal => "even-horizontal",
            Self::EvenVertical => "even-vertical",
            Self::Tiled => "tiled",
            Self::MainHorizontal => "main-horizontal",
            Self::MainHorizontalMirrored => "main-horizontal-mirrored",
            Self::MainVertical => "main-vertical",
            Self::MainVerticalMirrored => "main-vertical-mirrored",
        }
    }
}

/// A window's layout: the tree, the window's size and its zoomed pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    root: Node,
    size: (u16, u16),
    zoomed: Option<String>,
    /// Step 25: the preset last applied (for `next-layout`), until the tree changes otherwise.
    preset: Option<Preset>,
}

impl Layout {
    /// A window with one pane of `size`.
    pub fn new(pane: &str, size: (u16, u16)) -> Self {
        Self {
            root: Node::Pane(pane.to_string()),
            size: (size.0.max(MIN_COLS), size.1.max(MIN_ROWS)),
            zoomed: None,
            preset: None,
        }
    }

    /// Step 25 (`next-layout`, `previous-layout`): the next or previous preset of tmux's cycle.
    pub fn cycle(&mut self, forward: bool) -> Preset {
        let n = Preset::CYCLE.len();
        let next = match self
            .preset
            .and_then(|p| Preset::CYCLE.iter().position(|q| *q == p))
        {
            Some(i) if forward => Preset::CYCLE[(i + 1) % n],
            Some(i) => Preset::CYCLE[(i + n - 1) % n],
            None if forward => Preset::CYCLE[0],
            None => Preset::CYCLE[n - 1],
        };
        self.preset(next);
        next
    }

    /// Step 30: gives this tree the saved tree's cell sizes where both have the same shape
    /// (`map`: saved pane id → this tree's pane id), then fits the window. A part whose shape
    /// differs keeps its sizes.
    pub fn apply_cells(&mut self, saved: &Value) {
        fn walk(n: &mut Node, s: &Value) {
            match n {
                Node::Pane(_) => {}
                Node::Split { dir, children } => {
                    let sdir = match s["direction"].as_str() {
                        Some("horizontal") => Some(Dir::Horizontal),
                        Some("vertical") => Some(Dir::Vertical),
                        _ => None,
                    };
                    let Some(sc) = s["children"].as_array() else {
                        return;
                    };
                    if sdir != Some(*dir) || sc.len() != children.len() {
                        return;
                    }
                    for (c, x) in children.iter_mut().zip(sc) {
                        if let Some(cells) = x["cells"].as_u64().and_then(|v| u16::try_from(v).ok())
                        {
                            c.size = cells.max(1);
                        }
                        walk(&mut c.node, &x["node"]);
                    }
                }
            }
        }
        walk(&mut self.root, saved);
        let size = self.size;
        fit(&mut self.root, Dir::Horizontal, size.0);
        fit(&mut self.root, Dir::Vertical, size.1);
    }

    /// Step 25 (`select-layout -E`): every split shares its space equally among its children.
    pub fn spread(&mut self) {
        fn ones(n: &mut Node) {
            if let Node::Split { children, .. } = n {
                for c in children.iter_mut() {
                    c.size = 1;
                    ones(&mut c.node);
                }
            }
        }
        self.zoomed = None;
        ones(&mut self.root);
        let size = self.size;
        fit(&mut self.root, Dir::Horizontal, size.0);
        fit(&mut self.root, Dir::Vertical, size.1);
        self.preset = None;
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    pub fn zoomed(&self) -> Option<&str> {
        self.zoomed.as_deref()
    }

    #[cfg(test)]
    pub fn root(&self) -> &Node {
        &self.root
    }

    /// The panes in layout order (left to right, top to bottom): tmux's pane order.
    pub fn panes(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect(&self.root, &mut out);
        out
    }

    pub fn contains(&self, pane: &str) -> bool {
        self.panes().iter().any(|p| p == pane)
    }

    /// Every pane's place. The zoomed pane, if any, fills the window; the others keep the
    /// places they have when it is unzoomed (and are not shown).
    pub fn rects(&self) -> Vec<(String, Rect)> {
        let mut out = Vec::new();
        let (c, r) = self.size;
        place(&self.root, 0, 0, c, r, &mut out);
        if let Some(z) = &self.zoomed {
            for (p, rect) in &mut out {
                if p == z {
                    *rect = Rect {
                        x: 0,
                        y: 0,
                        cols: c,
                        rows: r,
                    };
                }
            }
        }
        out
    }

    pub fn rect(&self, pane: &str) -> Option<Rect> {
        self.rects()
            .into_iter()
            .find(|(p, _)| p == pane)
            .map(|r| r.1)
    }

    /// Whether `pane` is drawn: every pane, or only the zoomed one.
    pub fn visible(&self, pane: &str) -> bool {
        self.zoomed.as_deref().is_none_or(|z| z == pane)
    }

    /// The smallest size the window can be laid out at.
    pub fn min_size(&self) -> (u16, u16) {
        (
            min_along(&self.root, Dir::Horizontal),
            min_along(&self.root, Dir::Vertical),
        )
    }

    /// Lays the window out at `size` (a client's view of the window). Children keep their
    /// proportions; the result never goes below [`Layout::min_size`].
    pub fn resize(&mut self, size: (u16, u16)) {
        let min = self.min_size();
        let size = (size.0.max(min.0), size.1.max(min.1));
        let (c, r) = self.size;
        if size.0 != c {
            fit(&mut self.root, Dir::Horizontal, size.0);
        }
        if size.1 != r {
            fit(&mut self.root, Dir::Vertical, size.1);
        }
        self.size = size;
    }

    /// Splits `target` in `dir`, adding `new` after it (or before it). `amount` is the new
    /// pane's extent (default: half). Unzooms.
    pub fn split(
        &mut self,
        target: &str,
        new: &str,
        dir: Dir,
        before: bool,
        amount: Option<Amount>,
    ) -> Result<(), Refused> {
        let rect = self.unzoomed_rect(target).ok_or(Refused::NoPane)?;
        let have = dir.along(rect.size());
        let min = dir.min();
        if have < 2 * min + 1 {
            return Err(Refused::NoSpace {
                need: 2 * min + 1,
                have,
            });
        }
        let room = have - 1;
        let wanted = match amount {
            None => room / 2,
            Some(Amount::Cells(n)) => n,
            Some(Amount::Percent(p)) => {
                u16::try_from(u32::from(room) * u32::from(p.min(100)) / 100).unwrap_or(room)
            }
        };
        let new_size = wanted.clamp(min, room - min);
        let old_size = room - new_size;
        self.zoomed = None;
        let leaf = find_parent(&mut self.root, target).ok_or(Refused::NoPane)?;
        let pair = |old: Node| {
            let a = Child {
                size: old_size,
                node: old,
            };
            let b = Child {
                size: new_size,
                node: Node::Pane(new.to_string()),
            };
            if before { vec![b, a] } else { vec![a, b] }
        };
        match leaf {
            Parent::Root => {
                let old = std::mem::replace(&mut self.root, Node::Pane(String::new()));
                self.root = Node::Split {
                    dir,
                    children: pair(old),
                };
            }
            Parent::Split(node, i) => {
                let Node::Split {
                    dir: pdir,
                    children,
                } = node
                else {
                    unreachable!("find_parent returns splits");
                };
                if *pdir == dir {
                    // Same direction: the new pane becomes a sibling.
                    children[i].size = old_size;
                    let c = Child {
                        size: new_size,
                        node: Node::Pane(new.to_string()),
                    };
                    children.insert(if before { i } else { i + 1 }, c);
                } else {
                    let along = children[i].size;
                    let old = std::mem::replace(&mut children[i].node, Node::Pane(String::new()));
                    children[i] = Child {
                        size: along,
                        node: Node::Split {
                            dir,
                            children: pair(old),
                        },
                    };
                }
            }
        }
        Ok(())
    }

    /// Takes `pane` out; its space goes to its next sibling (else the previous one). A split
    /// left with one child is replaced by it. The last pane is never removed.
    pub fn remove(&mut self, pane: &str) -> Result<(), Refused> {
        if matches!(self.root, Node::Pane(_)) {
            return Err(Refused::NoPane);
        }
        if self.zoomed.as_deref() == Some(pane) {
            self.zoomed = None;
        }
        let Some(Parent::Split(node, i)) = find_parent(&mut self.root, pane) else {
            return Err(Refused::NoPane);
        };
        let Node::Split { dir, children } = node else {
            unreachable!("find_parent returns splits");
        };
        let freed = children[i].size + 1;
        children.remove(i);
        let j = if i < children.len() { i } else { i - 1 };
        let along = children[j].size + freed;
        children[j].size = along;
        fit(&mut children[j].node, *dir, along);
        if children.len() == 1 {
            let only = children.remove(0).node;
            *node = only;
        }
        flatten(&mut self.root);
        Ok(())
    }

    /// Moves the border of `pane` towards `side` by up to `cells` (tmux `resize-pane -L/-R/-U/-D`):
    /// with a neighbour on that side the pane grows into it, otherwise the opposite border moves
    /// and the pane shrinks. Returns how many cells it moved (0: nothing to move). Unzooms.
    pub fn adjust(&mut self, pane: &str, side: Side, cells: u16) -> Result<u16, Refused> {
        if !self.contains(pane) {
            return Err(Refused::NoPane);
        }
        self.zoomed = None;
        Ok(adjust_in(&mut self.root, pane, side, cells))
    }

    /// Swaps two panes' places (tmux `swap-pane -s -t`). Unzooms.
    pub fn swap(&mut self, a: &str, b: &str) -> Result<(), Refused> {
        if !self.contains(a) || !self.contains(b) {
            return Err(Refused::NoPane);
        }
        self.zoomed = None;
        rename(&mut self.root, &|p| {
            if p == a {
                Some(b.to_string())
            } else if p == b {
                Some(a.to_string())
            } else {
                None
            }
        });
        Ok(())
    }

    /// Zooms `pane` (it fills the window) or unzooms (`None`).
    pub fn zoom(&mut self, pane: Option<&str>) -> Result<(), Refused> {
        if let Some(p) = pane
            && !self.contains(p)
        {
            return Err(Refused::NoPane);
        }
        self.zoomed = pane.filter(|_| self.panes().len() > 1).map(str::to_string);
        Ok(())
    }

    /// Moves every pane one place along the pane order (tmux `rotate-window`): `up`, the second
    /// pane takes the first place and the first the last; otherwise the other way. The places
    /// stay; the panes (and their processes) move. Unzooms.
    pub fn rotate(&mut self, up: bool) {
        let order = self.panes();
        if order.len() < 2 {
            return;
        }
        let n = order.len();
        self.zoomed = None;
        let to = |i: usize| -> String {
            if up {
                order[(i + 1) % n].clone()
            } else {
                order[(i + n - 1) % n].clone()
            }
        };
        let mut i = 0;
        relabel(&mut self.root, &mut |_| {
            let id = to(i);
            i += 1;
            id
        });
    }

    /// Arranges every pane, in pane order, in a basic layout with equal shares (tmux
    /// `select-layout even-horizontal|even-vertical|tiled`). Unzooms.
    pub fn preset(&mut self, p: Preset) {
        let order = self.panes();
        if order.len() < 2 {
            return;
        }
        self.zoomed = None;
        let leaf = |id: &String| Child {
            size: 1,
            node: Node::Pane(id.clone()),
        };
        self.root = match p {
            Preset::EvenHorizontal => Node::Split {
                dir: Dir::Horizontal,
                children: order.iter().map(leaf).collect(),
            },
            Preset::EvenVertical => Node::Split {
                dir: Dir::Vertical,
                children: order.iter().map(leaf).collect(),
            },
            Preset::MainHorizontal
            | Preset::MainHorizontalMirrored
            | Preset::MainVertical
            | Preset::MainVerticalMirrored => {
                let vertical = matches!(p, Preset::MainVertical | Preset::MainVerticalMirrored);
                let mirrored = matches!(
                    p,
                    Preset::MainHorizontalMirrored | Preset::MainVerticalMirrored
                );
                let (main_dir, rest_dir) = if vertical {
                    (Dir::Horizontal, Dir::Vertical)
                } else {
                    (Dir::Vertical, Dir::Horizontal)
                };
                // The main pane stays the main pane from one main-* preset to the next (a
                // mirrored one put it last); otherwise it is the first pane.
                let main_id = match self.preset {
                    Some(Preset::MainHorizontalMirrored | Preset::MainVerticalMirrored) => {
                        order[order.len() - 1].clone()
                    }
                    _ => order[0].clone(),
                };
                let rest: Vec<Child> = order
                    .iter()
                    .filter(|id| **id != main_id)
                    .map(leaf)
                    .collect();
                let others = if rest.len() == 1 {
                    rest.into_iter().next().map(|c| c.node).expect("one pane")
                } else {
                    Node::Split {
                        dir: rest_dir,
                        children: rest,
                    }
                };
                let total = main_dir.along(self.size);
                let want = if vertical {
                    MAIN_PANE_WIDTH
                } else {
                    MAIN_PANE_HEIGHT
                };
                let room = total.saturating_sub(1);
                let main_size = want
                    .min(room.saturating_sub(main_dir.min()))
                    .max(main_dir.min());
                let main = Child {
                    size: main_size,
                    node: Node::Pane(main_id),
                };
                let others = Child {
                    size: room.saturating_sub(main_size).max(1),
                    node: others,
                };
                Node::Split {
                    dir: main_dir,
                    children: if mirrored {
                        vec![others, main]
                    } else {
                        vec![main, others]
                    },
                }
            }
            Preset::Tiled => {
                let n = order.len();
                let cols = (1..=n).find(|c| c * c >= n).unwrap_or(n);
                let rows: Vec<Child> = order
                    .chunks(cols)
                    .map(|row| Child {
                        size: 1,
                        node: if row.len() == 1 {
                            Node::Pane(row[0].clone())
                        } else {
                            Node::Split {
                                dir: Dir::Horizontal,
                                children: row.iter().map(leaf).collect(),
                            }
                        },
                    })
                    .collect();
                if rows.len() == 1 {
                    rows.into_iter().next().map(|c| c.node).expect("one row")
                } else {
                    Node::Split {
                        dir: Dir::Vertical,
                        children: rows,
                    }
                }
            }
        };
        let min = self.min_size();
        let size = (self.size.0.max(min.0), self.size.1.max(min.1));
        fit(&mut self.root, Dir::Horizontal, size.0);
        fit(&mut self.root, Dir::Vertical, size.1);
        self.size = size;
        self.preset = Some(p);
    }

    /// The pane next to `pane` towards `side` (tmux `select-pane -L/-R/-U/-D`): among the panes
    /// whose border touches that side, the one that overlaps it most (the topmost or leftmost on
    /// a tie). Wraps around to the far side, as tmux does.
    pub fn neighbour(&self, pane: &str, side: Side) -> Option<String> {
        let rects = self.unzoomed_rects();
        let me = rects.iter().find(|(p, _)| p == pane)?.1;
        let (w, h) = self.size;
        let edge = |r: &Rect| -> (i32, i32, i32, i32) {
            (
                i32::from(r.x),
                i32::from(r.y),
                i32::from(r.x) + i32::from(r.cols),
                i32::from(r.y) + i32::from(r.rows),
            )
        };
        let (mx0, my0, mx1, my1) = edge(&me);
        let touches = |r: &Rect, wrap: bool| -> Option<i32> {
            let (x0, y0, x1, y1) = edge(r);
            let (adjacent, lo, hi, olo, ohi) = match side {
                Side::Left => (
                    if wrap {
                        x1 == i32::from(w)
                    } else {
                        x1 + 1 == mx0
                    },
                    my0,
                    my1,
                    y0,
                    y1,
                ),
                Side::Right => (if wrap { x0 == 0 } else { x0 == mx1 + 1 }, my0, my1, y0, y1),
                Side::Up => (
                    if wrap {
                        y1 == i32::from(h)
                    } else {
                        y1 + 1 == my0
                    },
                    mx0,
                    mx1,
                    x0,
                    x1,
                ),
                Side::Down => (if wrap { y0 == 0 } else { y0 == my1 + 1 }, mx0, mx1, x0, x1),
            };
            let overlap = hi.min(ohi) - lo.max(olo);
            (adjacent && overlap > 0).then_some(overlap)
        };
        for wrap in [false, true] {
            let best = rects
                .iter()
                .filter(|(p, _)| p != pane)
                .filter_map(|(p, r)| touches(r, wrap).map(|o| (o, r, p)))
                .max_by(|a, b| a.0.cmp(&b.0).then((b.1.y, b.1.x).cmp(&(a.1.y, a.1.x))));
            if let Some((_, _, p)) = best {
                return Some(p.clone());
            }
        }
        None
    }

    /// The tree as the contract writes it (STATE_SCHEMA.md "Layout"): `size` is a child's share
    /// of its split (0–1), `cells` its extent.
    pub fn json(&self) -> Value {
        node_json(&self.root, self.size)
    }

    fn unzoomed_rects(&self) -> Vec<(String, Rect)> {
        let mut out = Vec::new();
        place(&self.root, 0, 0, self.size.0, self.size.1, &mut out);
        out
    }

    fn unzoomed_rect(&self, pane: &str) -> Option<Rect> {
        self.unzoomed_rects()
            .into_iter()
            .find(|(p, _)| p == pane)
            .map(|r| r.1)
    }
}

fn collect(n: &Node, out: &mut Vec<String>) {
    match n {
        Node::Pane(p) => out.push(p.clone()),
        Node::Split { children, .. } => children.iter().for_each(|c| collect(&c.node, out)),
    }
}

fn place(n: &Node, x: u16, y: u16, c: u16, r: u16, out: &mut Vec<(String, Rect)>) {
    match n {
        Node::Pane(p) => out.push((
            p.clone(),
            Rect {
                x,
                y,
                cols: c,
                rows: r,
            },
        )),
        Node::Split { dir, children } => {
            let mut at = 0u16;
            for ch in children {
                match dir {
                    Dir::Horizontal => place(&ch.node, x + at, y, ch.size, r, out),
                    Dir::Vertical => place(&ch.node, x, y + at, c, ch.size, out),
                }
                at = at.saturating_add(ch.size).saturating_add(1);
            }
        }
    }
}

/// The smallest extent of `n` along `d`.
fn min_along(n: &Node, d: Dir) -> u16 {
    match n {
        Node::Pane(_) => d.min(),
        Node::Split { dir, children } if *dir == d => {
            let sum: u16 = children.iter().map(|c| min_along(&c.node, d)).sum();
            sum + u16::try_from(children.len() - 1).unwrap_or(0)
        }
        Node::Split { children, .. } => children
            .iter()
            .map(|c| min_along(&c.node, d))
            .max()
            .unwrap_or(d.min()),
    }
}

/// Gives `n` the extent `to` along `d`: a split in `d` shares it out in proportion (never below
/// each child's minimum); a split across `d` gives every child the same extent.
fn fit(n: &mut Node, d: Dir, to: u16) {
    let Node::Split { dir, children } = n else {
        return;
    };
    if *dir != d {
        for c in children.iter_mut() {
            fit(&mut c.node, d, to);
        }
        return;
    }
    let seps = u16::try_from(children.len() - 1).unwrap_or(0);
    let room = to.saturating_sub(seps);
    let old: u32 = children
        .iter()
        .map(|c| u32::from(c.size))
        .sum::<u32>()
        .max(1);
    let mins: Vec<u16> = children.iter().map(|c| min_along(&c.node, d)).collect();
    let mut sizes: Vec<u16> = children
        .iter()
        .zip(&mins)
        .map(|(c, m)| {
            let s = u32::from(c.size) * u32::from(room) / old;
            u16::try_from(s).unwrap_or(u16::MAX).max(*m)
        })
        .collect();
    // Rounding and minimums: settle the difference on the last children first.
    let mut total: i32 = sizes.iter().map(|s| i32::from(*s)).sum();
    let goal = i32::from(room);
    let mut i = sizes.len();
    while total != goal && i > 0 {
        i -= 1;
        if total < goal {
            let add = goal - total;
            sizes[i] = u16::try_from(i32::from(sizes[i]) + add).unwrap_or(u16::MAX);
            total = goal;
        } else {
            let can = i32::from(sizes[i]) - i32::from(mins[i]);
            let take = can.min(total - goal);
            sizes[i] = u16::try_from(i32::from(sizes[i]) - take).unwrap_or(0);
            total -= take;
            if i == 0 && total != goal {
                break;
            }
        }
    }
    for (c, s) in children.iter_mut().zip(sizes) {
        c.size = s;
        fit(&mut c.node, d, s);
    }
}

enum Parent<'a> {
    Root,
    /// The split holding the pane and the pane's position in it.
    Split(&'a mut Node, usize),
}

fn find_parent<'a>(n: &'a mut Node, pane: &str) -> Option<Parent<'a>> {
    let path = path_to(n, pane)?;
    let Some((&last, outer)) = path.split_last() else {
        return Some(Parent::Root);
    };
    let mut node = n;
    for &k in outer {
        let Node::Split { children, .. } = node else {
            return None;
        };
        node = &mut children[k].node;
    }
    Some(Parent::Split(node, last))
}

/// The child positions from the root down to `pane` (empty: the root is the pane).
fn path_to(n: &Node, pane: &str) -> Option<Vec<usize>> {
    match n {
        Node::Pane(p) => (p == pane).then(Vec::new),
        Node::Split { children, .. } => children.iter().enumerate().find_map(|(i, c)| {
            let mut rest = path_to(&c.node, pane)?;
            rest.insert(0, i);
            Some(rest)
        }),
    }
}

fn contains(n: &Node, pane: &str) -> bool {
    match n {
        Node::Pane(p) => p == pane,
        Node::Split { children, .. } => children.iter().any(|c| contains(&c.node, pane)),
    }
}

/// Merges a split child that is a split in the same direction into its parent (after a removal
/// replaced a split by its only child).
fn flatten(n: &mut Node) {
    let Node::Split { dir, children } = n else {
        return;
    };
    let d = *dir;
    let mut out = Vec::with_capacity(children.len());
    for mut c in std::mem::take(children) {
        flatten(&mut c.node);
        match c.node {
            Node::Split {
                dir: cd,
                children: inner,
            } if cd == d => out.extend(inner),
            node => out.push(Child { size: c.size, node }),
        }
    }
    *children = out;
}

/// Gives every pane, in layout order, the id `f` returns.
fn relabel(n: &mut Node, f: &mut dyn FnMut(&str) -> String) {
    match n {
        Node::Pane(p) => *p = f(p),
        Node::Split { children, .. } => children.iter_mut().for_each(|c| relabel(&mut c.node, f)),
    }
}

fn rename(n: &mut Node, f: &dyn Fn(&str) -> Option<String>) {
    match n {
        Node::Pane(p) => {
            if let Some(to) = f(p) {
                *p = to;
            }
        }
        Node::Split { children, .. } => children.iter_mut().for_each(|c| rename(&mut c.node, f)),
    }
}

/// Moves a border in the innermost split along `side`'s axis that has one for `pane`.
fn adjust_in(n: &mut Node, pane: &str, side: Side, cells: u16) -> u16 {
    let Node::Split { dir, children } = n else {
        return 0;
    };
    let Some(i) = children.iter().position(|c| contains(&c.node, pane)) else {
        return 0;
    };
    // The innermost split first: a nested one may own the border.
    let inner = adjust_in(&mut children[i].node, pane, side, cells);
    if inner > 0 || *dir != side.dir() || children.len() < 2 {
        return inner;
    }
    let d = *dir;
    let last = children.len() - 1;
    // The border to move and who grows: (left/top child, right/bottom child, grow the first?).
    let (a, b, grow_a) = match (side.forward(), i) {
        (true, i) if i < last => (i, i + 1, true),
        (true, i) => (i - 1, i, true),
        (false, 0) => (0, 1, false),
        (false, i) => (i - 1, i, false),
    };
    let (grower, shrinker) = if grow_a { (a, b) } else { (b, a) };
    let can = children[shrinker]
        .size
        .saturating_sub(min_along(&children[shrinker].node, d));
    let moved = cells.min(can);
    if moved == 0 {
        return 0;
    }
    children[shrinker].size -= moved;
    children[grower].size += moved;
    let (gs, ss) = (children[grower].size, children[shrinker].size);
    fit_edge(&mut children[grower].node, d, gs, grower < shrinker);
    fit_edge(&mut children[shrinker].node, d, ss, shrinker < grower);
    moved
}

/// Like [`fit`], but a split in `d` changes only its child at the moved border (`at_end`: the
/// border is at its end) as far as that child allows, as tmux does.
fn fit_edge(n: &mut Node, d: Dir, to: u16, at_end: bool) {
    let Node::Split { dir, children } = n else {
        return;
    };
    if *dir != d {
        for c in children.iter_mut() {
            fit_edge(&mut c.node, d, to, at_end);
        }
        return;
    }
    let seps = u16::try_from(children.len() - 1).unwrap_or(0);
    let now: u16 = children.iter().map(|c| c.size).sum::<u16>() + seps;
    let mut delta = i32::from(to) - i32::from(now);
    let order: Vec<usize> = if at_end {
        (0..children.len()).rev().collect()
    } else {
        (0..children.len()).collect()
    };
    for k in order {
        if delta == 0 {
            break;
        }
        let min = i32::from(min_along(&children[k].node, d));
        let cur = i32::from(children[k].size);
        let next = (cur + delta).max(min);
        delta -= next - cur;
        children[k].size = u16::try_from(next).unwrap_or(0);
        let s = children[k].size;
        fit_edge(&mut children[k].node, d, s, at_end);
    }
}

fn node_json(n: &Node, size: (u16, u16)) -> Value {
    match n {
        Node::Pane(p) => json!({"type": "pane", "pane": p}),
        Node::Split { dir, children } => {
            let total = f64::from(dir.along(size)).max(1.0);
            let kids: Vec<Value> = children
                .iter()
                .map(|c| {
                    let sub = match dir {
                        Dir::Horizontal => (c.size, size.1),
                        Dir::Vertical => (size.0, c.size),
                    };
                    let share = (f64::from(c.size) / total * 1000.0).round() / 1000.0;
                    json!({"size": share, "cells": c.size, "node": node_json(&c.node, sub)})
                })
                .collect();
            json!({"type": "split", "direction": dir.name(), "children": kids})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: u16, y: u16, cols: u16, rows: u16) -> Rect {
        Rect { x, y, cols, rows }
    }

    fn rects(l: &Layout) -> Vec<(String, Rect)> {
        l.rects()
    }

    /// Every pane inside the window, none overlapping, all at least the minimum.
    fn check(l: &Layout) {
        let rs = rects(l);
        let (w, h) = l.size();
        for (p, a) in &rs {
            assert!(
                a.cols >= MIN_COLS && a.rows >= MIN_ROWS,
                "{p} too small: {a:?}"
            );
            assert!(
                a.x + a.cols <= w && a.y + a.rows <= h,
                "{p} outside {w}x{h}: {a:?}"
            );
        }
        if l.zoomed().is_none() {
            for (i, (p, a)) in rs.iter().enumerate() {
                for (q, b) in rs.iter().skip(i + 1) {
                    let apart = a.x + a.cols <= b.x
                        || b.x + b.cols <= a.x
                        || a.y + a.rows <= b.y
                        || b.y + b.rows <= a.y;
                    assert!(apart, "{p} {a:?} overlaps {q} {b:?}");
                }
            }
            // Cells covered by panes plus borders: nothing is lost.
            let area: u32 = rs
                .iter()
                .map(|(_, a)| u32::from(a.cols) * u32::from(a.rows))
                .sum();
            assert!(area <= u32::from(w) * u32::from(h));
        }
    }

    #[test]
    fn a_horizontal_split_puts_panes_side_by_side_with_a_border() {
        // SUBAK-FR-013: true horizontal and vertical bisecting with equal initial sizes.
        let mut l = Layout::new("a", (81, 24));
        l.split("a", "b", Dir::Horizontal, false, None).unwrap();
        assert_eq!(
            rects(&l),
            [
                ("a".into(), r(0, 0, 40, 24)),
                ("b".into(), r(41, 0, 40, 24))
            ]
        );
        assert_eq!(l.panes(), ["a", "b"]);
        check(&l);
    }

    #[test]
    fn a_vertical_split_stacks_and_before_puts_the_new_pane_first() {
        let mut l = Layout::new("a", (80, 25));
        l.split("a", "b", Dir::Vertical, true, None).unwrap();
        assert_eq!(
            rects(&l),
            [
                ("b".into(), r(0, 0, 80, 12)),
                ("a".into(), r(0, 13, 80, 12))
            ]
        );
        assert_eq!(l.panes(), ["b", "a"]);
    }

    #[test]
    fn splits_nest_and_same_direction_splits_become_siblings() {
        let mut l = Layout::new("a", (120, 40));
        l.split("a", "b", Dir::Horizontal, false, None).unwrap();
        l.split("b", "c", Dir::Vertical, false, None).unwrap();
        l.split("a", "d", Dir::Horizontal, false, None).unwrap();
        // a | d | (b / c): `d` joined the horizontal split as a sibling.
        assert_eq!(l.panes(), ["a", "d", "b", "c"]);
        let j = l.json();
        assert_eq!(j["type"], "split");
        assert_eq!(j["direction"], "horizontal");
        assert_eq!(j["children"].as_array().unwrap().len(), 3);
        assert_eq!(j["children"][2]["node"]["direction"], "vertical");
        check(&l);
    }

    #[test]
    fn sizes_by_cells_and_percent_are_kept_within_the_minimum() {
        let mut l = Layout::new("a", (100, 30));
        l.split("a", "b", Dir::Horizontal, false, Some(Amount::Cells(10)))
            .unwrap();
        assert_eq!(l.rect("b").unwrap().cols, 10);
        assert_eq!(l.rect("a").unwrap().cols, 89);
        l.split("a", "c", Dir::Vertical, false, Some(Amount::Percent(30)))
            .unwrap();
        assert_eq!(l.rect("c").unwrap().rows, 8); // 30 % of 29
        l.split("b", "d", Dir::Horizontal, false, Some(Amount::Cells(500)))
            .unwrap();
        assert_eq!(l.rect("b").unwrap().cols, MIN_COLS);
        check(&l);
    }

    #[test]
    fn a_pane_too_small_to_split_is_refused_and_unchanged() {
        let mut l = Layout::new("a", (4, 2));
        let before = l.clone();
        assert_eq!(
            l.split("a", "b", Dir::Horizontal, false, None),
            Err(Refused::NoSpace { need: 5, have: 4 })
        );
        assert_eq!(
            l.split("a", "b", Dir::Vertical, false, None),
            Err(Refused::NoSpace { need: 5, have: 2 })
        );
        assert_eq!(l, before);
        assert_eq!(
            l.split("x", "b", Dir::Vertical, false, None),
            Err(Refused::NoPane)
        );
    }

    #[test]
    fn repeated_deep_splits_stop_at_the_minimum_and_stay_consistent() {
        let mut l = Layout::new("p0", (200, 60));
        let mut n = 1;
        let mut target = "p0".to_string();
        loop {
            let dir = if n % 2 == 0 {
                Dir::Vertical
            } else {
                Dir::Horizontal
            };
            let new = format!("p{n}");
            match l.split(&target, &new, dir, false, None) {
                Ok(()) => {
                    check(&l);
                    target = new;
                    n += 1;
                }
                Err(Refused::NoSpace { .. }) => break,
                Err(e) => panic!("{e:?}"),
            }
        }
        assert!(n >= 9, "only {n} splits");
        // Removing them all, deepest first, gives the whole window back to p0.
        for k in (1..n).rev() {
            l.remove(&format!("p{k}")).unwrap();
            check(&l);
        }
        assert_eq!(l.rects(), [("p0".into(), r(0, 0, 200, 60))]);
        assert_eq!(l.root(), &Node::Pane("p0".into()));
    }

    #[test]
    fn removing_a_pane_gives_its_space_to_a_neighbour_and_collapses_splits() {
        let mut l = Layout::new("a", (81, 24));
        l.split("a", "b", Dir::Horizontal, false, None).unwrap();
        l.split("b", "c", Dir::Vertical, false, None).unwrap();
        l.remove("a").unwrap();
        // (b / c) fills the window: the split of two became the root.
        assert_eq!(
            rects(&l),
            [
                ("b".into(), r(0, 0, 81, 12)),
                ("c".into(), r(0, 13, 81, 11))
            ]
        );
        l.remove("c").unwrap();
        assert_eq!(rects(&l), [("b".into(), r(0, 0, 81, 24))]);
        assert_eq!(l.remove("b"), Err(Refused::NoPane), "the last pane stays");
    }

    #[test]
    fn a_removal_that_leaves_same_direction_splits_flattens_them() {
        let mut l = Layout::new("a", (100, 30));
        l.split("a", "b", Dir::Horizontal, false, None).unwrap();
        l.split("b", "c", Dir::Vertical, false, None).unwrap();
        l.split("c", "d", Dir::Horizontal, false, None).unwrap();
        // a | (b / (c | d)); removing b leaves a | c | d in one split.
        l.remove("b").unwrap();
        let j = l.json();
        assert_eq!(j["direction"], "horizontal");
        assert_eq!(j["children"].as_array().unwrap().len(), 3);
        check(&l);
    }

    #[test]
    fn resizing_the_window_keeps_proportions_and_the_minimum() {
        let mut l = Layout::new("a", (81, 24));
        l.split("a", "b", Dir::Horizontal, false, Some(Amount::Percent(25)))
            .unwrap();
        l.resize((161, 48));
        let (a, b) = (l.rect("a").unwrap(), l.rect("b").unwrap());
        assert_eq!(a.cols + b.cols + 1, 161);
        assert!((119..=121).contains(&a.cols), "{a:?}");
        assert_eq!((a.rows, b.rows), (48, 48));
        check(&l);
        // Far too small: laid out at its minimum, never below it.
        l.resize((1, 1));
        assert_eq!(l.size(), l.min_size());
        assert_eq!(l.size(), (2 * MIN_COLS + 1, MIN_ROWS));
        check(&l);
        l.resize((81, 24));
        check(&l);
        assert_eq!(
            l.rect("a").unwrap().cols + l.rect("b").unwrap().cols + 1,
            81
        );
    }

    #[test]
    fn adjust_moves_the_border_towards_the_side_or_shrinks_at_the_edge() {
        let mut l = Layout::new("a", (81, 24));
        l.split("a", "b", Dir::Horizontal, false, None).unwrap();
        // a grows to the right.
        assert_eq!(l.adjust("a", Side::Right, 5), Ok(5));
        assert_eq!(
            (l.rect("a").unwrap().cols, l.rect("b").unwrap().cols),
            (45, 35)
        );
        // b is at the right edge: -R moves its left border right (b shrinks).
        assert_eq!(l.adjust("b", Side::Right, 5), Ok(5));
        assert_eq!(l.rect("b").unwrap().cols, 30);
        // b grows to the left.
        assert_eq!(l.adjust("b", Side::Left, 10), Ok(10));
        assert_eq!(l.rect("b").unwrap().cols, 40);
        // No vertical border: nothing moves.
        assert_eq!(l.adjust("a", Side::Down, 3), Ok(0));
        // Never below the minimum.
        assert_eq!(l.adjust("a", Side::Right, 500), Ok(40 - MIN_COLS));
        assert_eq!(l.rect("b").unwrap().cols, MIN_COLS);
        check(&l);
    }

    #[test]
    fn adjust_reaches_the_border_of_an_outer_split() {
        let mut l = Layout::new("a", (81, 25));
        l.split("a", "b", Dir::Vertical, false, None).unwrap();
        l.split("a", "c", Dir::Horizontal, false, None).unwrap();
        // (a | c) / b: a's bottom border belongs to the outer vertical split.
        let before = l.rect("b").unwrap().rows;
        assert_eq!(l.adjust("a", Side::Down, 3), Ok(3));
        assert_eq!(l.rect("b").unwrap().rows, before - 3);
        assert_eq!(l.rect("c").unwrap().rows, l.rect("a").unwrap().rows);
        // a's right border belongs to the inner split.
        assert_eq!(l.adjust("a", Side::Right, 2), Ok(2));
        assert_eq!(l.rect("b").unwrap().cols, 81, "b spans the window");
        check(&l);
    }

    #[test]
    fn swap_exchanges_places_and_zoom_fills_the_window_until_unzoomed() {
        let mut l = Layout::new("a", (81, 24));
        l.split("a", "b", Dir::Horizontal, false, Some(Amount::Cells(20)))
            .unwrap();
        l.swap("a", "b").unwrap();
        assert_eq!(l.panes(), ["b", "a"]);
        assert_eq!(l.rect("b").unwrap(), r(0, 0, 60, 24));
        l.zoom(Some("a")).unwrap();
        assert_eq!(l.zoomed(), Some("a"));
        assert_eq!(l.rect("a").unwrap(), r(0, 0, 81, 24));
        assert!(l.visible("a") && !l.visible("b"));
        // The others keep their places while hidden.
        assert_eq!(l.rect("b").unwrap(), r(0, 0, 60, 24));
        check(&l);
        l.zoom(None).unwrap();
        assert_eq!(l.rect("a").unwrap(), r(61, 0, 20, 24), "zoom restores");
        // A split or a resize of the layout unzooms.
        l.zoom(Some("a")).unwrap();
        l.split("b", "c", Dir::Vertical, false, None).unwrap();
        assert_eq!(l.zoomed(), None);
        // One pane cannot be zoomed.
        let mut one = Layout::new("x", (10, 10));
        one.zoom(Some("x")).unwrap();
        assert_eq!(one.zoomed(), None);
        assert_eq!(l.swap("a", "zz"), Err(Refused::NoPane));
    }

    #[test]
    fn rotate_moves_panes_through_the_places() {
        let mut l = Layout::new("a", (81, 24));
        l.split("a", "b", Dir::Horizontal, false, Some(Amount::Cells(20)))
            .unwrap();
        l.split("b", "c", Dir::Vertical, false, None).unwrap();
        let places: Vec<Rect> = l.rects().into_iter().map(|r| r.1).collect();
        l.rotate(true);
        assert_eq!(l.panes(), ["b", "c", "a"]);
        assert_eq!(
            l.rects().into_iter().map(|r| r.1).collect::<Vec<_>>(),
            places,
            "the places stay"
        );
        l.rotate(false);
        assert_eq!(l.panes(), ["a", "b", "c"]);
        let mut one = Layout::new("x", (10, 10));
        one.rotate(true);
        assert_eq!(one.panes(), ["x"]);
    }

    #[test]
    fn presets_share_the_window_equally() {
        let mut l = Layout::new("a", (100, 30));
        l.split("a", "b", Dir::Horizontal, false, Some(Amount::Cells(10)))
            .unwrap();
        l.split("b", "c", Dir::Vertical, false, None).unwrap();
        l.split("a", "d", Dir::Vertical, false, None).unwrap();
        l.zoom(Some("a")).unwrap();
        l.preset(Preset::EvenHorizontal);
        assert_eq!(l.zoomed(), None);
        let w: Vec<u16> = l.rects().iter().map(|r| r.1.cols).collect();
        assert_eq!(w.iter().sum::<u16>() + 3, 100);
        assert!(
            w.iter().max().unwrap() - w.iter().min().unwrap() <= 1,
            "{w:?}"
        );
        l.preset(Preset::EvenVertical);
        let h: Vec<u16> = l.rects().iter().map(|r| r.1.rows).collect();
        assert_eq!(h.iter().sum::<u16>() + 3, 30);
        l.preset(Preset::Tiled);
        // Four panes: two rows of two.
        let rs = l.rects();
        assert_eq!(rs.iter().filter(|r| r.1.y == 0).count(), 2);
        assert_eq!(l.panes(), ["a", "d", "b", "c"]);
        check(&l);
        // Five panes in a small window: three columns, never below the minimum.
        l.split("c", "e", Dir::Vertical, false, None).unwrap();
        l.preset(Preset::Tiled);
        check(&l);
        assert_eq!(Preset::parse("tiled"), Some(Preset::Tiled));
        assert_eq!(Preset::parse("spiral"), None);
    }

    #[test]
    fn main_presets_cycle_and_spread() {
        let mut l = Layout::new("a", (200, 60));
        l.split("a", "b", Dir::Horizontal, false, None).unwrap();
        l.split("b", "c", Dir::Vertical, false, None).unwrap();
        l.preset(Preset::MainVertical);
        assert_eq!(l.rect("a").unwrap(), r(0, 0, 80, 60), "main-pane-width");
        assert_eq!(l.rect("b").unwrap().x, 81);
        assert_eq!(l.rect("b").unwrap().cols, 119);
        check(&l);
        l.preset(Preset::MainVerticalMirrored);
        assert_eq!(l.rect("a").unwrap().x, 120);
        l.preset(Preset::MainHorizontal);
        assert_eq!(l.rect("a").unwrap(), r(0, 0, 200, 24), "main-pane-height");
        assert_eq!(l.rect("b").unwrap().y, 25);
        l.preset(Preset::MainHorizontalMirrored);
        assert_eq!(l.rect("a").unwrap().y, 36);
        check(&l);
        // next-layout follows tmux's order and wraps.
        assert_eq!(l.cycle(true), Preset::MainVertical);
        assert_eq!(l.cycle(true), Preset::MainVerticalMirrored);
        assert_eq!(l.cycle(true), Preset::Tiled);
        assert_eq!(l.cycle(true), Preset::EvenHorizontal);
        assert_eq!(l.cycle(false), Preset::Tiled);
        // A small window: the main pane gives way so the others keep their minimum.
        let mut s = Layout::new("a", (20, 8));
        s.split("a", "b", Dir::Horizontal, false, None).unwrap();
        s.preset(Preset::MainVertical);
        check(&s);
        assert!(s.rect("b").unwrap().cols >= MIN_COLS);
        // Spread: every split equal.
        let mut e = Layout::new("a", (101, 30));
        e.split("a", "b", Dir::Horizontal, false, Some(Amount::Cells(10)))
            .unwrap();
        e.split("a", "c", Dir::Horizontal, false, Some(Amount::Cells(10)))
            .unwrap();
        e.spread();
        let w: Vec<u16> = e.rects().iter().map(|x| x.1.cols).collect();
        assert!(
            w.iter().max().unwrap() - w.iter().min().unwrap() <= 1,
            "{w:?}"
        );
        assert_eq!(
            Preset::parse("main-horizontal-mirrored"),
            Some(Preset::MainHorizontalMirrored)
        );
    }

    #[test]
    fn neighbours_follow_the_geometry_and_wrap_around() {
        // a | (b / c)
        let mut l = Layout::new("a", (81, 25));
        l.split("a", "b", Dir::Horizontal, false, None).unwrap();
        l.split("b", "c", Dir::Vertical, false, None).unwrap();
        assert_eq!(l.neighbour("a", Side::Right).as_deref(), Some("b"));
        assert_eq!(l.neighbour("c", Side::Left).as_deref(), Some("a"));
        assert_eq!(l.neighbour("b", Side::Down).as_deref(), Some("c"));
        assert_eq!(l.neighbour("c", Side::Up).as_deref(), Some("b"));
        // Wraps: right of b is a again, down from c is b.
        assert_eq!(l.neighbour("b", Side::Right).as_deref(), Some("a"));
        assert_eq!(l.neighbour("c", Side::Down).as_deref(), Some("b"));
        // a spans both rows: nothing above or below it but itself.
        assert_eq!(l.neighbour("a", Side::Up), None);
        let one = Layout::new("x", (10, 10));
        assert_eq!(one.neighbour("x", Side::Left), None);
    }

    #[test]
    fn the_json_tree_matches_the_contract() {
        let mut l = Layout::new("p-1", (81, 24));
        assert_eq!(l.json(), json!({"type": "pane", "pane": "p-1"}));
        l.split("p-1", "p-2", Dir::Horizontal, false, None).unwrap();
        let j = l.json();
        assert_eq!(j["children"][0]["cells"], 40);
        assert_eq!(j["children"][0]["node"]["pane"], "p-1");
        let share = j["children"][0]["size"].as_f64().unwrap();
        assert!((0.49..0.5).contains(&share), "{share}");
    }
}
