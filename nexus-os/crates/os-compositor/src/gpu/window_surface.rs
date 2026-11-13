//! Raw window handle bridge and WGPU surface configuration for Win32 HWND surfaces.

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle as RwhHandle, WindowsDisplayHandle,
};
use std::num::NonZeroIsize;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::UI::Controls::MARGINS;

pub struct Win32WindowWrapper(pub HWND);

impl HasWindowHandle for Win32WindowWrapper {
    fn window_handle(&self) -> Result<RwhHandle<'_>, HandleError> {
        let non_zero = NonZeroIsize::new(self.0 .0 as isize)
            .ok_or(HandleError::Unavailable)?;
        let handle = Win32WindowHandle::new(non_zero);
        unsafe { Ok(RwhHandle::borrow_raw(RawWindowHandle::Win32(handle))) }
    }
}

impl HasDisplayHandle for Win32WindowWrapper {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let handle = WindowsDisplayHandle::new();
        unsafe { Ok(DisplayHandle::borrow_raw(RawDisplayHandle::Windows(handle))) }
    }
}

pub struct ShellSurface {
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub format: wgpu::TextureFormat,
    pub _dcomp_device: Option<IDCompositionDevice>,
    pub _dcomp_target: Option<IDCompositionTarget>,
    pub _dcomp_visual: Option<IDCompositionVisual>,
}

impl ShellSurface {
    pub fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        hwnd: HWND,
        width: u32,
        height: u32,
    ) -> anyhow::Result<Self> {
        // Step 1: Extend glass/frame into client area for pre-multiplied alpha transparency
        unsafe {
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        }

        // Step 2: Create DirectComposition visual or fallback to Win32 HWND surface
        let (dcomp_device, dcomp_target, dcomp_visual, surface_target) = unsafe {
            let dcomp_res: windows::core::Result<(IDCompositionDevice, IDCompositionTarget, IDCompositionVisual)> = (|| {
                let device: IDCompositionDevice = DCompositionCreateDevice(None)?;
                let target: IDCompositionTarget = device.CreateTargetForHwnd(hwnd, true)?;
                let visual: IDCompositionVisual = device.CreateVisual()?;
                target.SetRoot(&visual)?;
                device.Commit()?;
                Ok((device, target, visual))
            })();

            match dcomp_res {
                Ok((d, t, v)) => {
                    let raw_visual = windows::core::Interface::as_raw(&v);
                    (Some(d), Some(t), Some(v), wgpu::SurfaceTargetUnsafe::CompositionVisual(raw_visual))
                }
                Err(e) => {
                    tracing::warn!("DirectComposition init failed ({:?}), falling back to HWND surface", e);
                    let wrapper = Win32WindowWrapper(hwnd);
                    let target = wgpu::SurfaceTargetUnsafe::from_window(&wrapper)?;
                    (None, None, None, target)
                }
            }
        };

        let surface = unsafe { instance.create_surface_unsafe(surface_target)? };

        // Step 3: Determine surface capabilities and preferred format
        let caps = surface.get_capabilities(adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or_else(|| {
                caps.formats
                    .first()
                    .copied()
                    .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb)
            });

        // Prefer PreMultiplied alpha mode for glass/squircle anti-aliased transparency
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied) {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else {
            caps.alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto)
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(device, &config);

        Ok(Self {
            surface,
            config,
            format,
            _dcomp_device: dcomp_device,
            _dcomp_target: dcomp_target,
            _dcomp_visual: dcomp_visual,
        })
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(device, &self.config);
        }
    }
}
