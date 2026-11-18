use crate::geometry::Rect;
use serde::{Deserialize, Serialize};

/// Standard and multi-column snap zones that cleanly coexist with Windows 11 Snap Layouts & FancyZones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SnapZone {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    LeftThird,
    CenterThird,
    RightThird,
    LeftTwoThirds,
    RightTwoThirds,
    Maximize,
}

/// Computes the target snapped rectangle within `work_area` for a given `SnapZone`,
/// respecting outer margin / gaps.
pub fn compute_snap_rect(zone: SnapZone, work_area: Rect, gap: u32) -> Rect {
    let usable = work_area.shrink(gap);
    if usable.is_empty() {
        return work_area;
    }

    let half_w = usable.width.saturating_sub(gap) / 2;
    let right_half_w = usable.width.saturating_sub(half_w + gap);

    let half_h = usable.height.saturating_sub(gap) / 2;
    let bottom_half_h = usable.height.saturating_sub(half_h + gap);

    let third_w = usable.width.saturating_sub(gap * 2) / 3;
    let two_thirds_w = usable.width.saturating_sub(third_w + gap);

    match zone {
        SnapZone::Maximize => usable,
        SnapZone::LeftHalf => Rect::new(usable.x, usable.y, half_w, usable.height),
        SnapZone::RightHalf => Rect::new(
            usable.x + (half_w + gap) as i32,
            usable.y,
            right_half_w,
            usable.height,
        ),
        SnapZone::TopHalf => Rect::new(usable.x, usable.y, usable.width, half_h),
        SnapZone::BottomHalf => Rect::new(
            usable.x,
            usable.y + (half_h + gap) as i32,
            usable.width,
            bottom_half_h,
        ),
        SnapZone::TopLeft => Rect::new(usable.x, usable.y, half_w, half_h),
        SnapZone::TopRight => Rect::new(
            usable.x + (half_w + gap) as i32,
            usable.y,
            right_half_w,
            half_h,
        ),
        SnapZone::BottomLeft => Rect::new(
            usable.x,
            usable.y + (half_h + gap) as i32,
            half_w,
            bottom_half_h,
        ),
        SnapZone::BottomRight => Rect::new(
            usable.x + (half_w + gap) as i32,
            usable.y + (half_h + gap) as i32,
            right_half_w,
            bottom_half_h,
        ),
        SnapZone::LeftThird => Rect::new(usable.x, usable.y, third_w, usable.height),
        SnapZone::CenterThird => Rect::new(
            usable.x + (third_w + gap) as i32,
            usable.y,
            third_w,
            usable.height,
        ),
        SnapZone::RightThird => Rect::new(
            usable.x + ((third_w + gap) * 2) as i32,
            usable.y,
            usable.width.saturating_sub((third_w + gap) * 2),
            usable.height,
        ),
        SnapZone::LeftTwoThirds => Rect::new(usable.x, usable.y, two_thirds_w, usable.height),
        SnapZone::RightTwoThirds => Rect::new(
            usable.x + (third_w + gap) as i32,
            usable.y,
            usable.width.saturating_sub(third_w + gap),
            usable.height,
        ),
    }
}
