//! Window management modelled on Omarchy's Hyprland setup: dwindle (binary
//! split) tiling, 10px outer and 5px inner gaps, directional focus and swap,
//! per-window floating, fullscreen and maximize, workspaces, and a floating
//! "window mode" for a whole workspace. Pure logic: no UI types, fully tested.
use std::collections::HashMap;

pub const GAP_OUT: f32 = 10.0;
pub const GAP_IN: f32 = 5.0;
/// Title bar height inside a window frame; the Slint frame uses the same value.
pub const HEADER: f32 = 28.0;
pub const BORDER: f32 = 2.0;
/// Width of each title bar button (float, fullscreen, close), from the right.
pub const BTN_W: f32 = 26.0;
pub const MIN_W: f32 = 220.0;
pub const MIN_H: f32 = 120.0;
pub const WORKSPACES: usize = 5;
/// Half-width of the grab band around the gap between two tiles.
const GRAB: f32 = 8.0;
const EDGE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}
/// `Horizontal` puts the two children side by side; `Vertical` stacks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Tiled,
    /// Omarchy's floating workspace: every window floats with a title bar.
    Floating,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Full {
    /// Covers the whole screen, bars included.
    Screen,
    /// Fills the workspace area ("full width" in Omarchy).
    Maximized,
}

/// Each panel exists once, so its kind doubles as the window id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Overview,
    Cpu,
    Memory,
    Network,
    Hardware,
    Services,
    System,
}
impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Overview,
        Kind::Cpu,
        Kind::Memory,
        Kind::Network,
        Kind::Hardware,
        Kind::Services,
        Kind::System,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn title(self) -> &'static str {
        match self {
            Kind::Overview => "AGX",
            Kind::Cpu => "CPU",
            Kind::Memory => "Memory and storage",
            Kind::Network => "Network",
            Kind::Hardware => "Hardware",
            Kind::Services => "Services",
            Kind::System => "System",
        }
    }
}

