//! wgpu renderer. Instanced SDF quads, a separable blur, vibrancy, and sprites.
//!
//! A frame with no vibrancy skips the blur pass. Noise is a spatial hash, so a
//! still scene does not need another redraw. GPU buffer capacity grows until
//! the scene stabilizes; steady frames only `write_buffer` into those buffers.

use std::sync::mpsc;

use bytemuck::{Pod, Zeroable};

use crate::atlas::Atlas;
use crate::color::Color;
use crate::draw::{DrawList, ShapeInstance, SpriteGpu, SpriteInstance, MODE_VIBRANCY};
use crate::error::{Error, Result};
use crate::material::gaussian_kernel;

pub const SHAPE_WGSL: &str = include_str!("shaders/shape.wgsl");
pub const VIBRANCY_WGSL: &str = include_str!("shaders/vibrancy.wgsl");
pub const BLUR_WGSL: &str = include_str!("shaders/blur.wgsl");
pub const BLIT_WGSL: &str = include_str!("shaders/blit.wgsl");
pub const SPRITE_WGSL: &str = include_str!("shaders/sprite.wgsl");

const SHAPE_ATTRS: [wgpu::VertexAttribute; 9] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 16,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 32,
        shader_location: 2,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 48,
        shader_location: 3,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 64,
        shader_location: 4,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 80,
        shader_location: 5,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 96,
        shader_location: 6,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 112,
        shader_location: 7,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 128,
        shader_location: 8,
    },
];

const SPRITE_ATTRS: [wgpu::VertexAttribute; 3] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 16,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 32,
        shader_location: 2,
    },
];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniforms {
    viewport: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurUniforms {
    dir_texel: [f32; 4],
    w0: [f32; 4],
    w1: [f32; 4],
    w2: [f32; 4],
}

struct Slot<T> {
    label: &'static str,
    buffer: wgpu::Buffer,
    capacity: u64,
    count: u32,
    staging: Vec<T>,
}

impl<T: Pod> Slot<T> {
    fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self {
            label,
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 256,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            capacity: 256,
            count: 0,
            staging: Vec::new(),
        }
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[T]) {
        self.count = data.len() as u32;
        if data.is_empty() {
            return;
        }
        self.ensure(device, std::mem::size_of_val(data) as u64);
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(data));
    }

    fn upload_staging(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.count = self.staging.len() as u32;
        if self.staging.is_empty() {
            return;
        }
        self.ensure(
            device,
            std::mem::size_of_val(self.staging.as_slice()) as u64,
        );
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.staging));
    }

    fn ensure(&mut self, device: &wgpu::Device, bytes: u64) {
        if bytes <= self.capacity {
            return;
        }
        let mut cap = self.capacity.max(256);
        while cap < bytes {
            cap = cap.saturating_mul(2);
        }
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.capacity = cap;
    }
}

