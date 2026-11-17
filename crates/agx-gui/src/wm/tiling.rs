//! Binary split-tree layout and manipulation.
use super::{Axis, GAP_IN, Handle, Kind, Node, Rect};

fn children(cell: Rect, axis: Axis, ratio: f32) -> (Rect, Rect) {
    match axis {
        Axis::Horizontal => (
            Rect::new(cell.x, cell.y, cell.w * ratio, cell.h),
            Rect::new(
                cell.x + cell.w * ratio,
                cell.y,
                cell.w * (1.0 - ratio),
                cell.h,
            ),
        ),
        Axis::Vertical => (
            Rect::new(cell.x, cell.y, cell.w, cell.h * ratio),
            Rect::new(
                cell.x,
                cell.y + cell.h * ratio,
                cell.w,
                cell.h * (1.0 - ratio),
            ),
        ),
    }
}

impl Node {
    pub(super) fn contains(&self, k: Kind) -> bool {
        match self {
            Node::Leaf(x) => *x == k,
            Node::Split { a, b, .. } => a.contains(k) || b.contains(k),
        }
    }
    pub(super) fn layout(
        &self,
        cell: Rect,
        out: &mut Vec<(Kind, Rect)>,
        handles: &mut Vec<Handle>,
    ) {
        match self {
            Node::Leaf(k) => out.push((*k, cell.inset(GAP_IN))),
            Node::Split {
                id,
                axis,
                ratio,
                a,
                b,
            } => {
                let (ca, cb) = children(cell, *axis, *ratio);
                let pos = match axis {
                    Axis::Horizontal => cell.x + cell.w * ratio,
                    Axis::Vertical => cell.y + cell.h * ratio,
                };
                handles.push(Handle {
                    split: *id,
                    axis: *axis,
                    region: cell,
                    pos,
                });
                a.layout(ca, out, handles);
                b.layout(cb, out, handles);
            }
        }
    }
    pub(super) fn remove(self, k: Kind) -> Option<Node> {
        match self {
            Node::Leaf(x) => (x != k).then_some(Node::Leaf(x)),
            Node::Split {
                id,
                axis,
                ratio,
                a,
                b,
            } => match (a.remove(k), b.remove(k)) {
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
    pub(super) fn insert_at(
        &mut self,
        target: Kind,
        new: Kind,
        axis: Axis,
        next_id: &mut u32,
        before: bool,
    ) -> bool {
        match self {
            Node::Leaf(x) if *x == target => {
                *next_id += 1;
                let (old, fresh) = (Box::new(Node::Leaf(target)), Box::new(Node::Leaf(new)));
                let (a, b) = if before { (fresh, old) } else { (old, fresh) };
                *self = Node::Split {
                    id: *next_id,
                    axis,
                    ratio: 0.5,
                    a,
                    b,
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => {
                a.insert_at(target, new, axis, next_id, before)
                    || b.insert_at(target, new, axis, next_id, before)
            }
        }
    }
    pub(super) fn swap_leaves(&mut self, p: Kind, q: Kind) {
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
    pub(super) fn toggle_parent(&mut self, k: Kind) -> bool {
        if let Node::Split { axis, a, b, .. } = self {
            let direct =
                matches!(**a, Node::Leaf(x) if x == k) || matches!(**b, Node::Leaf(x) if x == k);
            if direct {
                *axis = if *axis == Axis::Horizontal {
                    Axis::Vertical
                } else {
                    Axis::Horizontal
                };
                return true;
            }
            return a.toggle_parent(k) || b.toggle_parent(k);
        }
        false
    }
    pub(super) fn set_ratio(&mut self, target: u32, r: f32) -> bool {
        if let Node::Split {
            id, ratio, a, b, ..
        } = self
        {
            if *id == target {
                *ratio = r.clamp(0.1, 0.9);
                return true;
            }
            return a.set_ratio(target, r) || b.set_ratio(target, r);
        }
        false
    }
    /// Deepest split above `k` that divides along `axis`: (id, k is first child, cell length).
    pub(super) fn find_split(&self, cell: Rect, k: Kind, axis: Axis) -> Option<(u32, bool, f32)> {
        let Node::Split {
            id,
            axis: ax,
            ratio,
            a,
            b,
        } = self
        else {
            return None;
        };
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
            let len = if axis == Axis::Horizontal {
                cell.w
            } else {
                cell.h
            };
            (*id, in_a, len)
        })
    }
    pub(super) fn ratio_of(&self, target: u32) -> Option<f32> {
        if let Node::Split {
            id, ratio, a, b, ..
        } = self
        {
            if *id == target {
                return Some(*ratio);
            }
            return a.ratio_of(target).or_else(|| b.ratio_of(target));
        }
        None
    }
}
