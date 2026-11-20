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
        desktop.cur = 1;
        desktop.open(Kind::AgentAngi);
        if !crate::HEADLESS {
            desktop.seed_terminal_workspace();
            desktop.cur = 3;
            desktop.open(Kind::Web);
            desktop.cur = 4;
            desktop.open(Kind::System);
        }
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

    fn seed_terminal_workspace(&mut self) {
        self.ws[2].tree = Some(Node::Split {
            id: self.next_split,
            axis: Axis::Horizontal,
            ratio: 0.5,
            a: Box::new(Node::Leaf(Kind::TerminalWindow(0))),
            b: Box::new(Node::Split {
                id: self.next_split + 1,
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(Node::Leaf(Kind::TerminalWindow(1))),
                b: Box::new(Node::Leaf(Kind::TerminalWindow(2))),
            }),
        });
        self.next_split += 2;
        self.ws[2].z = (0..3).map(Kind::TerminalWindow).collect();
        self.ws[2].focus = Some(Kind::TerminalWindow(0));
    }
}
