//! BorderOverlay: Fullscreen click-through window rendering L4-norm squircle active borders at 120Hz.
//! Utilizes event-driven dirty-state rendering to ensure zero GPU load when windows are stationary.

use std::sync::Arc;
use tracing::info;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, RegisterClassExW,
    ShowWindow, CS_HREDRAW, CS_VREDRAW, HMENU,
    SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNOACTIVATE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use wgpu::util::DeviceExt;

use crate::gpu::context::GpuContext;
use crate::gpu::shaders::{SquirclePipeline, SquircleUniforms};
use crate::gpu::window_surface::ShellSurface;
use crate::spring::{SpringConfig, SpringRect};

unsafe extern "system" fn border_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub struct BorderOverlay {
    pub hwnd: HWND,
    gpu: Arc<GpuContext>,
    surface: ShellSurface,
    pipeline: SquirclePipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    spring: SpringRect,
    screen_width: u32,
    screen_height: u32,
    is_dirty: bool,
    visible: bool,
}

pub fn create_border_overlay_window() -> anyhow::Result<HWND> {
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as u32;
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) } as u32;

    info!(
        "Creating Fullscreen Border Overlay Surface: {}x{} px",
        screen_width, screen_height
    );

    let class_name = w!("NexusBorderOverlaySurface");
    let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(border_wnd_proc),
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
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            class_name,
            w!("Nexus-OS Border Overlay"),
            WS_POPUP,
            0,
            0,
            screen_width as i32,
            screen_height as i32,
            HWND::default(),
            HMENU::default(),
            hinstance,
            None,
        )?
    };

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }

    Ok(hwnd)
}

impl BorderOverlay {
    pub fn new(gpu: Arc<GpuContext>, hwnd: HWND, spring_cfg: SpringConfig) -> anyhow::Result<Self> {
        let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as u32;
        let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) } as u32;

        let surface = ShellSurface::new(
            &gpu.instance,
            &gpu.adapter,
            &gpu.device,
            hwnd,
            screen_width,
            screen_height,
        )?;

        let pipeline = SquirclePipeline::new(&gpu.device, surface.format);

        let mut initial_uniforms = SquircleUniforms::default();
        initial_uniforms.viewport_size = [screen_width as f32, screen_height as f32];

        let uniform_buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("border_overlay_uniform_buffer"),
            contents: bytemuck::bytes_of(&initial_uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("border_overlay_bind_group"),
            layout: &pipeline.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let spring = SpringRect::new(0.0, 0.0, 0.0, 0.0, spring_cfg);

        Ok(Self {
            hwnd,
            gpu,
            surface,
            pipeline,
            uniform_buffer,
            bind_group,
            spring,
            screen_width,
            screen_height,
            is_dirty: true,
            visible: true,
        })
    }

    /// Set target bounding rectangle for the focused window [x, y, w, h]
    pub fn set_target_rect(&mut self, target: [f32; 4]) {
        self.spring.set_target(target[0], target[1], target[2], target[3]);
        self.is_dirty = true;
    }

    /// Step spring physics animation
    pub fn update(&mut self, dt: f32) -> bool {
        if !self.spring.is_at_rest() {
            self.spring.step(dt);
            self.is_dirty = true;
        }
        self.is_dirty
    }

    /// Renders the L4-norm squircle border frame if dirty
    pub fn render(&mut self) -> anyhow::Result<()> {
        if !self.is_dirty || !self.visible {
            return Ok(());
        }

        let (x, y, w, h) = self.spring.current_rect();
        let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);

        // If window is degenerate or minimized, clear and rest
        if w < 10.0 || h < 10.0 {
            let output = self.surface.surface.get_current_texture()?;
            let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("border_overlay_clear_encoder"),
            });
            {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("border_overlay_clear_pass"),
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
            }
            self.gpu.queue.submit(std::iter::once(encoder.finish()));
            output.present();
            self.is_dirty = false;
            return Ok(());
        }

        // Center and size of current squircle rect
        let center = [x + w * 0.5, y + h * 0.5];
        let size = [w, h];

        let uniforms = SquircleUniforms {
            rect_center: center,
            rect_size: size,
            corner_radius: 12.0,
            border_width: 2.0,
            _pad0: [0.0, 0.0],
            border_color: [0.23, 0.51, 0.96, 0.95],   // macOS-style accent blue
            specular_color: [1.0, 1.0, 1.0, 0.90],   // 1px specular highlight
            glow_color: [0.23, 0.51, 0.96, 0.20],
            viewport_size: [self.screen_width as f32, self.screen_height as f32],
            specular_intensity: 1.0,
            _pad1: 0.0,
        };

        self.gpu.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&uniforms),
        );

        let output = self.surface.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("border_overlay_render_encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("border_overlay_pass"),
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

            pass.set_pipeline(&self.pipeline.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // If the spring is at rest, clear the dirty flag to suspend rendering loop
        if self.spring.is_at_rest() {
            self.is_dirty = false;
        }

        Ok(())
    }
}

impl Drop for BorderOverlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