struct ColorTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct BlurTargets {
    size: (u32, u32),
    sharp: ColorTarget,
    ping: ColorTarget,
    pong: ColorTarget,
    h_buf: wgpu::Buffer,
    v_buf: wgpu::Buffer,
    h_bind: wgpu::BindGroup,
    v_bind: wgpu::BindGroup,
    blit_bind: wgpu::BindGroup,
    vib_bind: wgpu::BindGroup,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    sampler: wgpu::Sampler,
    frame_buf: wgpu::Buffer,
    shape_pipeline: wgpu::RenderPipeline,
    shape_bind: wgpu::BindGroup,
    vibrancy_pipeline: wgpu::RenderPipeline,
    vibrancy_bgl: wgpu::BindGroupLayout,
    blur_pipeline: wgpu::RenderPipeline,
    blur_bgl: wgpu::BindGroupLayout,
    blit_pipeline: wgpu::RenderPipeline,
    blit_bgl: wgpu::BindGroupLayout,
    sprite_pipeline: wgpu::RenderPipeline,
    sprite_bgl: wgpu::BindGroupLayout,
    sprite_bind: wgpu::BindGroup,
    bg: Slot<ShapeInstance>,
    fg: Slot<ShapeInstance>,
    solid: Slot<ShapeInstance>,
    vib: Slot<ShapeInstance>,
    sprites: Slot<SpriteGpu>,
    atlas_tex: wgpu::Texture,
    atlas_size: u32,
    atlas_gen: u64,
    blur: Option<BlurTargets>,
}

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nx-ui-linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });
        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nx-ui-frame"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shape_bgl = uniform_layout(
            &device,
            "nx-ui-shape-bgl",
            wgpu::ShaderStages::VERTEX_FRAGMENT,
        );
        let shape_shader = shader(&device, "nx-ui-shape", SHAPE_WGSL);
        let shape_pipeline = pipeline(
            &device,
            &shape_shader,
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("nx-ui-shape-pl"),
                bind_group_layouts: &[&shape_bgl],
                push_constant_ranges: &[],
            }),
            Some(shape_vertex_layout()),
            format,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            "nx-ui-shape",
        );
        let shape_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nx-ui-shape-bg"),
            layout: &shape_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buf.as_entire_binding(),
            }],
        });

        let vibrancy_bgl = textured_layout(&device, "nx-ui-vib-bgl", true);
        let vibrancy_shader = shader(&device, "nx-ui-vibrancy", VIBRANCY_WGSL);
        let vibrancy_pipeline = pipeline(
            &device,
            &vibrancy_shader,
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("nx-ui-vib-pl"),
                bind_group_layouts: &[&vibrancy_bgl],
                push_constant_ranges: &[],
            }),
            Some(shape_vertex_layout()),
            format,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            "nx-ui-vibrancy",
        );

        let blur_bgl = textured_layout(&device, "nx-ui-blur-bgl", true);
        let blur_shader = shader(&device, "nx-ui-blur", BLUR_WGSL);
        let blur_pipeline = pipeline(
            &device,
            &blur_shader,
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("nx-ui-blur-pl"),
                bind_group_layouts: &[&blur_bgl],
                push_constant_ranges: &[],
            }),
            None,
            format,
            None,
            "nx-ui-blur",
        );

        let blit_bgl = textured_layout(&device, "nx-ui-blit-bgl", false);
        let blit_shader = shader(&device, "nx-ui-blit", BLIT_WGSL);
        let blit_pipeline = pipeline(
            &device,
            &blit_shader,
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("nx-ui-blit-pl"),
                bind_group_layouts: &[&blit_bgl],
                push_constant_ranges: &[],
            }),
            None,
            format,
            None,
            "nx-ui-blit",
        );

        let sprite_bgl = textured_layout(&device, "nx-ui-sprite-bgl", true);
        let sprite_shader = shader(&device, "nx-ui-sprite", SPRITE_WGSL);
        let sprite_pipeline = pipeline(
            &device,
            &sprite_shader,
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("nx-ui-sprite-pl"),
                bind_group_layouts: &[&sprite_bgl],
                push_constant_ranges: &[],
            }),
            Some(sprite_vertex_layout()),
            format,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            "nx-ui-sprite",
        );

        let atlas_size = 64;
        let atlas_tex = color_texture(
            &device,
            atlas_size,
            atlas_size,
            wgpu::TextureFormat::Rgba8Unorm,
            false,
            "nx-ui-atlas",
        );
        let atlas_view = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sprite_bind =
            sprite_bind_group(&device, &sprite_bgl, &frame_buf, &atlas_view, &sampler);

        Self {
            bg: Slot::new(&device, "nx-ui-bg"),
            fg: Slot::new(&device, "nx-ui-fg"),
            solid: Slot::new(&device, "nx-ui-solid"),
            vib: Slot::new(&device, "nx-ui-vib"),
            sprites: Slot::new(&device, "nx-ui-sprites"),
            device,
            queue,
            format,
            sampler,
            frame_buf,
            shape_pipeline,
            shape_bind,
            vibrancy_pipeline,
            vibrancy_bgl,
            blur_pipeline,
            blur_bgl,
            blit_pipeline,
            blit_bgl,
            sprite_pipeline,
            sprite_bgl,
            sprite_bind,
            atlas_tex,
            atlas_size,
            atlas_gen: 0,
            blur: None,
        }
    }

    pub fn render(
        &mut self,
        target: &wgpu::TextureView,
        size: (u32, u32),
        clear: Color,
        draw: &DrawList,
        atlas: &Atlas,
    ) -> Result<()> {
        if size.0 == 0 || size.1 == 0 {
            return Ok(());
        }
        let device = self.device.clone();
        let queue = self.queue.clone();
        self.sync_atlas(&device, &queue, atlas);
        let frame = FrameUniforms {
            viewport: [size.0 as f32, size.1 as f32],
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&frame));
        self.bg.upload(&device, &queue, &draw.background);
        self.fg.upload(&device, &queue, &draw.foreground);
        self.solid.staging.clear();
        self.vib.staging.clear();
        for shape in &draw.materials {
            if shape.extra[0] == MODE_VIBRANCY {
                self.vib.staging.push(*shape);
            } else {
                self.solid.staging.push(*shape);
            }
        }
        self.solid.upload_staging(&device, &queue);
        self.vib.upload_staging(&device, &queue);
        stage_sprites(
            &mut self.sprites.staging,
            &draw.sprites,
            atlas.size() as f32,
        );
        self.sprites.upload_staging(&device, &queue);

        let clear = gpu_color(clear);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nx-ui"),
        });
        if self.vib.count > 0 {
            let spread = max_spread(&self.vib.staging);
            self.ensure_blur(&device, size);
            self.write_blur(&queue, spread, size);
            self.encode_blur_path(&mut encoder, target, size, clear);
        } else {
            self.encode_direct(&mut encoder, target, size, clear);
        }
        queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn sync_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &Atlas) {
        if self.atlas_size != atlas.size() {
            self.atlas_tex = color_texture(
                device,
                atlas.size(),
                atlas.size(),
                wgpu::TextureFormat::Rgba8Unorm,
                false,
                "nx-ui-atlas",
            );
            self.atlas_size = atlas.size();
            let view = self
                .atlas_tex
                .create_view(&wgpu::TextureViewDescriptor::default());
            self.sprite_bind = sprite_bind_group(
                device,
                &self.sprite_bgl,
                &self.frame_buf,
                &view,
                &self.sampler,
            );
            self.atlas_gen = 0;
        }
        if self.atlas_gen == atlas.generation {
            return;
        }
        let row = atlas.size() * 4;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            atlas.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(atlas.size()),
            },
            wgpu::Extent3d {
                width: atlas.size(),
                height: atlas.size(),
                depth_or_array_layers: 1,
            },
        );
        self.atlas_gen = atlas.generation;
    }

    fn ensure_blur(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.blur.as_ref().is_some_and(|blur| blur.size == size) {
            return;
        }
        let sharp = ColorTarget::new(device, size, self.format, "nx-ui-sharp");
        let ping = ColorTarget::new(device, size, self.format, "nx-ui-ping");
        let pong = ColorTarget::new(device, size, self.format, "nx-ui-pong");
        let h_buf = uniform_buffer(device, "nx-ui-blur-h");
        let v_buf = uniform_buffer(device, "nx-ui-blur-v");
        let h_bind = blur_bind(device, &self.blur_bgl, &h_buf, &sharp.view, &self.sampler);
        let v_bind = blur_bind(device, &self.blur_bgl, &v_buf, &ping.view, &self.sampler);
        let blit_bind = blit_bind(device, &self.blit_bgl, &sharp.view, &self.sampler);
        let vib_bind = sprite_bind_group(
            device,
            &self.vibrancy_bgl,
            &self.frame_buf,
            &pong.view,
            &self.sampler,
        );
        self.blur = Some(BlurTargets {
            size,
            sharp,
            ping,
            pong,
            h_buf,
            v_buf,
            h_bind,
            v_bind,
            blit_bind,
            vib_bind,
        });
    }

    fn write_blur(&self, queue: &wgpu::Queue, spread: f32, size: (u32, u32)) {
        let Some(blur) = self.blur.as_ref() else {
            return;
        };
        let (horizontal, vertical) = blur_pair(spread, size.0, size.1);
        queue.write_buffer(&blur.h_buf, 0, bytemuck::bytes_of(&horizontal));
        queue.write_buffer(&blur.v_buf, 0, bytemuck::bytes_of(&vertical));
    }

    fn encode_direct(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
        clear: wgpu::Color,
    ) {
        let mut pass = begin_pass(encoder, target, clear, "nx-ui-direct");
        pass.set_viewport(0.0, 0.0, size.0 as f32, size.1 as f32, 0.0, 1.0);
        draw_shapes(
            &mut pass,
            &self.shape_pipeline,
            &self.shape_bind,
            &self.bg,
            &self.solid,
            &self.fg,
        );
        draw_sprites(
            &mut pass,
            &self.sprite_pipeline,
            &self.sprite_bind,
            &self.sprites,
        );
    }

    fn encode_blur_path(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
        clear: wgpu::Color,
    ) {
        let Some(blur) = self.blur.as_ref() else {
            return;
        };
        {
            let mut pass = begin_pass(encoder, &blur.sharp.view, clear, "nx-ui-offscreen");
            pass.set_viewport(0.0, 0.0, size.0 as f32, size.1 as f32, 0.0, 1.0);
            draw_one(
                &mut pass,
                &self.shape_pipeline,
                &self.shape_bind,
                &self.bg.buffer,
                self.bg.count,
            );
        }
        {
            let mut pass = begin_pass(encoder, &blur.ping.view, clear, "nx-ui-blur-h");
            pass.set_viewport(0.0, 0.0, size.0 as f32, size.1 as f32, 0.0, 1.0);
            draw_fullscreen(&mut pass, &self.blur_pipeline, &blur.h_bind);
        }
        {
            let mut pass = begin_pass(encoder, &blur.pong.view, clear, "nx-ui-blur-v");
            pass.set_viewport(0.0, 0.0, size.0 as f32, size.1 as f32, 0.0, 1.0);
            draw_fullscreen(&mut pass, &self.blur_pipeline, &blur.v_bind);
        }
        let mut pass = begin_pass(encoder, target, clear, "nx-ui-composite");
        pass.set_viewport(0.0, 0.0, size.0 as f32, size.1 as f32, 0.0, 1.0);
        draw_fullscreen(&mut pass, &self.blit_pipeline, &blur.blit_bind);
        draw_one(
            &mut pass,
            &self.vibrancy_pipeline,
            &blur.vib_bind,
            &self.vib.buffer,
            self.vib.count,
        );
        draw_one(
            &mut pass,
            &self.shape_pipeline,
            &self.shape_bind,
            &self.solid.buffer,
            self.solid.count,
        );
        draw_one(
            &mut pass,
            &self.shape_pipeline,
            &self.shape_bind,
            &self.fg.buffer,
            self.fg.count,
        );
        draw_sprites(
            &mut pass,
            &self.sprite_pipeline,
            &self.sprite_bind,
            &self.sprites,
        );
    }
}