#[derive(Clone, Debug)]
enum Node {
    Leaf(Kind),
    Split {
        id: u32,
        axis: Axis,
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Handle {
    pub split: u32,
    pub axis: Axis,
    /// The cell the split divides; used to turn pointer position into a ratio.
    pub region: Rect,
    /// Where the gap sits: an x for side-by-side splits, a y for stacked ones.
    pub pos: f32,
}

fn children(cell: Rect, axis: Axis, ratio: f32) -> (Rect, Rect) {
    match axis {
        Axis::Horizontal => (
            Rect::new(cell.x, cell.y, cell.w * ratio, cell.h),
            Rect::new(cell.x + cell.w * ratio, cell.y, cell.w * (1.0 - ratio), cell.h),
        ),
        Axis::Vertical => (
            Rect::new(cell.x, cell.y, cell.w, cell.h * ratio),
            Rect::new(cell.x, cell.y + cell.h * ratio, cell.w, cell.h * (1.0 - ratio)),
        ),
    }
}

impl Node {
    fn contains(&self, k: Kind) -> bool {
        match self {
            Node::Leaf(x) => *x == k,
            Node::Split { a, b, .. } => a.contains(k) || b.contains(k),
        }
    }
    fn layout(&self, cell: Rect, out: &mut Vec<(Kind, Rect)>, handles: &mut Vec<Handle>) {
        match self {
            Node::Leaf(k) => out.push((*k, cell.inset(GAP_IN))),
            Node::Split { id, axis, ratio, a, b } => {
                let (ca, cb) = children(cell, *axis, *ratio);
                let pos = match axis {
                    Axis::Horizontal => cell.x + cell.w * ratio,
                    Axis::Vertical => cell.y + cell.h * ratio,
                };
                handles.push(Handle { split: *id, axis: *axis, region: cell, pos });
                a.layout(ca, out, handles);
                b.layout(cb, out, handles);
            }
        }
    }
    fn remove(self, k: Kind) -> Option<Node> {
        match self {
            Node::Leaf(x) => (x != k).then_some(Node::Leaf(x)),
            Node::Split { id, axis, ratio, a, b } => match (a.remove(k), b.remove(k)) {
                (Some(a), Some(b)) => Some(Node::Split {
                    id,
                    axis,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            },
        }
    }
    /// Split the leaf `target`, putting `new` after it.
    fn insert_at(&mut self, target: Kind, new: Kind, axis: Axis, next_id: &mut u32, before: bool) -> bool {
        match self {
            Node::Leaf(x) if *x == target => {
                *next_id += 1;
                let (old, fresh) = (Box::new(Node::Leaf(target)), Box::new(Node::Leaf(new)));
                let (a, b) = if before { (fresh, old) } else { (old, fresh) };
                *self = Node::Split { id: *next_id, axis, ratio: 0.5, a, b };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => {
                a.insert_at(target, new, axis, next_id, before) || b.insert_at(target, new, axis, next_id, before)
            }
        }
    }
    fn swap_leaves(&mut self, p: Kind, q: Kind) {
        match self {
            Node::Leaf(x) => {
                if *x == p {
                    *x = q;
                } else if *x == q {
                    *x = p;
                }
            }
            Node::Split { a, b, .. } => {
                a.swap_leaves(p, q);
                b.swap_leaves(p, q);
            }
        }
    }
    /// Flip the split that directly holds `k` (Hyprland's togglesplit).
    fn toggle_parent(&mut self, k: Kind) -> bool {
        if let Node::Split { axis, a, b, .. } = self {
            let direct = matches!(**a, Node::Leaf(x) if x == k) || matches!(**b, Node::Leaf(x) if x == k);
            if direct {
                *axis = if *axis == Axis::Horizontal { Axis::Vertical } else { Axis::Horizontal };
                return true;
            }
            return a.toggle_parent(k) || b.toggle_parent(k);
        }
        false
    }
    fn set_ratio(&mut self, target: u32, r: f32) -> bool {
        if let Node::Split { id, ratio, a, b, .. } = self {
            if *id == target {
                *ratio = r.clamp(0.1, 0.9);
                return true;
            }
            return a.set_ratio(target, r) || b.set_ratio(target, r);
        }
        false
    }
    /// Deepest split above `k` that divides along `axis`: (id, k is first child, cell length).
    fn find_split(&self, cell: Rect, k: Kind, axis: Axis) -> Option<(u32, bool, f32)> {
        let Node::Split { id, axis: ax, ratio, a, b } = self else { return None };
        let (ca, cb) = children(cell, *ax, *ratio);
        let (child, child_cell, in_a) = if a.contains(k) {
            (a, ca, true)
        } else if b.contains(k) {
            (b, cb, false)
        } else {
            return None;
        };
        if let Some(found) = child.find_split(child_cell, k, axis) {
            return Some(found);
        }
        (*ax == axis).then(|| {
            let len = if axis == Axis::Horizontal { cell.w } else { cell.h };
            (*id, in_a, len)
        })
    }
    fn ratio_of(&self, target: u32) -> Option<f32> {
        if let Node::Split { id, ratio, a, b, .. } = self {
            if *id == target {
                return Some(*ratio);
            }
            return a.ratio_of(target).or_else(|| b.ratio_of(target));
        }
        None
    }
}

#[derive(Clone, Debug)]
struct Workspace {
    tree: Option<Node>,
    /// Windows floated one by one (Super+T); they are not in the tree.
    floats: Vec<Kind>,
    /// Every window here, back to front.
    z: Vec<Kind>,
    mode: Mode,
    focus: Option<Kind>,
    full: Option<(Kind, Full)>,
}
impl Workspace {
    fn empty() -> Self {
        Self { tree: None, floats: vec![], z: vec![], mode: Mode::Tiled, focus: None, full: None }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub kind: Kind,
    pub rect: Rect,
    pub floating: bool,
    pub focused: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// Back to front.
    pub windows: Vec<Placed>,
    pub handles: Vec<Handle>,
    /// True when a window covers the whole screen and the bars should hide.
    pub screen_full: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Float,
    Full,
    Close,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}
#[derive(Clone, Copy, Debug)]
pub enum Hit {
    None,
    Button(Kind, Button),
    Header(Kind),
    Edge(Kind, Edges),
    Handle(Handle),
    Body(Kind),
}

pub struct Desktop {
    ws: Vec<Workspace>,
    cur: usize,
    prev: usize,
    next_split: u32,
    usable: Rect,
    screen: Rect,
    float_rect: HashMap<Kind, Rect>,
}

impl Desktop {
    pub fn new(screen: Rect, usable: Rect) -> Self {
        let mut d = Self {
            ws: (0..WORKSPACES).map(|_| Workspace::empty()).collect(),
            cur: 0,
            prev: 0,
            next_split: 0,
            usable,
            screen,
            float_rect: HashMap::new(),
        };
        // Overview on the left, CPU above memory on the right, then one
        // panel per remaining workspace.
        d.cur = 0;
        d.open(Kind::Overview);
        d.open(Kind::Cpu);
        d.ws[0].focus = Some(Kind::Cpu);
        d.open(Kind::Memory);
        d.ws[0].focus = Some(Kind::Overview);
        d.set_ratio_for(Kind::Overview, 0.55);
        for (i, k) in [Kind::Network, Kind::Hardware, Kind::Services, Kind::System].into_iter().enumerate() {
            d.cur = i + 1;
            d.open(k);
        }
        d.cur = 0;
        d
    }

    pub fn set_area(&mut self, screen: Rect, usable: Rect) {
        self.screen = screen;
        self.usable = usable;
    }
    pub fn current(&self) -> usize {
        self.cur
    }
    pub fn mode(&self) -> Mode {
        self.ws[self.cur].mode
    }
    pub fn focused(&self) -> Option<Kind> {
        self.ws[self.cur].focus
    }
    pub fn occupied(&self, i: usize) -> bool {
        self.ws.get(i).is_some_and(|w| !w.z.is_empty())
    }
    pub fn is_open(&self, k: Kind) -> bool {
        self.ws.iter().any(|w| w.z.contains(&k))
    }
    pub fn is_floating(&self, k: Kind) -> bool {
        let w = &self.ws[self.cur];
        w.mode == Mode::Floating || w.floats.contains(&k)
    }
    pub fn fullscreen_of(&self) -> Option<(Kind, Full)> {
        self.ws[self.cur].full
    }

    fn tiled(&self, w: &Workspace) -> (Vec<(Kind, Rect)>, Vec<Handle>) {
        let (mut out, mut handles) = (vec![], vec![]);
        if let Some(tree) = &w.tree {
            tree.layout(self.usable.inset(GAP_OUT - GAP_IN), &mut out, &mut handles);
        }
        (out, handles)
    }

    fn default_float(&self, n: usize) -> Rect {
        let (w, h) = (self.usable.w * 0.6, self.usable.h * 0.6);
        let off = 30.0 * (n % 6) as f32;
        Rect::new(self.usable.x + 40.0 + off, self.usable.y + 30.0 + off, w, h)
    }

    pub fn scene(&self) -> Scene {
        let w = &self.ws[self.cur];
        if let Some((k, f)) = w.full {
            let rect = match f {
                Full::Screen => self.screen,
                Full::Maximized => self.usable.inset(GAP_OUT),
            };
            return Scene {
                windows: vec![Placed { kind: k, rect, floating: false, focused: true }],
                handles: vec![],
                screen_full: f == Full::Screen,
            };
        }
        let (tiled, handles) = self.tiled(w);
        let tiled: HashMap<Kind, Rect> = tiled.into_iter().collect();
        let mut windows = vec![];
        let focused = |k| w.focus == Some(k);
        match w.mode {
            Mode::Tiled => {
                for k in &w.z {
                    if let Some(r) = tiled.get(k) {
                        windows.push(Placed { kind: *k, rect: *r, floating: false, focused: focused(*k) });
                    }
                }
                for k in &w.z {
                    if w.floats.contains(k) {
                        let rect = self.float_rect.get(k).copied().unwrap_or_else(|| self.default_float(0));
                        windows.push(Placed { kind: *k, rect, floating: true, focused: focused(*k) });
                    }
                }
                Scene { windows, handles, screen_full: false }
            }
            Mode::Floating => {
                for (n, k) in w.z.iter().enumerate() {
                    let rect = self
                        .float_rect
                        .get(k)
                        .copied()
                        .or_else(|| tiled.get(k).copied())
                        .unwrap_or_else(|| self.default_float(n));
                    windows.push(Placed { kind: *k, rect, floating: true, focused: focused(*k) });
                }
                Scene { windows, handles: vec![], screen_full: false }
            }
        }
    }

    pub fn rect_of(&self, k: Kind) -> Option<Rect> {
        if self.fullscreen_of().is_none() && self.is_floating(k)
            && let Some(rect) = self.float_rect.get(&k)
        {
            return Some(*rect);
        }
        self.scene().rect_of(k)
    }

    // --- focus ---------------------------------------------------------
    pub fn focus(&mut self, k: Kind) {
        let w = &mut self.ws[self.cur];
        if w.z.contains(&k) {
            w.focus = Some(k);
            w.z.retain(|x| *x != k);
            w.z.push(k);
        }
    }
    pub fn neighbor(&self, from: Kind, dir: Dir) -> Option<Kind> {
        let scene = self.scene();
        let me = scene.windows.iter().find(|p| p.kind == from)?;
        let (cx, cy) = me.rect.center();
        scene
            .windows
            .iter()
            .filter(|p| p.kind != from)
            .filter_map(|p| {
                let (ox, oy) = p.rect.center();
                let (along, across) = match dir {
                    Dir::Left => (cx - ox, (cy - oy).abs()),
                    Dir::Right => (ox - cx, (cy - oy).abs()),
                    Dir::Up => (cy - oy, (cx - ox).abs()),
                    Dir::Down => (oy - cy, (cx - ox).abs()),
                };
                (along > 1.0).then_some((along + 2.0 * across, p.kind))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, k)| k)
    }
    pub fn focus_dir(&mut self, dir: Dir) {
        if let Some(f) = self.focused()
            && let Some(n) = self.neighbor(f, dir)
        {
            self.focus(n);
        }
    }
    pub fn cycle_focus(&mut self, back: bool) {
        let mut order: Vec<Kind> = self.ws[self.cur].z.clone();
        order.sort_by_key(|k| k.index());
        let Some(f) = self.focused() else { return };
        let Some(pos) = order.iter().position(|k| *k == f) else { return };
        let n = order.len();
        let next = if back { (pos + n - 1) % n } else { (pos + 1) % n };
        self.focus(order[next]);
    }

    // --- open, close, move ----------------------------------------------
    /// Open (or bring here) a panel on the current workspace.
    pub fn open(&mut self, k: Kind) {
        if self.ws[self.cur].z.contains(&k) {
            self.focus(k);
            return;
        }
        self.detach(k);
        self.attach(self.cur, k);
        self.focus(k);
    }
    pub fn close(&mut self, k: Kind) {
        self.detach(k);
    }
    /// Remove a panel from whatever workspace holds it.
    fn detach(&mut self, k: Kind) {
        for w in &mut self.ws {
            if !w.z.contains(&k) {
                continue;
            }
            w.tree = w.tree.take().and_then(|t| t.remove(k));
            w.floats.retain(|x| *x != k);
            w.z.retain(|x| *x != k);
            if w.full.is_some_and(|(f, _)| f == k) {
                w.full = None;
            }
            if w.focus == Some(k) {
                w.focus = w.z.last().copied();
            }
        }
    }
    fn attach(&mut self, ws: usize, k: Kind) {
        let n = self.ws[ws].z.len();
        let anchor = self.ws[ws].focus.filter(|f| self.ws[ws].tree.as_ref().is_some_and(|t| t.contains(*f)));
        let axis = anchor
            .and_then(|a| {
                let saved = self.cur;
                self.cur = ws;
                let r = self.rect_of(a);
                self.cur = saved;
                r
            })
            .map_or(Axis::Horizontal, |r| if r.w >= r.h { Axis::Horizontal } else { Axis::Vertical });
        let mut next = self.next_split;
        let w = &mut self.ws[ws];
        match (&mut w.tree, anchor) {
            (None, _) => w.tree = Some(Node::Leaf(k)),
            (Some(t), Some(a)) => {
                t.insert_at(a, k, axis, &mut next, false);
            }
            (Some(t), None) => {
                // No tiled focus: split the first leaf.
                let mut leaves = vec![];
                leaves_of(t, &mut leaves);
                t.insert_at(leaves[0], k, axis, &mut next, false);
            }
        }
        self.next_split = next;
        self.ws[ws].z.push(k);
        let fallback = self.default_float(n);
        self.float_rect.entry(k).or_insert(fallback);
    }
    pub fn move_to_workspace(&mut self, target: usize, follow: bool) {
        let Some(k) = self.focused() else { return };
        if target >= WORKSPACES || target == self.cur {
            return;
        }
        let was_float = self.ws[self.cur].floats.contains(&k);
        let rect = self.rect_of(k);
        self.detach(k);
        self.attach(target, k);
        if let Some(r) = rect {
            self.float_rect.insert(k, r);
        }
        if was_float {
            // Keep it floating on the other side too.
            let w = &mut self.ws[target];
            w.tree = w.tree.take().and_then(|t| t.remove(k));
            w.floats.push(k);
        }
        self.ws[target].focus = Some(k);
        if follow {
            self.goto(target);
        }
    }
    pub fn goto(&mut self, i: usize) {
        if i < WORKSPACES && i != self.cur {
            self.prev = self.cur;
            self.cur = i;
        }
    }
    pub fn next_workspace(&mut self, back: bool) {
        let n = WORKSPACES;
        self.goto(if back { (self.cur + n - 1) % n } else { (self.cur + 1) % n });
    }

    // --- layout operations ------------------------------------------------
    pub fn swap(&mut self, a: Kind, b: Kind) {
        let w = &mut self.ws[self.cur];
        if let Some(t) = &mut w.tree
            && t.contains(a)
            && t.contains(b)
        {
            t.swap_leaves(a, b);
        }
    }
    pub fn swap_dir(&mut self, dir: Dir) {
        if let Some(f) = self.focused()
            && let Some(n) = self.neighbor(f, dir)
        {
            self.swap(f, n);
        }
    }
    pub fn toggle_split(&mut self) {
        if let Some(f) = self.focused()
            && let Some(t) = &mut self.ws[self.cur].tree
        {
            t.toggle_parent(f);
        }
    }
    /// Super+T. Returns a note when the workspace is in window mode.
    pub fn toggle_float(&mut self) -> Option<&'static str> {
        if self.ws[self.cur].mode == Mode::Floating {
            return Some("Window mode: press the layout key to tile this workspace");
        }
        let f = self.focused()?;
        let rect = self.rect_of(f)?;
        if self.ws[self.cur].floats.contains(&f) {
            self.ws[self.cur].floats.retain(|x| *x != f);
            // Tile it back where it was dropped: beside the tile it overlaps most,
            // on the side it sits.
            let drop = self.drop_placement(rect);
            let mut n = self.next_split;
            let w = &mut self.ws[self.cur];
            match (&mut w.tree, drop) {
                (Some(t), Some((anchor, axis, before))) => {
                    t.insert_at(anchor, f, axis, &mut n, before);
                }
                _ => w.tree = Some(Node::Leaf(f)),
            }
            self.next_split = n;
        } else {
            self.float_rect.insert(f, rect);
            let w = &mut self.ws[self.cur];
            w.tree = w.tree.take().and_then(|t| t.remove(f));
            w.floats.push(f);
        }
        None
    }
    /// Where a floating window at `rect` would tile: (anchor tile, split axis, goes before).
    fn drop_placement(&self, rect: Rect) -> Option<(Kind, Axis, bool)> {
        let (tiled, _) = self.tiled(&self.ws[self.cur]);
        let overlap = |r: &Rect| {
            let w = (rect.right().min(r.right()) - rect.x.max(r.x)).max(0.0);
            let h = (rect.bottom().min(r.bottom()) - rect.y.max(r.y)).max(0.0);
            w * h
        };
        let (cx, cy) = rect.center();
        let dist = |r: &Rect| {
            let (ox, oy) = r.center();
            (cx - ox).hypot(cy - oy)
        };
        let (kind, target) = tiled.iter().copied().max_by(|(_, a), (_, b)| {
            overlap(a).total_cmp(&overlap(b)).then(dist(b).total_cmp(&dist(a)))
        })?;
        let (tx, ty) = target.center();
        let (dx, dy) = ((cx - tx) / target.w.max(1.0), (cy - ty) / target.h.max(1.0));
        Some(if dx.abs() >= dy.abs() { (kind, Axis::Horizontal, dx < 0.0) } else { (kind, Axis::Vertical, dy < 0.0) })
    }
    pub fn toggle_fullscreen(&mut self, kind: Full) {
        let Some(f) = self.focused() else { return };
        let w = &mut self.ws[self.cur];
        w.full = match w.full {
            Some((_, cur)) if cur == kind => None,
            _ => Some((f, kind)),
        };
    }
    /// Omarchy's workspace layout cycle: tiled, then window mode.
    pub fn cycle_layout(&mut self) {
        let (tiled, _) = self.tiled(&self.ws[self.cur]);
        for (k, r) in tiled {
            self.float_rect.insert(k, r);
        }
        let w = &mut self.ws[self.cur];
        w.full = None;
        w.mode = if w.mode == Mode::Tiled { Mode::Floating } else { Mode::Tiled };
    }

    // --- resizing ---------------------------------------------------------
    /// Keyboard resize by pixels. Tiled windows move their split; floating ones change size.
    pub fn resize(&mut self, dw: f32, dh: f32) {
        if let Some(f) = self.focused() {
            self.resize_window(f, dw, dh);
        }
    }
    pub fn resize_window(&mut self, k: Kind, dw: f32, dh: f32) {
        if self.is_floating(k) {
            let mut r = self.rect_of(k).unwrap_or(self.default_float(0));
            r.w = (r.w + dw).max(MIN_W);
            r.h = (r.h + dh).max(MIN_H);
            self.float_rect.insert(k, r);
            return;
        }
        let area = self.usable.inset(GAP_OUT - GAP_IN);
        for (delta, axis) in [(dw, Axis::Horizontal), (dh, Axis::Vertical)] {
            if delta == 0.0 {
                continue;
            }
            let found = self.ws[self.cur].tree.as_ref().and_then(|t| t.find_split(area, k, axis));
            if let Some((id, in_a, len)) = found
                && let Some(t) = &mut self.ws[self.cur].tree
                && let Some(r) = t.ratio_of(id)
            {
                let d = delta / len.max(1.0);
                t.set_ratio(id, if in_a { r + d } else { r - d });
            }
        }
    }
    fn set_ratio_for(&mut self, k: Kind, ratio: f32) {
        let area = self.usable.inset(GAP_OUT - GAP_IN);
        let found = self.ws[self.cur].tree.as_ref().and_then(|t| t.find_split(area, k, Axis::Horizontal));
        if let (Some((id, _, _)), Some(t)) = (found, &mut self.ws[self.cur].tree) {
            t.set_ratio(id, ratio);
        }
    }
    pub fn set_split(&mut self, split: u32, ratio: f32) {
        if let Some(t) = &mut self.ws[self.cur].tree {
            t.set_ratio(split, ratio);
        }
    }
    pub fn move_floating(&mut self, k: Kind, dx: f32, dy: f32) {
        if let Some(mut r) = self.rect_of(k) {
            r.x = (r.x + dx).clamp(self.screen.x - r.w + 80.0, self.screen.right() - 80.0);
            r.y = (r.y + dy).clamp(self.usable.y, self.screen.bottom() - HEADER);
            self.float_rect.insert(k, r);
        }
    }
    pub fn resize_floating_edges(&mut self, k: Kind, e: Edges, dx: f32, dy: f32) {
        let Some(mut r) = self.rect_of(k) else { return };
        if e.right {
            r.w = (r.w + dx).max(MIN_W);
        }
        if e.bottom {
            r.h = (r.h + dy).max(MIN_H);
        }
        if e.left {
            let w = (r.w - dx).max(MIN_W);
            r.x += r.w - w;
            r.w = w;
        }
        if e.top {
            let h = (r.h - dy).max(MIN_H);
            r.y += r.h - h;
            r.h = h;
        }
        self.float_rect.insert(k, r);
    }
    /// Split ratio that would put the gap under the pointer.
    #[cfg(test)]
    pub fn handle_ratio(&self, split: u32, pointer_x: f32, pointer_y: f32) -> Option<f32> {
        self.scene().handle_ratio(split, pointer_x, pointer_y)
    }

    // --- hit testing ----------------------------------------------------------
    #[cfg(test)]
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        self.scene().hit(x, y)
    }
}

impl Scene {
    pub fn handle_ratio(&self, split: u32, pointer_x: f32, pointer_y: f32) -> Option<f32> {
        let h = self.handles.iter().find(|h| h.split == split)?;
        Some(match h.axis {
            Axis::Horizontal => (pointer_x - h.region.x) / h.region.w.max(1.0),
            Axis::Vertical => (pointer_y - h.region.y) / h.region.h.max(1.0),
        })
    }

    pub fn rect_of(&self, k: Kind) -> Option<Rect> {
        self.windows.iter().find(|p| p.kind == k).map(|p| p.rect)
    }

    pub fn hit(&self, x: f32, y: f32) -> Hit {
        for p in self.windows.iter().rev().filter(|p| p.floating) {
            if let Some(h) = hit_window(p, x, y, true) {
                return h;
            }
        }
        for h in &self.handles {
            if let Some(hit) = handle_hit(h, x, y) {
                return hit;
            }
        }
        for p in self.windows.iter().rev() {
            if let Some(h) = hit_window(p, x, y, false) {
                return h;
            }
        }
        Hit::None
    }
    /// The tiled window under a point, for drop targets.
    pub fn tile_at(&self, x: f32, y: f32) -> Option<Kind> {
        self.windows.iter().find(|p| !p.floating && p.rect.contains(x, y)).map(|p| p.kind)
    }
}

fn leaves_of(n: &Node, out: &mut Vec<Kind>) {
    match n {
        Node::Leaf(k) => out.push(*k),
        Node::Split { a, b, .. } => {
            leaves_of(a, out);
            leaves_of(b, out);
        }
    }
}

fn handle_hit(h: &Handle, x: f32, y: f32) -> Option<Hit> {
    let r = h.region;
    let inside = match h.axis {
        Axis::Horizontal => (x - h.pos).abs() <= GRAB && y >= r.y && y < r.bottom(),
        Axis::Vertical => (y - h.pos).abs() <= GRAB && x >= r.x && x < r.right(),
    };
    inside.then_some(Hit::Handle(*h))
}

fn hit_window(p: &Placed, x: f32, y: f32, grab_edges: bool) -> Option<Hit> {
    let r = p.rect;
    if grab_edges {
        let outer = Rect::new(r.x - EDGE, r.y - EDGE, r.w + 2.0 * EDGE, r.h + 2.0 * EDGE);
        if !outer.contains(x, y) {
            return None;
        }
        let e = Edges {
            left: x < r.x + EDGE,
            right: x >= r.right() - EDGE,
            top: y < r.y + EDGE,
            bottom: y >= r.bottom() - EDGE,
        };
        if e != Edges::default() {
            return Some(Hit::Edge(p.kind, e));
        }
    }
    if !r.contains(x, y) {
        return None;
    }
    if y < r.y + HEADER {
        let from_right = r.right() - BORDER - x;
        if from_right >= 0.0 && y >= r.y + BORDER {
            let b = match (from_right / BTN_W) as i32 {
                0 => Some(Button::Close),
                1 => Some(Button::Full),
                2 => Some(Button::Float),
                _ => None,
            };
            if let Some(b) = b {
                return Some(Hit::Button(p.kind, b));
            }
        }
        return Some(Hit::Header(p.kind));
    }
    Some(Hit::Body(p.kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desk() -> Desktop {
        let screen = Rect::new(0.0, 0.0, 1200.0, 800.0);
        Desktop::new(screen, Rect::new(0.0, 34.0, 1200.0, 740.0))
    }
    fn rect(d: &Desktop, k: Kind) -> Rect {
        d.rect_of(k).unwrap()
    }

    #[test]
    fn default_workspace_is_overview_left_cpu_over_memory() {
        let d = desk();
        let (o, c, m) = (rect(&d, Kind::Overview), rect(&d, Kind::Cpu), rect(&d, Kind::Memory));
        assert!(o.right() <= c.x && o.right() <= m.x, "overview is left of the column");
        assert!(c.bottom() <= m.y, "cpu is above memory");
        assert_eq!(o.x, GAP_OUT);
        // Omarchy: 10px at the edge, 10px (2 x 5) between tiles.
        assert!((c.x - o.right() - 2.0 * GAP_IN).abs() < 0.01);
        assert!((m.y - c.bottom() - 2.0 * GAP_IN).abs() < 0.01);
    }

    #[test]
    fn opening_splits_the_focused_tile_along_its_long_side() {
        let mut d = desk();
        d.cur = 1; // only Network
        let before = rect(&d, Kind::Network);
        assert!(before.w > before.h);
        d.open(Kind::Hardware);
        let (n, h) = (rect(&d, Kind::Network), rect(&d, Kind::Hardware));
        assert!(n.right() <= h.x, "wide tile splits side by side");
        assert_eq!(d.focused(), Some(Kind::Hardware));
    }

    #[test]
    fn close_collapses_the_split() {
        let mut d = desk();
        d.close(Kind::Cpu);
        assert!(!d.is_open(Kind::Cpu));
        let m = rect(&d, Kind::Memory);
        assert!(m.h > 600.0, "memory takes the freed column");
    }

    #[test]
    fn directional_focus_and_swap() {
        let mut d = desk();
        d.focus(Kind::Overview);
        d.focus_dir(Dir::Right);
        assert!(matches!(d.focused(), Some(Kind::Cpu | Kind::Memory)));
        d.focus(Kind::Cpu);
        d.focus_dir(Dir::Down);
        assert_eq!(d.focused(), Some(Kind::Memory));
        d.focus_dir(Dir::Left);
        assert_eq!(d.focused(), Some(Kind::Overview));
        let (o, c) = (rect(&d, Kind::Overview), rect(&d, Kind::Cpu));
        d.swap(Kind::Overview, Kind::Cpu);
        assert_eq!(rect(&d, Kind::Overview), c);
        assert_eq!(rect(&d, Kind::Cpu), o);
    }

    #[test]
    fn toggle_split_flips_orientation() {
        let mut d = desk();
        d.focus(Kind::Cpu);
        let (c0, m0) = (rect(&d, Kind::Cpu), rect(&d, Kind::Memory));
        assert!(c0.bottom() <= m0.y);
        d.toggle_split();
        let (c1, m1) = (rect(&d, Kind::Cpu), rect(&d, Kind::Memory));
        assert!(c1.right() <= m1.x, "now side by side");
    }

    #[test]
    fn keyboard_resize_moves_the_split_from_either_side() {
        let mut d = desk();
        let w0 = rect(&d, Kind::Overview).w;
        d.focus(Kind::Overview);
        d.resize(100.0, 0.0);
        assert!((rect(&d, Kind::Overview).w - w0 - 100.0).abs() < 1.0);
        // The right-hand column grows by moving the same split left.
        d.focus(Kind::Cpu);
        let c0 = rect(&d, Kind::Cpu).w;
        d.resize(100.0, 0.0);
        assert!((rect(&d, Kind::Cpu).w - c0 - 100.0).abs() < 1.0);
    }

    #[test]
    fn float_and_tile_round_trip() {
        let mut d = desk();
        d.focus(Kind::Cpu);
        assert!(d.toggle_float().is_none());
        assert!(d.is_floating(Kind::Cpu));
        assert!(rect(&d, Kind::Memory).h > 600.0, "memory reflows");
        let s = d.scene();
        assert_eq!(s.windows.last().unwrap().kind, Kind::Cpu, "floating draws on top");
        d.toggle_float();
        assert!(!d.is_floating(Kind::Cpu));
    }

    #[test]
    fn untiling_a_float_returns_beside_the_tile_it_covers() {
        let mut d = desk();
        d.focus(Kind::Memory);
        d.toggle_float();
        // Memory floats where it was, over the lower right; CPU grows underneath.
        assert!(rect(&d, Kind::Cpu).h > 600.0);
        d.toggle_float();
        let (c, m) = (rect(&d, Kind::Cpu), rect(&d, Kind::Memory));
        assert!(c.bottom() <= m.y, "memory is back under cpu, not beside the overview");
        assert!(m.x > rect(&d, Kind::Overview).right());
    }

    #[test]
    fn ultrawide_screen_tiles_edge_to_edge_and_windows_can_go_right() {
        let mut d = desk();
        d.set_area(Rect::new(0.0, 0.0, 2560.0, 1080.0), Rect::new(0.0, 34.0, 2560.0, 1020.0));
        let right_edge = [Kind::Overview, Kind::Cpu, Kind::Memory]
            .iter()
            .map(|k| rect(&d, *k).right())
            .fold(0.0, f32::max);
        assert!((right_edge - (2560.0 - GAP_OUT)).abs() < 0.01, "tiles reach the right edge");
        d.cycle_layout();
        d.move_floating(Kind::Cpu, 1800.0, 0.0);
        assert!(rect(&d, Kind::Cpu).x > 1800.0, "no stale 1280px limit");
    }

    #[test]
    fn window_mode_floats_in_place_and_returns() {
        let mut d = desk();
        let before: Vec<_> = [Kind::Overview, Kind::Cpu, Kind::Memory].iter().map(|k| rect(&d, *k)).collect();
        d.cycle_layout();
        assert_eq!(d.mode(), Mode::Floating);
        let during: Vec<_> = [Kind::Overview, Kind::Cpu, Kind::Memory].iter().map(|k| rect(&d, *k)).collect();
        assert_eq!(before, during, "windows keep their place when floated");
        d.move_floating(Kind::Cpu, 50.0, 40.0);
        assert!(rect(&d, Kind::Cpu).x > before[1].x);
        assert!(d.scene().handles.is_empty());
        d.cycle_layout();
        assert_eq!(rect(&d, Kind::Cpu), before[1], "tiling restores the layout");
    }

    #[test]
    fn fullscreen_modes() {
        let mut d = desk();
        d.focus(Kind::Memory);
        d.toggle_fullscreen(Full::Screen);
        let s = d.scene();
        assert!(s.screen_full);
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].rect, Rect::new(0.0, 0.0, 1200.0, 800.0));
        d.toggle_fullscreen(Full::Screen);
        d.toggle_fullscreen(Full::Maximized);
        assert_eq!(d.scene().windows[0].rect, Rect::new(10.0, 44.0, 1180.0, 720.0));
    }

    #[test]
    fn move_window_between_workspaces() {
        let mut d = desk();
        d.focus(Kind::Cpu);
        d.move_to_workspace(2, false);
        assert_eq!(d.current(), 0);
        d.goto(2);
        assert!(d.rect_of(Kind::Cpu).is_some());
        assert!(d.rect_of(Kind::Hardware).is_some());
        d.goto(0);
        assert!(d.rect_of(Kind::Cpu).is_none());
        d.focus(Kind::Overview);
        d.move_to_workspace(3, true);
        assert_eq!(d.current(), 3);
    }

    #[test]
    fn closed_panels_reopen_on_the_current_workspace() {
        let mut d = desk();
        d.close(Kind::Network);
        assert!(!d.occupied(1));
        d.open(Kind::Network);
        assert!(d.rect_of(Kind::Network).is_some());
        assert!(d.occupied(0));
    }

    #[test]
    fn floating_edges_resize_and_clamp() {
        let mut d = desk();
        d.cycle_layout();
        let r0 = rect(&d, Kind::Cpu);
        d.resize_floating_edges(Kind::Cpu, Edges { right: true, bottom: true, ..Default::default() }, 30.0, 20.0);
        let r1 = rect(&d, Kind::Cpu);
        assert_eq!((r1.w, r1.h), (r0.w + 30.0, r0.h + 20.0));
        d.resize_floating_edges(Kind::Cpu, Edges { left: true, ..Default::default() }, 10_000.0, 0.0);
        assert_eq!(rect(&d, Kind::Cpu).w, MIN_W);
    }

    #[test]
    fn hit_testing_finds_buttons_headers_and_bodies() {
        let d = desk();
        let o = rect(&d, Kind::Overview);
        assert!(matches!(d.hit(o.x + 40.0, o.y + 10.0), Hit::Header(Kind::Overview)));
        assert!(matches!(d.hit(o.x + 40.0, o.y + 200.0), Hit::Body(Kind::Overview)));
        let close_x = o.right() - BORDER - 5.0;
        assert!(matches!(d.hit(close_x, o.y + 10.0), Hit::Button(Kind::Overview, Button::Close)));
        assert!(matches!(d.hit(close_x - BTN_W, o.y + 10.0), Hit::Button(Kind::Overview, Button::Full)));
    }

    #[test]
    fn dragging_a_gap_changes_the_split() {
        let mut d = desk();
        let o = rect(&d, Kind::Overview);
        let gap_x = o.right() + GAP_IN;
        let hit = d.hit(gap_x, 300.0);
        let Hit::Handle(h) = hit else { panic!("expected handle, got {hit:?}") };
        let ratio = d.handle_ratio(h.split, gap_x + 100.0, 300.0).unwrap();
        d.set_split(h.split, ratio);
        assert!((rect(&d, Kind::Overview).w - o.w - 100.0).abs() < 1.0);
    }
}
