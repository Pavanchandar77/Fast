use std::sync::Arc;
use std::time::Instant;

use nx_ui::atlas::Atlas;
use nx_ui::gallery::Gallery;
use nx_ui::geom::{Point, Size};
use nx_ui::gpu::{preferred_format, Renderer};
use nx_ui::icon::IconCache;
use nx_ui::input::Input;
use nx_ui::scene::Scene;
use nx_ui::text::TextEngine;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

struct App {
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    renderer: Option<Renderer>,
    gallery: Gallery,
    scene: Scene,
    text: TextEngine,
    icons: IconCache,
    atlas: Atlas,
    input: Input,
    last_frame: Instant,
    last_theme_dark: bool,
}

impl App {
    fn new() -> Self {
        let text = TextEngine::new().expect("Failed to initialize text engine");
        let gallery = Gallery::new(100_000);
        let dark = gallery.theme.mode.is_dark();
        Self {
            window: None,
            surface: None,
            device: None,
            queue: None,
            surface_config: None,
            renderer: None,
            gallery,
            scene: Scene::with_capacity(2048, 8192),
            text,
            icons: IconCache::new(),
            atlas: Atlas::new(1024),
            input: Input::default(),
            last_frame: Instant::now(),
            last_theme_dark: dark,
        }
    }
}

#[cfg(windows)]
fn update_windows_backdrop(window: &Window, dark: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
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

#[cfg(not(windows))]
fn update_windows_backdrop(_window: &Window, _dark: bool) {}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("Nexus-OS Gallery")
            .with_inner_size(LogicalSize::new(1280.0, 800.0));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("Failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };

        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = match instance.create_surface(window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to create wgpu surface: {e}");
                event_loop.exit();
                return;
            }
        };

        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })) {
                Some(a) => a,
                None => {
                    eprintln!("Failed to find suitable GPU adapter");
                    event_loop.exit();
                    return;
                }
            };

        let limits = adapter.limits();
        let mut required_limits =
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(limits.clone());
        required_limits.max_inter_stage_shader_components =
            limits.max_inter_stage_shader_components;

        let (device, queue) = match pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("nx-ui-gallery"),
                required_features: wgpu::Features::empty(),
                required_limits,
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        )) {
            Ok(dq) => dq,
            Err(e) => {
                eprintln!("Failed to create GPU device: {e}");
                event_loop.exit();
                return;
            }
        };

        let caps = surface.get_capabilities(&adapter);
        let format = preferred_format(&caps.formats).unwrap_or(wgpu::TextureFormat::Bgra8Unorm);

        let alpha_mode = if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
        {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else {
            caps.alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto)
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let renderer = Renderer::new(device.clone(), queue.clone(), format);

        update_windows_backdrop(&window, self.gallery.theme.mode.is_dark());

        self.window = Some(window);
        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface_config = Some(surface_config);
        self.renderer = Some(renderer);
        self.last_frame = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let (Some(surface), Some(device), Some(config)) =
                    (&self.surface, &self.device, &mut self.surface_config)
                {
                    config.width = size.width.max(1);
                    config.height = size.height.max(1);
                    surface.configure(device, config);
                }
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input.pointer = Point::new(position.x as f32, position.y as f32);
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                match button {
                    MouseButton::Left => {
                        if state == ElementState::Pressed {
                            self.input.primary_pressed = true;
                            self.input.primary_down = true;
                        } else {
                            self.input.primary_released = true;
                            self.input.primary_down = false;
                        }
                    }
                    MouseButton::Right if state == ElementState::Pressed => {
                        self.input.secondary_pressed = true;
                    }
                    _ => {}
                }
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                match delta {
                    MouseScrollDelta::LineDelta(_x, y) => {
                        self.input.scroll_y -= y * 36.0;
                    }
                    MouseScrollDelta::PixelDelta(pos) => {
                        self.input.scroll_y -= pos.y as f32;
                    }
                }
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match event.logical_key {
                        Key::Named(NamedKey::Escape) => self.input.escape = true,
                        Key::Named(NamedKey::Backspace) => self.input.backspace = true,
                        Key::Character(ref s) => {
                            for ch in s.chars() {
                                self.input.push_char(ch);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
                self.last_frame = now;

                let (Some(surface), Some(device), Some(renderer), Some(config), Some(window)) = (
                    &self.surface,
                    &self.device,
                    &mut self.renderer,
                    &self.surface_config,
                    &self.window,
                ) else {
                    return;
                };

                let width = config.width;
                let height = config.height;
                self.gallery.viewport = Size::new(width as f32, height as f32);

                self.gallery.update(&self.input, dt);
                self.input.clear_edges();

                if self.last_theme_dark != self.gallery.theme.mode.is_dark() {
                    self.last_theme_dark = self.gallery.theme.mode.is_dark();
                    update_windows_backdrop(window, self.last_theme_dark);
                }

                self.gallery.paint(
                    &mut self.scene,
                    &mut self.text,
                    &mut self.icons,
                    &mut self.atlas,
                );
                let draw = self.scene.flatten(|id| self.text.glyphs(id));

                let output = match surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        surface.configure(device, config);
                        window.request_redraw();
                        return;
                    }
                    Err(e) => {
                        eprintln!("Surface error: {e}");
                        return;
                    }
                };

                let view = output
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let clear = self.gallery.theme.palette.window_bg;
                let _ = renderer.render(&view, (width, height), clear, draw, &self.atlas);
                output.present();

                if self.gallery.animating() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new();
    let _ = event_loop.run_app(&mut app);
}
