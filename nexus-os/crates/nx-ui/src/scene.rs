//! Retained scene graph.
//!
//! Nodes are a pool. Each frame the tree is relinked with sibling pointers
//! (no per-node allocation once the pool has warmed up) and flattened into a
//! [`DrawList`] whose buffers keep their capacity.

use crate::draw::{DrawList, Layer, PlacedGlyph, Primitive, ShapeInstance, SpriteInstance};
use crate::geom::{Point, Rect, Size};
use crate::tokens::CORNER_SMOOTHING;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug)]
struct Node {
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    last_child: Option<NodeId>,
    next_sibling: Option<NodeId>,
    origin_x: f32,
    origin_y: f32,
    opacity: f32,
    visible: bool,
    primitive: Primitive,
    world_x: f32,
    world_y: f32,
    world_opacity: f32,
}

impl Node {
    fn root() -> Self {
        Self {
            parent: None,
            first_child: None,
            last_child: None,
            next_sibling: None,
            origin_x: 0.0,
            origin_y: 0.0,
            opacity: 1.0,
            visible: true,
            primitive: Primitive::Empty,
            world_x: 0.0,
            world_y: 0.0,
            world_opacity: 1.0,
        }
    }
}

pub struct Scene {
    nodes: Vec<Node>,
    cursor: usize,
    draw: DrawList,
    smoothing: f32,
    pub grew: bool,
}

impl Scene {
    pub fn new() -> Self {
        Self::with_capacity(512, 1024)
    }

    pub fn with_capacity(nodes: usize, sprites: usize) -> Self {
        let mut scene = Self {
            nodes: Vec::with_capacity(nodes.max(1)),
            cursor: 1,
            draw: DrawList::with_capacity(nodes, sprites),
            smoothing: CORNER_SMOOTHING,
            grew: false,
        };
        scene.nodes.push(Node::root());
        scene
    }

    pub fn root() -> NodeId {
        NodeId(0)
    }

    pub fn set_smoothing(&mut self, smoothing: f32) {
        self.smoothing = smoothing.clamp(0.0, 1.0);
    }

    pub fn begin_frame(&mut self) {
        let before = self.nodes.capacity();
        for node in &mut self.nodes {
            node.first_child = None;
            node.last_child = None;
            node.next_sibling = None;
            node.parent = None;
        }
        self.cursor = 1;
        self.grew = self.nodes.capacity() > before;
    }

    pub fn emit(
        &mut self,
        parent: NodeId,
        origin: Point,
        opacity: f32,
        primitive: Primitive,
    ) -> NodeId {
        let before_nodes = self.nodes.capacity();
        let id = if self.cursor < self.nodes.len() {
            let id = NodeId(self.cursor as u32);
            let node = &mut self.nodes[self.cursor];
            node.origin_x = origin.x;
            node.origin_y = origin.y;
            node.opacity = opacity;
            node.visible = true;
            node.primitive = primitive;
            self.cursor += 1;
            id
        } else {
            self.nodes.push(Node {
                parent: None,
                first_child: None,
                last_child: None,
                next_sibling: None,
                origin_x: origin.x,
                origin_y: origin.y,
                opacity,
                visible: true,
                primitive,
                world_x: 0.0,
                world_y: 0.0,
                world_opacity: 1.0,
            });
            let id = NodeId((self.nodes.len() - 1) as u32);
            self.cursor = self.nodes.len();
            id
        };
        if self.nodes.capacity() > before_nodes {
            self.grew = true;
        }
        self.link(parent, id);
        id
    }

