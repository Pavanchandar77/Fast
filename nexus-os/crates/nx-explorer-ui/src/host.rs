//! Window and swapchain host behind cfg(windows) with DWM Mica/Acrylic backdrop hooks.

#[cfg(windows)]
use std::sync::Arc;
#[cfg(windows)]
use std::time::Instant;

#[cfg(windows)]
use crate::view_model::ExplorerWindow;
#[cfg(windows)]
use nx_ui::atlas::Atlas;
#[cfg(windows)]
use nx_ui::gpu::{preferred_format, Renderer};
#[cfg(windows)]
use nx_ui::icon::IconCache;
#[cfg(windows)]
use nx_ui::input::Input;
#[cfg(windows)]
use nx_ui::scene::Scene;
#[cfg(windows)]
use nx_ui::text::TextEngine;
#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(windows)]
use winit::application::ApplicationHandler;
#[cfg(windows)]
use winit::dpi::LogicalSize;
#[cfg(windows)]
use winit::event::WindowEvent;
#[cfg(windows)]
use winit::event_loop::ActiveEventLoop;
#[cfg(windows)]
use winit::window::{Window, WindowAttributes, WindowId};

#[cfg(windows)]
pub struct ExplorerApp {
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    renderer: Option<Renderer>,
    explorer_window: ExplorerWindow,
    scene: Scene,
    text: TextEngine,
    icons: IconCache,
    atlas: Atlas,
    input: Input,
    last_frame: Instant,
}

#[cfg(windows)]
impl ExplorerApp {
    pub fn new(explorer_window: ExplorerWindow) -> Option<Self> {
        let text = TextEngine::new().ok()?;
        Some(Self {
            window: None,
            surface: None,
            device: None,
            queue: None,
            surface_config: None,
            renderer: None,
            explorer_window,
            scene: Scene::with_capacity(4096, 16384),
            text,
            icons: IconCache::new(),
            atlas: Atlas::new(1024),
            input: Input::default(),
            last_frame: Instant::now(),
        })
    }
}

#[cfg(windows)]
fn apply_dwm_backdrop(window: &Window, dark: bool) {
    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
            let hwnd = win32_handle.hwnd.get();
            let request = nx_ui::platform::BackdropRequest {
                kind: nx_ui::platform::SystemBackdrop::Mica,
                dark,
                corners: nx_ui::platform::CornerPreference::Round,
            };
            let _ = nx_ui::platform::apply_hwnd(hwnd, request);
        }
    }
}

#[cfg(windows)]
impl ApplicationHandler for ExplorerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("Nexus Explorer")
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(600.0, 400.0));

        let Ok(raw_window) = event_loop.create_window(attrs) else {
            return;
        };
        let window = Arc::new(raw_window);
        apply_dwm_backdrop(&window, self.explorer_window.theme.mode.is_dark());

        let instance = wgpu::Instance::default();
        let Ok(surface) = instance.create_surface(window.clone()) else {
            return;
        };

        let Some(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
        else {
            return;
        };

        let Ok((device, queue)) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Nexus Explorer Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        )) else {
            return;
        };

        let caps = surface.get_capabilities(&adapter);
        let format = preferred_format(&caps.formats).unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = Renderer::new(device.clone(), queue.clone(), format);

        self.window = Some(window);
        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface_config = Some(config);
        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(physical) => {
                if let (Some(config), Some(surface), Some(device)) =
                    (&mut self.surface_config, &self.surface, &self.device)
                {
                    config.width = physical.width.max(1);
                    config.height = physical.height.max(1);
                    surface.configure(device, config);
                    self.explorer_window.viewport =
                        nx_ui::geom::Size::new(physical.width as f32, physical.height as f32);
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last_frame).as_secs_f32().min(0.1);
                self.last_frame = now;

                self.explorer_window.update(&self.input, dt);

                self.scene.begin_frame();
                let mut frame = nx_ui::widgets::Frame {
                    scene: &mut self.scene,
                    text: &mut self.text,
                    icons: &mut self.icons,
                    atlas: &mut self.atlas,
                    theme: self.explorer_window.theme,
                };

                self.explorer_window.paint(&mut frame);
                let draw = self.scene.flatten(|id| self.text.glyphs(id));

                if let (Some(surface), Some(device), Some(config), Some(renderer)) = (
                    &self.surface,
                    &self.device,
                    &self.surface_config,
                    &mut self.renderer,
                ) {
                    let output = match surface.get_current_texture() {
                        Ok(frame) => frame,
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            surface.configure(device, config);
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                            return;
                        }
                        Err(_) => return,
                    };

                    let view = output
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    let clear = self.explorer_window.theme.palette.window_bg;
                    let _ = renderer.render(
                        &view,
                        (config.width, config.height),
                        clear,
                        draw,
                        &self.atlas,
                    );
                    output.present();
                }

                if self.explorer_window.animating() {
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }
}
