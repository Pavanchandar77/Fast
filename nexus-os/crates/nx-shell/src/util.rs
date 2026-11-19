//! Small helpers shared by the view-models. No allocation surprises, no panics.

/// Clip `text` to at most `max` Unicode scalars, appending an ellipsis when cut.
pub fn truncate_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

/// Take a prefix that ends on a char boundary.
pub fn clip_str(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub fn finite_f32(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

/// Round a finite f32 into i32. Non-finite values become 0.
pub fn px_i32(v: f32) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    let r = v.round();
    if r >= i32::MAX as f32 {
        i32::MAX
    } else if r <= i32::MIN as f32 {
        i32::MIN
    } else {
        r as i32
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    let t = finite_f32(t, 0.0).clamp(0.0, 1.0);
    a + (b - a) * t
}

pub fn smoothstep(t: f32) -> f32 {
    let t = finite_f32(t, 0.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