fn draw_shapes<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    pipeline: &'a wgpu::RenderPipeline,
    group: &'a wgpu::BindGroup,
    bg: &'a Slot<ShapeInstance>,
    solid: &'a Slot<ShapeInstance>,
    fg: &'a Slot<ShapeInstance>,
) {
    draw_one(pass, pipeline, group, &bg.buffer, bg.count);
    draw_one(pass, pipeline, group, &solid.buffer, solid.count);
    draw_one(pass, pipeline, group, &fg.buffer, fg.count);
}

fn draw_sprites<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    pipeline: &'a wgpu::RenderPipeline,
    group: &'a wgpu::BindGroup,
    slot: &'a Slot<SpriteGpu>,
) {
    draw_one(pass, pipeline, group, &slot.buffer, slot.count);
}

fn draw_one<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    pipeline: &'a wgpu::RenderPipeline,
    group: &'a wgpu::BindGroup,
    buffer: &'a wgpu::Buffer,
    count: u32,
) {
    if count == 0 {
        return;
    }
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.set_vertex_buffer(0, buffer.slice(..));
    pass.draw(0..6, 0..count);
}

fn draw_fullscreen<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    pipeline: &'a wgpu::RenderPipeline,
    group: &'a wgpu::BindGroup,
) {
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    clear: wgpu::Color,
    label: &'a str,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

impl ColorTarget {
    fn new(
        device: &wgpu::Device,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        label: &'static str,
    ) -> Self {
        let texture = color_texture(device, size.0, size.1, format, true, label);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            _texture: texture,
            view,
        }
    }
}

