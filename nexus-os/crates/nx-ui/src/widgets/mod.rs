//! Interactive controls. Widgets own their springs and paint into a [`Scene`].
//!
//! Nothing here opens a window. Pass an [`Input`] from tests or from winit.

mod controls;
mod table;

pub use controls::{
    Button, ContextMenu, ScrollView, SearchField, Segmented, Sidebar, SidebarItem, Toolbar,
    ToolbarSlots, Tooltip,
};
pub use table::{row_name, VirtualTable, TABLE_COLUMNS};

use crate::atlas::Atlas;
use crate::color::Color;
use crate::draw::{Border, Layer, Paint, Primitive};
use crate::geom::{CornerRadii, Point, Rect, Size};
use crate::icon::IconCache;
use crate::material::Material;
use crate::scene::{NodeId, Scene};
use crate::text::{TextEngine, TextStyle};
use crate::theme::Theme;
use crate::tokens::{Shadow, TypeStep};

/// Borrows for one paint. `theme` is copied so widgets can keep using it.
pub struct Frame<'a> {
    pub scene: &'a mut Scene,
    pub text: &'a mut TextEngine,
    pub icons: &'a mut IconCache,
    pub atlas: &'a mut Atlas,
    pub theme: Theme,
}

pub fn style_of(step: TypeStep) -> TextStyle {
    TextStyle::new(step.size, step.line, step.weight)
}

impl Frame<'_> {
    pub fn panel(
        &mut self,
        parent: NodeId,
        rect: Rect,
        fill: Paint,
        radii: CornerRadii,
        border: Border,
        shadow: Shadow,
        layer: Layer,
    ) -> NodeId {
        self.scene.emit(
            parent,
            rect.origin(),
            1.0,
            Primitive::Panel {
                size: rect.size(),
                radii,
                fill,
                border,
                shadow_blur: shadow.blur,
                shadow_offset: (shadow.x, shadow.y),
                shadow_color: shadow.color,
                layer,
            },
        )
    }

    pub fn material(
        &mut self,
        parent: NodeId,
        rect: Rect,
        radii: CornerRadii,
        material: Material,
    ) -> NodeId {
        self.scene.emit(
            parent,
            rect.origin(),
            1.0,
            Primitive::Material {
                size: rect.size(),
                radii,
                material,
            },
        )
    }

    pub fn label(
        &mut self,
        parent: NodeId,
        origin: Point,
        text: &str,
        style: TextStyle,
        color: Color,
    ) -> Size {
        let prepared = self.text.prepare(self.atlas, text, style);
        if prepared.id == crate::text::TEXT_NONE || prepared.width <= 0.0 {
            return Size::new(prepared.width, prepared.height);
        }
        self.scene.emit(
            parent,
            origin,
            1.0,
            Primitive::Text {
                id: prepared.id,
                color,
                size: Size::new(prepared.width, prepared.height),
            },
        );
        Size::new(prepared.width, prepared.height)
    }

    pub fn icon(&mut self, parent: NodeId, origin: Point, svg: &str, px: f32, tint: Color) {
        let size = px.max(8.0);
        let Ok(src) = self.icons.prepare(self.atlas, svg, size.round() as u32) else {
            return;
        };
        self.scene
            .emit(parent, origin, 1.0, Primitive::Icon { src, size, tint });
    }

    pub fn image(&mut self, parent: NodeId, rect: Rect, src: [f32; 4], radii: CornerRadii) {
        self.scene.emit(
            parent,
            rect.origin(),
            1.0,
            Primitive::Image {
                src,
                size: rect.size(),
                radii,
            },
        );
    }
}