    fn link(&mut self, parent: NodeId, child: NodeId) {
        let parent_i = parent.0 as usize;
        let child_i = child.0 as usize;
        if parent_i >= self.nodes.len() || child_i >= self.nodes.len() || parent_i == child_i {
            return;
        }
        self.nodes[child_i].parent = Some(parent);
        self.nodes[child_i].next_sibling = None;
        if self.nodes[parent_i].first_child.is_none() {
            self.nodes[parent_i].first_child = Some(child);
            self.nodes[parent_i].last_child = Some(child);
            return;
        }
        if let Some(last) = self.nodes[parent_i].last_child {
            self.nodes[last.0 as usize].next_sibling = Some(child);
        }
        self.nodes[parent_i].last_child = Some(child);
    }

    pub fn flatten<'a>(&mut self, glyphs: impl Fn(u32) -> &'a [PlacedGlyph]) -> &DrawList {
        let draw_before = self.draw.capacity_signature();
        self.draw.clear();
        self.place(0, 0.0, 0.0, 1.0);
        self.emit_walk(0, &glyphs);
        let draw_after = self.draw.capacity_signature();
        if draw_after != draw_before {
            // Capacity grew. `clear` does not shrink, so a difference means a realloc.
            for (before, after) in draw_before.iter().zip(draw_after.iter()) {
                if after > before {
                    self.grew = true;
                }
            }
        }
        &self.draw
    }

    pub fn draw(&self) -> &DrawList {
        &self.draw
    }

    pub fn node_count(&self) -> usize {
        self.cursor
    }

    pub fn capacities(&self) -> (usize, [usize; 4]) {
        (self.nodes.capacity(), self.draw.capacity_signature())
    }

    fn place(&mut self, index: usize, px: f32, py: f32, po: f32) {
        if index >= self.nodes.len() {
            return;
        }
        let origin_x = self.nodes[index].origin_x;
        let origin_y = self.nodes[index].origin_y;
        let opacity = self.nodes[index].opacity;
        let visible = self.nodes[index].visible;
        self.nodes[index].world_x = px + origin_x;
        self.nodes[index].world_y = py + origin_y;
        self.nodes[index].world_opacity = po * opacity;
        if !visible {
            return;
        }
        let wx = self.nodes[index].world_x;
        let wy = self.nodes[index].world_y;
        let wo = self.nodes[index].world_opacity;
        let mut child = self.nodes[index].first_child;
        while let Some(cid) = child {
            let next = self.nodes[cid.0 as usize].next_sibling;
            self.place(cid.0 as usize, wx, wy, wo);
            child = next;
        }
    }

    fn emit_walk<'a>(&mut self, index: usize, glyphs: &impl Fn(u32) -> &'a [PlacedGlyph]) {
        if index >= self.nodes.len() || !self.nodes[index].visible {
            return;
        }
        let wx = self.nodes[index].world_x;
        let wy = self.nodes[index].world_y;
        let opacity = self.nodes[index].world_opacity;
        let primitive = self.nodes[index].primitive;
        push_primitive(
            &mut self.draw,
            wx,
            wy,
            opacity,
            primitive,
            self.smoothing,
            glyphs,
        );
        let mut child = self.nodes[index].first_child;
        while let Some(cid) = child {
            let next = self.nodes[cid.0 as usize].next_sibling;
            self.emit_walk(cid.0 as usize, glyphs);
            child = next;
        }
    }

    /// Top-most node whose primitive rect contains `point`, in paint order.
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        self.hit_rec(0, point)
    }

    fn hit_rec(&self, index: usize, point: Point) -> Option<NodeId> {
        if index >= self.nodes.len() || !self.nodes[index].visible {
            return None;
        }
        let mut found = None;
        if primitive_contains(
            self.nodes[index].primitive,
            self.nodes[index].world_x,
            self.nodes[index].world_y,
            point,
        ) {
            found = Some(NodeId(index as u32));
        }
        let mut child = self.nodes[index].first_child;
        while let Some(cid) = child {
            if let Some(hit) = self.hit_rec(cid.0 as usize, point) {
                found = Some(hit);
            }
            child = self.nodes[cid.0 as usize].next_sibling;
        }
        found
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

fn primitive_contains(primitive: Primitive, x: f32, y: f32, point: Point) -> bool {
    let size = match primitive {
        Primitive::Empty => return false,
        Primitive::Panel { size, .. }
        | Primitive::Text { size, .. }
        | Primitive::Image { size, .. }
        | Primitive::Material { size, .. } => size,
        Primitive::Icon { size, .. } => Size::new(size, size),
    };
    Rect::new(x, y, size.w, size.h).contains(point)
}

fn push_primitive<'a>(
    draw: &mut DrawList,
    x: f32,
    y: f32,
    opacity: f32,
    primitive: Primitive,
    smoothing: f32,
    glyphs: &impl Fn(u32) -> &'a [PlacedGlyph],
) {
    if opacity <= 0.001 {
        return;
    }
    match primitive {
        Primitive::Empty => {}
        Primitive::Panel {
            size,
            radii,
            fill,
            border,
            shadow_blur,
            shadow_offset,
            shadow_color,
            layer,
        } => {
            let rect = Rect::new(x, y, size.w, size.h);
            if shadow_blur > 0.0 && shadow_color.a > 0.0 && !matches!(layer, Layer::Material) {
                let shadow = ShapeInstance::shadow(
                    rect,
                    radii,
                    shadow_color,
                    shadow_blur,
                    shadow_offset,
                    smoothing,
                    opacity,
                );
                match layer {
                    Layer::Foreground => draw.foreground.push(shadow),
                    _ => draw.background.push(shadow),
                }
            }
            let (c0, c1, angle) = fill.colors();
            let shape = ShapeInstance::panel(
                rect,
                radii,
                c0,
                c1,
                angle,
                border.color,
                border.width,
                smoothing,
                opacity,
                [0.0; 4],
            );
            match layer {
                Layer::Foreground => draw.foreground.push(shape),
                Layer::Material => draw.materials.push(shape),
                Layer::Background => draw.background.push(shape),
            }
        }
        Primitive::Material {
            size,
            radii,
            material,
        } => {
            let rect = Rect::new(x, y, size.w, size.h);
            draw.materials.push(ShapeInstance::vibrancy(
                rect, radii, material, smoothing, opacity,
            ));
        }
        Primitive::Text { id, color, .. } => {
            if id == u32::MAX {
                return;
            }
            for glyph in glyphs(id) {
                if glyph.w <= 0.0 || glyph.h <= 0.0 {
                    continue;
                }
                draw.sprites
                    .push(SpriteInstance::glyph(x, y, *glyph, color, opacity));
            }
        }
        Primitive::Icon { src, size, tint } => {
            draw.sprites
                .push(SpriteInstance::image(Rect::new(x, y, size, size), src, opacity).tint(tint));
        }
        Primitive::Image { src, size, .. } => {
            draw.sprites.push(SpriteInstance::image(
                Rect::new(x, y, size.w, size.h),
                src,
                opacity,
            ));
        }
    }
}