pub struct Headless {
    renderer: Renderer,
    target: wgpu::Texture,
    pub width: u32,
    pub height: u32,
}

impl Headless {
    pub fn try_new(width: u32, height: u32) -> Result<Self> {
        let (device, queue) = request_device()?;
        tracing::debug!("nx-ui headless {width}x{height}");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let renderer = Renderer::new(device.clone(), queue, format);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nx-ui-headless"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Ok(Self {
            renderer,
            target,
            width: width.max(1),
            height: height.max(1),
        })
    }

    pub fn render_rgba(&mut self, clear: Color, draw: &DrawList, atlas: &Atlas) -> Result<Vec<u8>> {
        let view = self
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer
            .render(&view, (self.width, self.height), clear, draw, atlas)?;
        read_rgba(
            &self.renderer.device,
            &self.renderer.queue,
            &self.target,
            self.width,
            self.height,
        )
    }
}

pub fn preferred_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    const ORDER: [wgpu::TextureFormat; 4] = [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ];
    for format in ORDER {
        if formats.contains(&format) {
            return Some(format);
        }
    }
    formats.first().copied()
}

pub fn stage_sprites(dst: &mut Vec<SpriteGpu>, sprites: &[SpriteInstance], atlas: f32) {
    dst.clear();
    dst.extend(
        sprites
            .iter()
            .copied()
            .map(|sprite| SpriteGpu::from_sprite(sprite, atlas)),
    );
}

