//! Initial desktop composition, seeded as movable windows at tiled positions.
use super::{Axis, Desktop, Kind, Node, Rect, WORKSPACES, Workspace};
use std::collections::{HashMap, HashSet};

impl Desktop {
    pub fn new(screen: Rect, usable: Rect) -> Self {
        let mut desktop = Self {
            ws: (0..WORKSPACES).map(|_| Workspace::empty()).collect(),
            cur: 0,
            prev: 0,
            next_split: 3,
            usable,
            screen,
            float_rect: HashMap::new(),
            collapsed: HashSet::new(),
        };
        // Settings fills the left half. ANGI sits above CPU and Memory on the
        // right. Explicit axes keep the same composition at every aspect ratio.
        desktop.ws[0].tree = Some(Node::Split {
            id: 0,
            axis: Axis::Horizontal,
            ratio: 0.5,
            a: Box::new(Node::Leaf(Kind::Settings)),
            b: Box::new(Node::Split {
                id: 1,
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(Node::Leaf(Kind::Overview)),
                b: Box::new(Node::Split {
                    id: 2,
                    axis: Axis::Horizontal,
                    ratio: 0.5,
                    a: Box::new(Node::Leaf(Kind::Cpu)),
                    b: Box::new(Node::Leaf(Kind::Memory)),
                }),
            }),
        });
        desktop.ws[0].z = vec![Kind::Settings, Kind::Overview, Kind::Cpu, Kind::Memory];
        desktop.ws[0].focus = Some(Kind::Memory);
        for (i, kind) in [
            Kind::AgentAngie,
            Kind::TerminalWindow(0),
            Kind::Web,
            Kind::System,
        ]
        .into_iter()
        .enumerate()
        {
            desktop.cur = i + 1;
            desktop.open(kind);
        }
        desktop.ws[2].tree = Some(Node::Split {
            id: desktop.next_split,
            axis: Axis::Horizontal,
            ratio: 0.5,
            a: Box::new(Node::Leaf(Kind::TerminalWindow(0))),
            b: Box::new(Node::Split {
                id: desktop.next_split + 1,
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(Node::Leaf(Kind::TerminalWindow(1))),
                b: Box::new(Node::Leaf(Kind::TerminalWindow(2))),
            }),
        });
        desktop.next_split += 2;
        desktop.ws[2].z = (0..3).map(Kind::TerminalWindow).collect();
        desktop.ws[2].focus = Some(Kind::TerminalWindow(0));
        // Seed each workspace from its tree before switching to windowed mode.
        for i in 0..WORKSPACES {
            desktop.cur = i;
            if i != 2 {
                desktop.cycle_layout();
            }
        }
        desktop.cur = 0;
        desktop
    }
}