impl SpriteInstance {
    fn tint(mut self, color: crate::color::Color) -> Self {
        let prem = color.premultiply();
        self.color = [
            self.color[0] * prem[0],
            self.color[1] * prem[1],
            self.color[2] * prem[2],
            self.color[3] * prem[3],
        ];
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::draw::{Border, Layer, Paint};
    use crate::geom::{CornerRadii, Size};

    fn panel(w: f32, h: f32, color: Color) -> Primitive {
        Primitive::Panel {
            size: Size::new(w, h),
            radii: CornerRadii::all(8.0),
            fill: Paint::Solid(color),
            border: Border::default(),
            shadow_blur: 0.0,
            shadow_offset: (0.0, 0.0),
            shadow_color: Color::TRANSPARENT,
            layer: Layer::Background,
        }
    }

    #[test]
    fn children_paint_after_their_parent() {
        let mut scene = Scene::new();
        scene.begin_frame();
        let parent = scene.emit(
            Scene::root(),
            Point::new(0.0, 0.0),
            1.0,
            panel(100.0, 80.0, Color::WHITE),
        );
        scene.emit(
            parent,
            Point::new(10.0, 10.0),
            1.0,
            panel(20.0, 20.0, Color::BLACK),
        );
        let draw = scene.flatten(|_| &[]).clone();
        assert_eq!(draw.background.len(), 2);
        assert_eq!(draw.background[0].rect, [0.0, 0.0, 100.0, 80.0]);
        assert_eq!(draw.background[1].rect, [10.0, 10.0, 20.0, 20.0]);
    }

    #[test]
    fn opacity_multiplies_down_the_tree() {
        let mut scene = Scene::new();
        scene.begin_frame();
        let parent = scene.emit(
            Scene::root(),
            Point::new(0.0, 0.0),
            0.5,
            panel(10.0, 10.0, Color::WHITE),
        );
        scene.emit(
            parent,
            Point::new(1.0, 2.0),
            0.5,
            panel(4.0, 4.0, Color::WHITE),
        );
        let draw = scene.flatten(|_| &[]).clone();
        assert!((draw.background[0].extra[2] - 0.5).abs() < 1e-5);
        assert!((draw.background[1].extra[2] - 0.25).abs() < 1e-5);
        assert_eq!(draw.background[1].rect[0], 1.0);
        assert_eq!(draw.background[1].rect[1], 2.0);
    }

    #[test]
    fn hidden_nodes_are_skipped() {
        let mut scene = Scene::new();
        scene.begin_frame();
        let parent = scene.emit(
            Scene::root(),
            Point::new(0.0, 0.0),
            1.0,
            panel(10.0, 10.0, Color::WHITE),
        );
        scene.nodes[parent.0 as usize].visible = false;
        scene.emit(
            parent,
            Point::new(0.0, 0.0),
            1.0,
            panel(4.0, 4.0, Color::BLACK),
        );
        let draw = scene.flatten(|_| &[]);
        assert!(draw.background.is_empty());
    }

    #[test]
    fn steady_flatten_does_not_grow_capacity() {
        let mut scene = Scene::with_capacity(32, 8);
        for _ in 0..2 {
            scene.begin_frame();
            let parent = scene.emit(
                Scene::root(),
                Point::new(0.0, 0.0),
                1.0,
                panel(30.0, 20.0, Color::WHITE),
            );
            scene.emit(
                parent,
                Point::new(2.0, 2.0),
                1.0,
                panel(8.0, 8.0, Color::BLACK),
            );
            scene.flatten(|_| &[]);
        }
        let caps = scene.capacities();
        scene.grew = false;
        scene.begin_frame();
        let parent = scene.emit(
            Scene::root(),
            Point::new(0.0, 0.0),
            1.0,
            panel(30.0, 20.0, Color::WHITE),
        );
        scene.emit(
            parent,
            Point::new(2.0, 2.0),
            1.0,
            panel(8.0, 8.0, Color::BLACK),
        );
        scene.flatten(|_| &[]);
        assert!(!scene.grew);
        assert_eq!(scene.capacities(), caps);
    }

    #[test]
    fn hit_test_returns_the_top_most_child() {
        let mut scene = Scene::new();
        scene.begin_frame();
        let parent = scene.emit(
            Scene::root(),
            Point::new(0.0, 0.0),
            1.0,
            panel(100.0, 100.0, Color::WHITE),
        );
        let child = scene.emit(
            parent,
            Point::new(10.0, 10.0),
            1.0,
            panel(20.0, 20.0, Color::BLACK),
        );
        scene.flatten(|_| &[]);
        assert_eq!(scene.hit_test(Point::new(12.0, 12.0)), Some(child));
        assert_eq!(scene.hit_test(Point::new(2.0, 2.0)), Some(parent));
        assert_eq!(scene.hit_test(Point::new(200.0, 200.0)), None);
    }
}