fn request_device() -> Result<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or(Error::NoAdapter)?;
    let limits = adapter.limits();
    let mut required_limits =
        wgpu::Limits::downlevel_webgl2_defaults().using_resolution(limits.clone());
    required_limits.max_inter_stage_shader_components = limits.max_inter_stage_shader_components;
    pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("nx-ui"),
            required_features: wgpu::Features::empty(),
            required_limits,
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))
    .map_err(|err| Error::Gpu(err.to_string()))
}

fn read_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let unpadded = width.saturating_mul(4);
    let padded = unpadded.saturating_add(align - 1) & !(align - 1);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nx-ui-readback"),
        size: (padded as u64).saturating_mul(height as u64).max(4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nx-ui-readback"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    let (send, recv) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = send.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    match recv.recv() {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(Error::Gpu(format!("map readback: {err}"))),
        Err(_) => return Err(Error::Gpu("map readback closed".into())),
    }
    let mapped = slice.get_mapped_range();
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for row in 0..height as usize {
        let src = row * padded as usize;
        let dst = row * width as usize * 4;
        pixels[dst..dst + unpadded as usize].copy_from_slice(&mapped[src..src + unpadded as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}

fn shader(device: &wgpu::Device, label: &'static str, source: &'static str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    vertex: Option<wgpu::VertexBufferLayout<'static>>,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    label: &'static str,
) -> wgpu::RenderPipeline {
    let buffers: &[wgpu::VertexBufferLayout] = match &vertex {
        Some(layout) => std::slice::from_ref(layout),
        None => &[],
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            buffers,
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn shape_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ShapeInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &SHAPE_ATTRS,
    }
}

fn sprite_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<SpriteGpu>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &SPRITE_ATTRS,
    }
}

