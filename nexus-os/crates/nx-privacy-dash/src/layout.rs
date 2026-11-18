//! Pure layout tree and primitives for dashboard view-model snapshot testing.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// 2D rectangle in layout space with single-precision floating point coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }

    pub fn inset(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: (self.width - (2.0 * dx)).max(0.0),
            height: (self.height - (2.0 * dy)).max(0.0),
        }
    }

    /// Check if two rects match within a floating point tolerance.
    pub fn approx_eq(&self, other: &Rect, tolerance: f32) -> bool {
        (self.x - other.x).abs() <= tolerance
            && (self.y - other.y).abs() <= tolerance
            && (self.width - other.width).abs() <= tolerance
            && (self.height - other.height).abs() <= tolerance
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{:.1}, {:.1}, {:.1}x{:.1}]",
            self.x, self.y, self.width, self.height
        )
    }
}

/// Category of UI widget element in the pure layout tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WidgetKind {
    Window,
    Header,
    TabBar,
    TabButton,
    Section,
    Card,
    IndicatorChip,
    ToggleSwitch,
    Button,
    Table,
    TableHeader,
    TableRow,
    TableCell,
    Badge,
    Sparkline,
    Text,
    Divider,
}

impl fmt::Display for WidgetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A node in the deterministic layout tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutNode {
    pub id: String,
    pub kind: WidgetKind,
    pub rect: Rect,
    pub label: Option<String>,
    pub attributes: BTreeMap<String, String>,
    pub children: Vec<LayoutNode>,
}

impl LayoutNode {
    pub fn new(id: impl Into<String>, kind: WidgetKind, rect: Rect) -> Self {
        Self {
            id: id.into(),
            kind,
            rect,
            label: None,
            attributes: BTreeMap::new(),
            children: Vec::new(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_attr(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), val.into());
        self
    }

    pub fn add_child(&mut self, child: LayoutNode) {
        self.children.push(child);
    }

    /// Recursively find a node by its ID.
    pub fn find_node(&self, target_id: &str) -> Option<&LayoutNode> {
        if self.id == target_id {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find_node(target_id) {
                return Some(found);
            }
        }
        None
    }

    /// Format layout tree as structured indented text for golden snapshot tests.
    pub fn formatted_tree(&self, indent: usize) -> String {
        let pad = "  ".repeat(indent);
        let label_str = self
            .label
            .as_deref()
            .map(|l| format!(" \"{}\"", l))
            .unwrap_or_default();

        let mut out = format!(
            "{}- {} #{}: {}{}\n",
            pad, self.kind, self.id, self.rect, label_str
        );

        for child in &self.children {
            out.push_str(&child.formatted_tree(indent + 1));
        }

        out
    }

    /// Fuzzy comparison ensuring layout trees match in structure and within tolerance.
    pub fn approx_eq(&self, other: &LayoutNode, tolerance: f32) -> bool {
        if self.id != other.id || self.kind != other.kind || self.label != other.label {
            return false;
        }

        if !self.rect.approx_eq(&other.rect, tolerance) {
            return false;
        }

        if self.children.len() != other.children.len() {
            return false;
        }

        for (a, b) in self.children.iter().zip(&other.children) {
            if !a.approx_eq(b, tolerance) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_geometry_and_inset() {
        let r = Rect::new(10.0, 20.0, 100.0, 200.0);
        assert_eq!(r.right(), 110.0);
        assert_eq!(r.bottom(), 220.0);
        assert!(r.contains_point(15.0, 25.0));
        assert!(!r.contains_point(5.0, 25.0));

        let inset = r.inset(5.0, 10.0);
        assert_eq!(inset.x, 15.0);
        assert_eq!(inset.y, 30.0);
        assert_eq!(inset.width, 90.0);
        assert_eq!(inset.height, 180.0);

        assert!(r.approx_eq(&Rect::new(10.005, 20.005, 100.005, 200.005), 0.01));
        assert!(!r.approx_eq(&Rect::new(10.05, 20.0, 100.0, 200.0), 0.01));
    }

    #[test]
    fn layout_node_tree_formatting_and_search() {
        let mut root = LayoutNode::new(
            "root",
            WidgetKind::Window,
            Rect::new(0.0, 0.0, 800.0, 600.0),
        )
        .with_label("Main Window");

        let mut header = LayoutNode::new(
            "header",
            WidgetKind::Header,
            Rect::new(0.0, 0.0, 800.0, 50.0),
        )
        .with_label("Top Header");

        let button = LayoutNode::new(
            "btn_close",
            WidgetKind::Button,
            Rect::new(750.0, 10.0, 40.0, 30.0),
        )
        .with_label("Close");

        header.add_child(button);
        root.add_child(header);

        // Find child
        let found = root.find_node("btn_close");
        assert!(found.is_some());
        assert_eq!(found.unwrap().label.as_deref(), Some("Close"));

        // Format
        let formatted = root.formatted_tree(0);
        assert!(formatted.contains("- Window #root: [0.0, 0.0, 800.0x600.0] \"Main Window\""));
        assert!(formatted.contains("  - Header #header: [0.0, 0.0, 800.0x50.0] \"Top Header\""));
        assert!(formatted.contains("    - Button #btn_close: [750.0, 10.0, 40.0x30.0] \"Close\""));
    }
}
