//! Tabs hold one pane or a split of two.

use crate::pane::Pane;

/// A tab: 1 pane, or 2 side by side.
#[derive(Debug, Clone)]
pub struct Tab {
    pub panes: Vec<Pane>,
    pub active: usize,
}

impl Tab {
    pub fn new(pane: Pane) -> Self {
        Tab {
            panes: vec![pane],
            active: 0,
        }
    }

    pub fn is_split(&self) -> bool {
        self.panes.len() > 1
    }

    pub fn pane(&self) -> Option<&Pane> {
        self.panes.get(self.active)
    }

    pub fn pane_mut(&mut self) -> Option<&mut Pane> {
        self.panes.get_mut(self.active)
    }

    pub fn other(&self) -> Option<&Pane> {
        if self.is_split() {
            self.panes.get(1 - self.active.min(1))
        } else {
            None
        }
    }

    pub fn title(&self) -> String {
        self.pane().map(|p| p.location.title()).unwrap_or_default()
    }
}
