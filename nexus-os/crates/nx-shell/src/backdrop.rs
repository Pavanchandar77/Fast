//! Menu bar, dock, and panel backdrops.
//!
//! On Windows 11 the host applies [`nx_ui::platform::BackdropRequest`] through
//! `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)`. When that attribute is
//! missing (older Windows, or the call fails) the same material falls back to
//! the nx-ui separable Gaussian kernel. Nothing here is stored on disk: DWM
//! attributes die with the HWND, and the fallback is a per-frame shader input.

use nx_ui::material::{gaussian_kernel, Material, MaterialKind};
use nx_ui::platform::{BackdropRequest, CornerPreference, SystemBackdrop};
use nx_ui::theme::Theme;

/// Which compositor path produces the blur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackdropPath {
    /// `DWMWA_SYSTEMBACKDROP_TYPE` Mica / Acrylic / Tabbed.
    DwmSystem,
    /// nx-ui custom blur (`gaussian_kernel` + vibrancy shader).
    CustomBlur,
}

/// A resolved backdrop. `kernel` is always filled so a failed DWM call can
/// downgrade without rebuilding the material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackdropChoice {
    pub request: BackdropRequest,
    pub material: Material,
    pub kernel: [f32; 9],
    pub sigma: f32,
    pub path: BackdropPath,
}

impl BackdropChoice {
    pub fn uses_dwm(self) -> bool {
        matches!(self.path, BackdropPath::DwmSystem)
    }

    /// Drop the system backdrop and keep the custom blur. Used when
    /// `DwmSetWindowAttribute` fails. Still nothing is persisted.
    pub fn downgrade_to_blur(mut self) -> Self {
        self.path = BackdropPath::CustomBlur;
        self.request.kind = SystemBackdrop::None;
        self
    }

    pub fn kernel_sum(self) -> f32 {
        self.kernel.iter().sum()
    }
}

/// Pick Mica, Acrylic, or custom blur using runtime capability probing via `nx-compat`.
pub fn choose_probed(theme: &Theme, prefer: SystemBackdrop) -> BackdropChoice {
    let dwm_supported = nx_compat::is_capability_supported(nx_compat::Capability::DwmMica);
    choose(theme, prefer, dwm_supported)
}

/// Pick Mica, Acrylic, or the custom blur.
///
/// `dwm_supported` is true on Windows 11 22H2+ where
/// `DWMWA_SYSTEMBACKDROP_TYPE` is honored. The host passes that flag.
pub fn choose(theme: &Theme, prefer: SystemBackdrop, dwm_supported: bool) -> BackdropChoice {
    let material = match prefer {
        SystemBackdrop::Acrylic => theme.materials.menu,
        SystemBackdrop::None => Material::solid(theme.palette.window_bg),
        SystemBackdrop::Auto | SystemBackdrop::Mica | SystemBackdrop::Tabbed => theme.materials.bar,
    };
    let system_kind = match prefer {
        SystemBackdrop::Acrylic => SystemBackdrop::Acrylic,
        SystemBackdrop::Tabbed => SystemBackdrop::Tabbed,
        SystemBackdrop::None => SystemBackdrop::None,
        SystemBackdrop::Auto | SystemBackdrop::Mica => SystemBackdrop::Mica,
    };
    let can_dwm =
        dwm_supported && material.kind.blurs() && !matches!(system_kind, SystemBackdrop::None);
    let path = if can_dwm {
        BackdropPath::DwmSystem
    } else {
        BackdropPath::CustomBlur
    };
    let sigma = if material.kind == MaterialKind::Solid {
        0.25
    } else {
        (material.blur / 6.0).clamp(0.25, 12.0)
    };
    BackdropChoice {
        request: BackdropRequest {
            kind: if can_dwm {
                system_kind
            } else {
                SystemBackdrop::None
            },
            dark: theme.mode.is_dark(),
            corners: CornerPreference::Round,
        },
        material,
        kernel: gaussian_kernel(sigma),
        sigma,
        path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nx_ui::theme::Accent;

    #[test]
    fn mica_uses_dwm_when_the_host_supports_it() {
        let theme = Theme::dark(Accent::Blue);
        let choice = choose(&theme, SystemBackdrop::Mica, true);
        assert_eq!(choice.path, BackdropPath::DwmSystem);
        assert_eq!(choice.request.system_backdrop_value(), 2);
        assert_eq!(choice.request.dark_mode_value(), 1);
        assert_eq!(choice.request.corner_value(), 2);
        assert!(choice.material.kind.blurs());
    }

    #[test]
    fn acrylic_value_is_transient_window() {
        let theme = Theme::light(Accent::Purple);
        let choice = choose(&theme, SystemBackdrop::Acrylic, true);
        assert_eq!(choice.request.system_backdrop_value(), 3);
        assert_eq!(choice.request.dark_mode_value(), 0);
        assert_eq!(choice.path, BackdropPath::DwmSystem);
    }

    #[test]
    fn unsupported_dwm_falls_back_to_a_normalized_kernel() {
        let theme = Theme::dark(Accent::Green);
        let choice = choose(&theme, SystemBackdrop::Mica, false);
        assert_eq!(choice.path, BackdropPath::CustomBlur);
        assert_eq!(choice.request.kind, SystemBackdrop::None);
        assert_eq!(choice.kernel.len(), 9);
        let sum = choice.kernel_sum();
        assert!((sum - 1.0).abs() < 1.0e-4, "kernel sum {sum}");
        assert!((choice.kernel[0] - choice.kernel[8]).abs() < 1.0e-5);
        assert!(choice.kernel[4] >= choice.kernel[0]);
    }

    #[test]
    fn downgrade_keeps_the_kernel_and_clears_the_dwm_kind() {
        let theme = Theme::light(Accent::Orange);
        let choice = choose(&theme, SystemBackdrop::Acrylic, true).downgrade_to_blur();
        assert_eq!(choice.path, BackdropPath::CustomBlur);
        assert_eq!(choice.request.kind, SystemBackdrop::None);
        assert!((choice.kernel_sum() - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn solid_preference_does_not_ask_dwm_for_mica() {
        let theme = Theme::light(Accent::Graphite);
        let choice = choose(&theme, SystemBackdrop::None, true);
        assert_eq!(choice.path, BackdropPath::CustomBlur);
        assert_eq!(choice.request.kind, SystemBackdrop::None);
    }
}
