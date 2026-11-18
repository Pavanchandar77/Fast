//! Draw-list records. The scene graph flattens into these; the renderer uploads them.
//!
//! Colors on [`ShapeInstance`] are straight (not premultiplied) sRGB. The shape
//! shader premultiplies after coverage. Sprite colors are premultiplied.

use bytemuck::{Pod, Zeroable};

use crate::color::Color;
use crate::geom::{CornerRadii, Rect, Size};
use crate::material::Material;

pub const MODE_FILL: f32 = 0.0;
pub const MODE_SHADOW: f32 = 1.0;
pub const MODE_VIBRANCY: f32 = 2.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ShapeInstance {
    pub rect: [f32; 4],
    pub radii: [f32; 4],
    pub color0: [f32; 4],
    pub color1: [f32; 4],
    pub shadow: [f32; 4],
    pub params: [f32; 4],
    pub extra: [f32; 4],
    pub border: [f32; 4],
    pub clip: [f32; 4],
}

impl ShapeInstance {
    pub fn fill(
        rect: Rect,
        radii: CornerRadii,
        color: Color,
        smoothing: f32,
        opacity: f32,
    ) -> Self {
        Self::panel(
            rect,
            radii,
            color,
            color,
            0.0,
            Color::TRANSPARENT,
            0.0,
            smoothing,
            opacity,
            [0.0; 4],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn panel(
        rect: Rect,
        radii: CornerRadii,
        color0: Color,
        color1: Color,
        angle: f32,
        border: Color,
        border_w: f32,
        smoothing: f32,
        opacity: f32,
        clip: [f32; 4],
    ) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.w, rect.h],
            radii: radii.to_array(),
            color0: color0.to_array(),
            color1: color1.to_array(),
            shadow: [0.0; 4],
            params: [border_w, 0.0, 0.0, 0.0],
            extra: [MODE_FILL, angle, opacity, smoothing],
            border: border.to_array(),
            clip,
        }
    }

    pub fn shadow(
        rect: Rect,
        radii: CornerRadii,
        color: Color,
        blur: f32,
        offset: (f32, f32),
        smoothing: f32,
        opacity: f32,
    ) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.w, rect.h],
            radii: radii.to_array(),
            color0: [0.0; 4],
            color1: [0.0; 4],
            shadow: color.to_array(),
            params: [0.0, blur, offset.0, offset.1],
            extra: [MODE_SHADOW, 0.0, opacity, smoothing],
            border: [0.0; 4],
            clip: [0.0; 4],
        }
    }

    pub fn vibrancy(
        rect: Rect,
        radii: CornerRadii,
        material: Material,
        smoothing: f32,
        opacity: f32,
    ) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.w, rect.h],
            radii: radii.to_array(),
            color0: material.tint.to_array(),
            color1: material.tint.to_array(),
            shadow: [0.0; 4],
            params: [material.noise, material.saturation, material.blur, 0.0],
            extra: [MODE_VIBRANCY, 0.0, opacity, smoothing],
            border: [0.0; 4],
            clip: [0.0; 4],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub dx: f32,
    pub dy: f32,
    pub w: f32,
    pub h: f32,
    pub src_x: f32,
    pub src_y: f32,
    pub src_w: f32,
    pub src_h: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct SpriteInstance {
    pub rect: [f32; 4],
    pub src: [f32; 4],
    pub color: [f32; 4],
}

impl SpriteInstance {
    pub fn glyph(
        origin_x: f32,
        origin_y: f32,
        glyph: PlacedGlyph,
        color: Color,
        opacity: f32,
    ) -> Self {
        let mut tint = color;
        tint.a *= opacity;
        Self {
            rect: [origin_x + glyph.dx, origin_y + glyph.dy, glyph.w, glyph.h],
            src: [glyph.src_x, glyph.src_y, glyph.src_w, glyph.src_h],
            color: tint.premultiply(),
        }
    }

    pub fn image(rect: Rect, src: [f32; 4], opacity: f32) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.w, rect.h],
            src,
            color: [opacity, opacity, opacity, opacity],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SpriteGpu {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
}

impl SpriteGpu {
    pub fn from_sprite(sprite: SpriteInstance, atlas: f32) -> Self {
        let atlas = atlas.max(1.0);
        let uv = [
            sprite.src[0] / atlas,
            sprite.src[1] / atlas,
            (sprite.src[0] + sprite.src[2]) / atlas,
            (sprite.src[1] + sprite.src[3]) / atlas,
        ];
        Self {
            rect: sprite.rect,
            uv,
            color: sprite.color,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DrawList {
    pub background: Vec<ShapeInstance>,
    pub materials: Vec<ShapeInstance>,
    pub foreground: Vec<ShapeInstance>,
    pub sprites: Vec<SpriteInstance>,
}

impl DrawList {
    pub fn with_capacity(shapes: usize, sprites: usize) -> Self {
        Self {
            background: Vec::with_capacity(shapes),
            materials: Vec::with_capacity(shapes / 4 + 1),
            foreground: Vec::with_capacity(shapes),
            sprites: Vec::with_capacity(sprites),
        }
    }

    pub fn clear(&mut self) {
        self.background.clear();
        self.materials.clear();
        self.foreground.clear();
        self.sprites.clear();
    }

    pub fn capacity_signature(&self) -> [usize; 4] {
        [
            self.background.capacity(),
            self.materials.capacity(),
            self.foreground.capacity(),
            self.sprites.capacity(),
        ]
    }
}

impl Default for DrawList {
    fn default() -> Self {
        Self::with_capacity(256, 1024)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    Background,
    Material,
    Foreground,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub color: Color,
    pub width: f32,
}

impl Default for Border {
    fn default() -> Self {
        Self {
            color: Color::TRANSPARENT,
            width: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    Solid(Color),
    Gradient { from: Color, to: Color, angle: f32 },
}

impl Paint {
    pub fn colors(self) -> (Color, Color, f32) {
        match self {
            Self::Solid(color) => (color, color, 0.0),
            Self::Gradient { from, to, angle } => (from, to, angle),
        }
    }
}

/// Resolved visual pushed by widgets. The scene stores this; flatten expands shadows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Primitive {
    #[default]
    Empty,
    Panel {
        size: Size,
        radii: CornerRadii,
        fill: Paint,
        border: Border,
        shadow_blur: f32,
        shadow_offset: (f32, f32),
        shadow_color: Color,
        layer: Layer,
    },
    Material {
        size: Size,
        radii: CornerRadii,
        material: Material,
    },
    Text {
        id: u32,
        color: Color,
        size: Size,
    },
    Icon {
        src: [f32; 4],
        size: f32,
        tint: Color,
    },
    Image {
        src: [f32; 4],
        size: Size,
        radii: CornerRadii,
    },
}
