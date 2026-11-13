//! GPU Shader & Render Pipeline builder for Nexus-OS UI surfaces.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SquircleUniforms {
    pub rect_center: [f32; 2],
    pub rect_size: [f32; 2],
    pub corner_radius: f32,
    pub border_width: f32,
    pub _pad0: [f32; 2],
    pub border_color: [f32; 4],
    pub specular_color: [f32; 4],
    pub glow_color: [f32; 4],
    pub viewport_size: [f32; 2],
    pub specular_intensity: f32,
    pub _pad1: f32,
}

impl Default for SquircleUniforms {
    fn default() -> Self {
        Self {
            rect_center: [0.0, 0.0],
            rect_size: [100.0, 100.0],
            corner_radius: 12.0,
            border_width: 2.0,
            _pad0: [0.0, 0.0],
            border_color: [0.38, 0.65, 0.98, 0.95],   // Apple/Aqua accent blue
            specular_color: [1.0, 1.0, 1.0, 0.85],   // 1px specular highlight
            glow_color: [0.38, 0.65, 0.98, 0.25],       // Soft accent aura
            viewport_size: [1920.0, 1080.0],
            specular_intensity: 1.0,
            _pad1: 0.0,
        }
    }
}

pub struct SquirclePipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
}

impl SquirclePipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader_source = include_str!("shaders/squircle.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("squircle_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("squircle_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("squircle_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("squircle_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }
}