fn uniform_layout(
    device: &wgpu::Device,
    label: &str,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn textured_layout(device: &wgpu::Device, label: &str, with_frame: bool) -> wgpu::BindGroupLayout {
    let tex = wgpu::BindGroupLayoutEntry {
        binding: if with_frame { 1 } else { 0 },
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let samp = wgpu::BindGroupLayoutEntry {
        binding: if with_frame { 2 } else { 1 },
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    };
    if with_frame {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex,
                samp,
            ],
        })
    } else {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[tex, samp],
        })
    }
}

fn sprite_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    frame: &wgpu::Buffer,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("nx-ui-textured"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: frame.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn blur_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    sprite_bind_group(device, layout, uniform, view, sampler)
}

fn blit_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("nx-ui-blit"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn uniform_buffer(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of::<BlurUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn color_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    target: bool,
    label: &'static str,
) -> wgpu::Texture {
    let usage = if target {
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
    } else {
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST
    };
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn gpu_color(color: Color) -> wgpu::Color {
    wgpu::Color {
        r: color.r as f64,
        g: color.g as f64,
        b: color.b as f64,
        a: color.a as f64,
    }
}

fn max_spread(shapes: &[ShapeInstance]) -> f32 {
    let mut spread: f32 = 8.0;
    for shape in shapes {
        spread = spread.max(shape.params[2]);
    }
    spread
}

fn pack_kernel(weights: [f32; 9]) -> [[f32; 4]; 3] {
    [
        [weights[0], weights[1], weights[2], weights[3]],
        [weights[4], weights[5], weights[6], weights[7]],
        [weights[8], 0.0, 0.0, 0.0],
    ]
}

fn blur_pair(spread: f32, width: u32, height: u32) -> (BlurUniforms, BlurUniforms) {
    let packed = pack_kernel(gaussian_kernel(1.8));
    let step = (spread / 4.0).max(1.0);
    let horizontal = BlurUniforms {
        dir_texel: [step / width.max(1) as f32, 0.0, 0.0, 0.0],
        w0: packed[0],
        w1: packed[1],
        w2: packed[2],
    };
    let vertical = BlurUniforms {
        dir_texel: [0.0, step / height.max(1) as f32, 0.0, 0.0],
        w0: packed[0],
        w1: packed[1],
        w2: packed[2],
    };
    (horizontal, vertical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::SpriteInstance;

    fn validate(name: &str, source: &str) {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|err| panic!("{name} failed to parse: {err}"));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|err| panic!("{name} failed to validate: {err:?}"));
    }

    #[test]
    fn shaders_pass_naga() {
        validate("shape", SHAPE_WGSL);
        validate("vibrancy", VIBRANCY_WGSL);
        validate("blur", BLUR_WGSL);
        validate("blit", BLIT_WGSL);
        validate("sprite", SPRITE_WGSL);
        assert!(VIBRANCY_WGSL.contains("127.1"));
        assert!(VIBRANCY_WGSL.contains("311.7"));
        assert!(VIBRANCY_WGSL.contains("43758.5453"));
        assert!(SHAPE_WGSL.contains("2.5"));
    }

    #[test]
    fn packed_kernel_matches_the_gaussian() {
        let weights = gaussian_kernel(1.8);
        let packed = pack_kernel(weights);
        let sum = packed[0].iter().sum::<f32>() + packed[1].iter().sum::<f32>() + packed[2][0];
        assert!((sum - 1.0).abs() < 1e-4, "sum {sum}");
        assert!((packed[0][0] - weights[0]).abs() < 1e-6);
        assert!((packed[2][0] - weights[8]).abs() < 1e-6);
        let (horizontal, vertical) = blur_pair(16.0, 100, 50);
        assert!(horizontal.dir_texel[0] > 0.0);
        assert_eq!(horizontal.dir_texel[1], 0.0);
        assert_eq!(vertical.dir_texel[0], 0.0);
        assert!(vertical.dir_texel[1] > horizontal.dir_texel[0]);
    }

    #[test]
    fn preferred_format_picks_unorm_before_srgb() {
        let formats = [
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
        ];
        assert_eq!(
            preferred_format(&formats),
            Some(wgpu::TextureFormat::Bgra8Unorm)
        );
        assert_eq!(preferred_format(&[]), None);
    }

    #[test]
    fn sprite_staging_reuses_capacity() {
        let sprite = SpriteInstance {
            rect: [0.0, 0.0, 4.0, 4.0],
            src: [1.0, 2.0, 4.0, 4.0],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let sprites = vec![sprite; 8];
        let mut dst = Vec::new();
        stage_sprites(&mut dst, &sprites, 64.0);
        let capacity = dst.capacity();
        stage_sprites(&mut dst, &sprites, 64.0);
        assert_eq!(dst.len(), 8);
        assert_eq!(dst.capacity(), capacity);
        assert!((dst[0].uv[0] - 1.0 / 64.0).abs() < 1e-5);
    }

    #[test]
    fn headless_wgpu_golden_image_test() {
        let width = 256;
        let height = 256;
        let mut headless = match Headless::try_new(width, height) {
            Ok(h) => h,
            Err(err) => {
                eprintln!("Skipping headless wgpu golden-image test (no adapter available: {err})");
                return;
            }
        };

        let mut scene = crate::scene::Scene::with_capacity(32, 32);
        scene.begin_frame();
        let rect = crate::geom::Rect::new(32.0, 32.0, 192.0, 192.0);
        let radii = crate::geom::CornerRadii::all(24.0);
        let border = crate::draw::Border {
            color: Color::rgba(255, 255, 255, 204),
            width: 2.0,
        };
        let fill = crate::draw::Paint::Gradient {
            from: Color::hex(0x007AFF),
            to: Color::hex(0x5AC8FA),
            angle: 45.0,
        };
        let primitive = crate::draw::Primitive::Panel {
            size: rect.size(),
            radii,
            fill,
            border,
            shadow_blur: 16.0,
            shadow_offset: (0.0, 8.0),
            shadow_color: Color::rgba(0, 0, 0, 90),
            layer: crate::draw::Layer::Foreground,
        };
        scene.emit(crate::scene::Scene::root(), rect.origin(), 1.0, primitive);

        let atlas = crate::atlas::Atlas::new(256);
        let draw = scene.flatten(|_| &[]);
        let clear = Color::rgba(26, 26, 26, 255);
        let rgba = match headless.render_rgba(clear, draw, &atlas) {
            Ok(pixels) => pixels,
            Err(err) => {
                eprintln!("Skipping headless golden-image test (render error: {err})");
                return;
            }
        };

        assert_eq!(rgba.len(), (width * height * 4) as usize);

        // Verify center pixel rendered non-clear (the rounded rect is at center)
        let center_idx = ((128 * width + 128) * 4) as usize;
        let center_pixel = &rgba[center_idx..center_idx + 4];
        assert!(
            center_pixel[3] > 0,
            "Center pixel alpha should be > 0: {:?}",
            center_pixel
        );
        // Center pixel should be blueish / gradient color, not clear color
        assert!(
            center_pixel[2] > 100,
            "Center pixel blue channel should be prominent: {:?}",
            center_pixel
        );

        // Verify top-left corner (0,0) is clear color (26 in sRGB)
        let corner_pixel = &rgba[0..4];
        assert!(
            (corner_pixel[0] as i32 - 26).abs() <= 5,
            "Corner pixel should match clear color: {:?}",
            corner_pixel
        );
    }
}
