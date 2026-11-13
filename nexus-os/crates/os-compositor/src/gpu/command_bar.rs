//! CommandBar: 680x420 centered floating Spotlight / Command Center surface.
//! Created hidden at startup and toggled in <2ms upon Super+Space without window recreate latency.

use std::sync::Arc;
use tracing::info;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, RegisterClassExW,
    SetForegroundWindow, ShowWindow, CS_HREDRAW, CS_VREDRAW,
    HMENU, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW, WNDCLASSEXW,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use wgpu::util::DeviceExt;
use bytemuck::{Pod, Zeroable};

use crate::gpu::context::GpuContext;
use crate::gpu::window_surface::ShellSurface;

pub const COMMAND_BAR_WIDTH: u32 = 680;
pub const COMMAND_BAR_HEIGHT: u32 = 420;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct CommandBarUniforms {
    pub viewport_size: [f32; 2],
    pub selected_index: u32,
    pub anim_progress: f32,
    pub _pad: [f32; 4],
}

unsafe extern "system" fn command_bar_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub struct CommandBar {
    pub hwnd: HWND,
    gpu: Arc<GpuContext>,
    surface: ShellSurface,
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    is_visible: bool,
    selected_index: u32,
    is_dirty: bool,
}

pub fn create_command_bar_window() -> anyhow::Result<HWND> {
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as i32;
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) } as i32;

    let x = (screen_width - COMMAND_BAR_WIDTH as i32) / 2;
    let y = (screen_height - COMMAND_BAR_HEIGHT as i32) / 3;

    info!(
        "Initializing Command Bar Surface: {}x{} px at ({}, {})",
        COMMAND_BAR_WIDTH, COMMAND_BAR_HEIGHT, x, y
    );

    let class_name = w!("NexusCommandBarSurface");
    let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(command_bar_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance.into(),
        hIcon: Default::default(),
        hCursor: Default::default(),
        hbrBackground: HBRUSH(std::ptr::null_mut()),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: class_name,
        hIconSm: Default::default(),
    };

    unsafe {
        let _ = RegisterClassExW(&wc);
    }

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Nexus-OS Command Center"),
            WS_POPUP,
            x,
            y,
            COMMAND_BAR_WIDTH as i32,
            COMMAND_BAR_HEIGHT as i32,
            HWND::default(),
            HMENU::default(),
            hinstance,
            None,
        )?
    };

    unsafe {
        // Initially hidden: <2ms instant toggle on hotkey
        let _ = ShowWindow(hwnd, SW_HIDE);
    }

    Ok(hwnd)
}

impl CommandBar {
    pub fn new(gpu: Arc<GpuContext>, hwnd: HWND) -> anyhow::Result<Self> {
        let surface = ShellSurface::new(
            &gpu.instance,
            &gpu.adapter,
            &gpu.device,
            hwnd,
            COMMAND_BAR_WIDTH,
            COMMAND_BAR_HEIGHT,
        )?;

        let shader_source = include_str!("shaders/command_bar.wgsl");
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("command_bar_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let bind_group_layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("command_bar_bind_group_layout"),
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

        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("command_bar_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("command_bar_render_pipeline"),
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
                    format: surface.format,
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

        let initial_uniforms = CommandBarUniforms {
            viewport_size: [COMMAND_BAR_WIDTH as f32, COMMAND_BAR_HEIGHT as f32],
            selected_index: 0,
            anim_progress: 1.0,
            _pad: [0.0; 4],
        };

        let uniform_buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("command_bar_uniform_buffer"),
            contents: bytemuck::bytes_of(&initial_uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("command_bar_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        Ok(Self {
            hwnd,
            gpu,
            surface,
            pipeline,
            uniform_buffer,
            bind_group,
            is_visible: false,
            selected_index: 0,
            is_dirty: true,
        })
    }

    /// Fast toggle in <2ms
    pub fn toggle(&mut self) {
        self.is_visible = !self.is_visible;
        unsafe {
            if self.is_visible {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                let _ = SetForegroundWindow(self.hwnd);
            } else {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
        self.is_dirty = true;
    }

    pub fn is_open(&self) -> bool {
        self.is_visible
    }

    pub fn select_next(&mut self) {
        self.selected_index = (self.selected_index + 1) % 5;
        self.is_dirty = true;
    }

    pub fn select_prev(&mut self) {
        if self.selected_index == 0 {
            self.selected_index = 4;
        } else {
            self.selected_index -= 1;
        }
        self.is_dirty = true;
    }

    pub fn render(&mut self) -> anyhow::Result<()> {
        if !self.is_dirty || !self.is_visible {
            return Ok(());
        }

        let uniforms = CommandBarUniforms {
            viewport_size: [COMMAND_BAR_WIDTH as f32, COMMAND_BAR_HEIGHT as f32],
            selected_index: self.selected_index,
            anim_progress: 1.0,
            _pad: [0.0; 4],
        };

        self.gpu.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&uniforms),
        );

        let output = self.surface.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("command_bar_render_encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("command_bar_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        self.is_dirty = false;
        Ok(())
    }
}

impl Drop for CommandBar {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
