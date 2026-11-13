//! TopBar: 28px macOS-refined Top Menu Bar.
//! Registered via SHAppBarMessage (ABM_NEW / ABM_SETPOS with ABE_TOP) to reserve desktop work area.

use std::sync::Arc;
use std::time::Instant;
use tracing::info;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABM_NEW, ABM_REMOVE, ABM_SETPOS, ABE_TOP, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, RegisterClassExW,
    ShowWindow, CS_HREDRAW, CS_VREDRAW, HMENU,
    SM_CXSCREEN, SW_SHOWNOACTIVATE, WNDCLASSEXW, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use wgpu::util::DeviceExt;
use bytemuck::{Pod, Zeroable};

use crate::gpu::context::GpuContext;
use crate::gpu::window_surface::ShellSurface;

pub const TOP_BAR_HEIGHT: u32 = 28;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct TopBarUniforms {
    pub viewport_size: [f32; 2],
    pub active_workspace: u32,
    pub num_workspaces: u32,
    pub cpu_percent: f32,
    pub ram_gb: f32,
    pub _pad: [f32; 2],
}

unsafe extern "system" fn top_bar_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

static TOP_BAR_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

pub struct TopBar {
    pub hwnd: HWND,
    gpu: Arc<GpuContext>,
    surface: ShellSurface,
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    screen_width: u32,
    active_workspace: u32,
    num_workspaces: u32,
    cpu_percent: f32,
    ram_gb: f32,
    last_telemetry_tick: Instant,
    is_dirty: bool,
}

pub fn create_top_bar_window() -> anyhow::Result<HWND> {
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as u32;

    info!(
        "Initializing 28px Top Menu Bar Surface: {}x{} px",
        screen_width, TOP_BAR_HEIGHT
    );

    let class_name = w!("NexusTopBarSurface");
    let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(top_bar_wnd_proc),
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
            w!("Nexus-OS Top Bar"),
            WS_POPUP,
            0,
            0,
            screen_width as i32,
            TOP_BAR_HEIGHT as i32,
            HWND::default(),
            HMENU::default(),
            hinstance,
            None,
        )?
    };

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }

    // Register as an official Windows AppBar to dock and reserve the desktop area
    unsafe {
        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: 0,
            uEdge: ABE_TOP,
            rc: RECT {
                left: 0,
                top: 0,
                right: screen_width as i32,
                bottom: TOP_BAR_HEIGHT as i32,
            },
            lParam: LPARAM(0),
        };
        SHAppBarMessage(ABM_NEW, &mut abd);
        SHAppBarMessage(ABM_SETPOS, &mut abd);
        info!("Registered Top Bar with SHAppBarMessage (ABM_NEW / ABM_SETPOS ABE_TOP)");
    }

    TOP_BAR_HWND.store(hwnd.0 as isize, std::sync::atomic::Ordering::SeqCst);

    Ok(hwnd)
}

impl TopBar {
    pub fn new(gpu: Arc<GpuContext>, hwnd: HWND) -> anyhow::Result<Self> {
        let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as u32;

        let surface = ShellSurface::new(
            &gpu.instance,
            &gpu.adapter,
            &gpu.device,
            hwnd,
            screen_width,
            TOP_BAR_HEIGHT,
        )?;

        // Build top bar WGSL pipeline
        let shader_source = include_str!("shaders/top_bar.wgsl");
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("top_bar_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let bind_group_layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("top_bar_bind_group_layout"),
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
            label: Some("top_bar_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("top_bar_render_pipeline"),
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

        let initial_uniforms = TopBarUniforms {
            viewport_size: [screen_width as f32, TOP_BAR_HEIGHT as f32],
            active_workspace: 1,
            num_workspaces: 5,
            cpu_percent: 3.5,
            ram_gb: 4.2,
            _pad: [0.0, 0.0],
        };

        let uniform_buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("top_bar_uniform_buffer"),
            contents: bytemuck::bytes_of(&initial_uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("top_bar_bind_group"),
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
            screen_width,
            active_workspace: 1,
            num_workspaces: 5,
            cpu_percent: 3.5,
            ram_gb: 4.2,
            last_telemetry_tick: Instant::now(),
            is_dirty: true,
        })
    }

    pub fn set_active_workspace(&mut self, ws: u32) {
        if self.active_workspace != ws {
            self.active_workspace = ws;
            self.is_dirty = true;
        }
    }

    pub fn update_telemetry(&mut self, cpu: f32, ram_gb: f32) {
        self.cpu_percent = cpu;
        self.ram_gb = ram_gb;
        self.is_dirty = true;
    }

    pub fn step(&mut self) -> bool {
        // 1Hz telemetry periodic update
        if self.last_telemetry_tick.elapsed().as_secs_f32() >= 1.0 {
            self.last_telemetry_tick = Instant::now();
            self.is_dirty = true;
        }
        self.is_dirty
    }

    pub fn render(&mut self) -> anyhow::Result<()> {
        if !self.is_dirty {
            return Ok(());
        }

        let uniforms = TopBarUniforms {
            viewport_size: [self.screen_width as f32, TOP_BAR_HEIGHT as f32],
            active_workspace: self.active_workspace,
            num_workspaces: self.num_workspaces,
            cpu_percent: self.cpu_percent,
            ram_gb: self.ram_gb,
            _pad: [0.0, 0.0],
        };

        self.gpu.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&uniforms),
        );

        let output = self.surface.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("top_bar_render_encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("top_bar_pass"),
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

    /// Global fail-safe unregister for TopBar AppBar reservation and window destruction.
    pub fn unregister_global() {
        let hwnd_raw = TOP_BAR_HWND.swap(0, std::sync::atomic::Ordering::SeqCst);
        if hwnd_raw != 0 {
            unsafe {
                let hwnd = HWND(hwnd_raw as *mut _);
                let mut abd = APPBARDATA {
                    cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                    hWnd: hwnd,
                    uCallbackMessage: 0,
                    uEdge: ABE_TOP,
                    rc: RECT::default(),
                    lParam: LPARAM(0),
                };
                SHAppBarMessage(ABM_REMOVE, &mut abd);
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

impl Drop for TopBar {
    fn drop(&mut self) {
        Self::unregister_global();
    }
}
