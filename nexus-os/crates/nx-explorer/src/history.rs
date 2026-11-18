//! Back/forward navigation history.

/// Bounded back/forward stacks around a current value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History<T> {
    back: Vec<T>,
    forward: Vec<T>,
    cap: usize,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(cap: usize) -> Self {
        History {
            back: Vec::new(),
            forward: Vec::new(),
            cap: cap.max(1),
        }
    }

    /// Record leaving `current` for a new location.
    pub fn push(&mut self, current: T) {
        if self.back.last() != Some(&current) {
            self.back.push(current);
            if self.back.len() > self.cap {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    /// Go back from `current`; returns the destination.
    pub fn back(&mut self, current: T) -> Option<T> {
        let dest = self.back.pop()?;
        self.forward.push(current);
        Some(dest)
    }

    pub fn forward(&mut self, current: T) -> Option<T> {
        let dest = self.forward.pop()?;
        self.back.push(current);
        Some(dest)
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Most recent first (for the back-button dropdown).
    pub fn back_list(&self) -> impl Iterator<Item = &T> {
        self.back.iter().rev()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_forward_and_cap() {
        let mut h = History::new(3);
        h.push(1);
        h.push(2);
        assert_eq!(h.back(3), Some(2));
        assert_eq!(h.back(2), Some(1));
        assert_eq!(h.back(1), None);
        assert_eq!(h.forward(1), Some(2));
        h.push(2);
        assert!(!h.can_forward(), "new navigation clears forward");
        for i in 10..20 {
            h.push(i);
        }
        assert_eq!(h.back_list().count(), 3);
    }
}
