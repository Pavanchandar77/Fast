use crate::geometry::{Direction, Point, Rect};
use crate::types::WindowId;

/// Finds the best directional candidate window from `from_window` along `dir`.
/// Evaluates bounding box geometric relations, angle projection, and alignment bonuses.
pub fn find_directional_candidate(
    from_window: WindowId,
    dir: Direction,
    rects: &[(WindowId, Rect)],
) -> Option<WindowId> {
    let from_rect = rects
        .iter()
        .find(|(w, _)| *w == from_window)
        .map(|(_, r)| *r)?;

    let mut best_candidate = None;
    let mut min_score = u64::MAX;

    for &(candidate_id, candidate_rect) in rects {
        if candidate_id == from_window || candidate_rect.is_empty() {
            continue;
        }

        if let Some(score) = from_rect.directional_distance(&candidate_rect, dir) {
            if score < min_score {
                min_score = score;
                best_candidate = Some(candidate_id);
            }
        }
    }

    best_candidate
}

/// Finds the window under the mouse cursor at `point`.
/// Iterates in reverse to respect front-to-back/active layer order.
pub fn find_window_at_point(point: Point, rects: &[(WindowId, Rect)]) -> Option<WindowId> {
    rects
        .iter()
        .rev()
        .find(|(_, r)| r.contains_point(point.x, point.y))
        .map(|(w, _)| *w)
}
