//! DWM backdrop and corner values.
//!
//! The integers match `DWMWA_SYSTEMBACKDROP_TYPE` (`DWMSBT_*`) and
//! `DWMWA_WINDOW_CORNER_PREFERENCE` (`DWMWCP_*`) so the pure core and the
//! Win32 hook cannot drift.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemBackdrop {
    /// `DWMSBT_AUTO` (0).
    Auto,
    /// `DWMSBT_NONE` (1).
    None,
    /// `DWMSBT_MAINWINDOW` (2), Mica.
    Mica,
    /// `DWMSBT_TRANSIENTWINDOW` (3), Acrylic.
    Acrylic,
    /// `DWMSBT_TABBEDWINDOW` (4), tabbed Mica.
    Tabbed,
}

impl SystemBackdrop {
    pub const fn dwm_value(self) -> i32 {
        match self {
            Self::Auto => 0,
            Self::None => 1,
            Self::Mica => 2,
            Self::Acrylic => 3,
            Self::Tabbed => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerPreference {
    /// `DWMWCP_DEFAULT` (0).
    Default,
    /// `DWMWCP_DONOTROUND` (1).
    DoNotRound,
    /// `DWMWCP_ROUND` (2).
    Round,
    /// `DWMWCP_ROUNDSMALL` (3).
    RoundSmall,
}

impl CornerPreference {
    pub const fn dwm_value(self) -> i32 {
        match self {
            Self::Default => 0,
            Self::DoNotRound => 1,
            Self::Round => 2,
            Self::RoundSmall => 3,
        }
    }
}

/// What to ask DWM for. Built on every platform; applied only on Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackdropRequest {
    pub kind: SystemBackdrop,
    pub dark: bool,
    pub corners: CornerPreference,
}

impl BackdropRequest {
    pub fn system_backdrop_value(self) -> i32 {
        self.kind.dwm_value()
    }

    pub fn corner_value(self) -> i32 {
        self.corners.dwm_value()
    }

    /// `DWMWA_USE_IMMERSIVE_DARK_MODE` is a BOOL, 0 or 1.
    pub fn dark_mode_value(self) -> i32 {
        i32::from(self.dark)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_values_match_dwm_constants() {
        assert_eq!(SystemBackdrop::Auto.dwm_value(), 0);
        assert_eq!(SystemBackdrop::None.dwm_value(), 1);
        assert_eq!(SystemBackdrop::Mica.dwm_value(), 2);
        assert_eq!(SystemBackdrop::Acrylic.dwm_value(), 3);
        assert_eq!(SystemBackdrop::Tabbed.dwm_value(), 4);
    }

    #[test]
    fn corner_values_match_dwm_constants() {
        assert_eq!(CornerPreference::Default.dwm_value(), 0);
        assert_eq!(CornerPreference::DoNotRound.dwm_value(), 1);
        assert_eq!(CornerPreference::Round.dwm_value(), 2);
        assert_eq!(CornerPreference::RoundSmall.dwm_value(), 3);
    }

    #[test]
    fn request_packs_dark_mode_as_bool() {
        let mica = BackdropRequest {
            kind: SystemBackdrop::Mica,
            dark: true,
            corners: CornerPreference::Round,
        };
        assert_eq!(mica.system_backdrop_value(), 2);
        assert_eq!(mica.corner_value(), 2);
        assert_eq!(mica.dark_mode_value(), 1);
        let light = BackdropRequest {
            dark: false,
            ..mica
        };
        assert_eq!(light.dark_mode_value(), 0);
    }
}
