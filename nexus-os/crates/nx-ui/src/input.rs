//! Platform-free pointer and keyboard edges for one frame.
//!
//! The gallery example fills this from winit. Widgets only see this struct,
//! so they run under `cargo test` with no window.

use crate::geom::Point;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub pointer: Point,
    pub primary_down: bool,
    pub primary_pressed: bool,
    pub primary_released: bool,
    pub secondary_pressed: bool,
    pub scroll_y: f32,
    pub backspace: bool,
    pub escape: bool,
    pub typed: [char; 8],
    pub typed_len: usize,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            pointer: Point::new(0.0, 0.0),
            primary_down: false,
            primary_pressed: false,
            primary_released: false,
            secondary_pressed: false,
            scroll_y: 0.0,
            backspace: false,
            escape: false,
            typed: ['\0'; 8],
            typed_len: 0,
        }
    }
}

impl Input {
    pub fn any_edge(&self) -> bool {
        self.primary_pressed
            || self.primary_released
            || self.secondary_pressed
            || self.scroll_y != 0.0
            || self.backspace
            || self.escape
            || self.typed_len > 0
    }

    pub fn push_char(&mut self, ch: char) {
        if self.typed_len < self.typed.len() && !ch.is_control() {
            self.typed[self.typed_len] = ch;
            self.typed_len += 1;
        }
    }

    pub fn typed_str(&self) -> String {
        let mut out = String::new();
        for ch in self.typed.iter().take(self.typed_len) {
            out.push(*ch);
        }
        out
    }

    pub fn clear_edges(&mut self) {
        self.primary_pressed = false;
        self.primary_released = false;
        self.secondary_pressed = false;
        self.scroll_y = 0.0;
        self.backspace = false;
        self.escape = false;
        self.typed_len = 0;
    }
}
