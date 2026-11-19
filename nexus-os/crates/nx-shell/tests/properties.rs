//! Property tests for nx-shell view models, layout geometry, fuzzy search, and scheduler.

use nx_shell::*;
use nx_ui::geom::{Point, Rect};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(250))]

    #[test]
    fn prop_truncate_and_clip_never_panics(s in "\\PC*", max_len in 0usize..100) {
        let truncated = nx_shell::util::truncate_chars(&s, max_len);
        let clipped = nx_shell::util::clip_str(&s, max_len);
        prop_assert!(truncated.chars().count() <= max_len + 1);
        prop_assert!(clipped.len() <= max_len);
        prop_assert!(s.starts_with(clipped));
    }

    #[test]
    fn prop_fuzzy_score_matches_subsequences(
        q in "[a-z0-9]{1,10}",
        target in "[a-zA-Z0-9 -_]{1,30}"
    ) {
        let folded = target.to_ascii_lowercase();
        let score = fuzzy_score(q.as_bytes(), folded.as_bytes(), target.as_bytes());
        if let Some(s) = score {
            prop_assert!(s > 0);
            // Verify q is genuinely a subsequence of folded
            let mut it = folded.bytes();
            for qb in q.bytes() {
                prop_assert!(it.any(|b| b == qb), "q {q} must be subsequence of folded {folded}");
            }
        }
    }

    #[test]
    fn prop_calculator_arbitrary_strings_never_panic(s in "\\PC{0,50}") {
        let _ = eval_calculator(&s);
    }

    #[test]
    fn prop_dock_layout_never_overlaps(
        scales in proptest::collection::vec(0.5f32..2.0f32, 1..12),
        bounces in proptest::collection::vec(-20.0f32..20.0f32, 1..12),
        width in 400.0f32..2560.0f32,
        height in 300.0f32..1440.0f32,
    ) {
        let bounds = Rect::new(0.0, 0.0, width, height);
        let placement = DockPlacement::bottom(nx_wm::MonitorId(0), bounds);
        let mag = Magnification::macos(1.0);
        let running = vec![true; scales.len()];

        let layout = layout_dock(&scales, &bounces, &running, placement, mag, 1.0);
        prop_assert_eq!(layout.icons.len(), scales.len());

        for pair in layout.icons.windows(2) {
            prop_assert!(
                !pair[0].rect.intersects(pair[1].rect),
                "Dock icons overlap: {:?} vs {:?}",
                pair[0].rect,
                pair[1].rect
            );
        }
    }

    #[test]
    fn prop_expose_layout_no_overlaps_and_bounds_contain(
        counts in 1usize..16,
        canvas_w in 600.0f32..3840.0f32,
        canvas_h in 400.0f32..2160.0f32,
    ) {
        let canvas = Rect::new(0.0, 28.0, canvas_w, canvas_h);
        let windows: Vec<ExposeWindow> = (0..counts)
            .map(|i| ExposeWindow {
                id: nx_wm::WindowId((i + 1) as u64),
                title: format!("Window {i}"),
                order: i as u64,
                source: nx_ui::geom::Size::new(400.0 + (i as f32 * 100.0) % 800.0, 300.0 + (i as f32 * 60.0) % 600.0),
                from: Rect::new(20.0 * i as f32, 40.0, 500.0, 400.0),
            })
            .collect();

        let tiles = layout_expose(&windows, canvas, ExposeParams::default());
        prop_assert_eq!(tiles.len(), counts);

        for pair in tiles.windows(2) {
            prop_assert!(
                !pair[0].content.intersects(pair[1].content),
                "Expose tiles content overlap: {:?} vs {:?}",
                pair[0].content,
                pair[1].content
            );
            prop_assert!(
                !pair[0].cell.intersects(pair[1].cell),
                "Expose tiles cell overlap: {:?} vs {:?}",
                pair[0].cell,
                pair[1].cell
            );
        }

        for tile in &tiles {
            prop_assert!(tile.content.x >= canvas.x - 0.5);
            prop_assert!(tile.content.y >= canvas.y - 0.5);
            prop_assert!(tile.content.right() <= canvas.right() + 0.5);
            prop_assert!(tile.content.bottom() <= canvas.bottom() + 0.5);
        }
    }

    #[test]
    fn prop_squircle_contains_center_and_smoothstep(
        w in 20.0f32..500.0f32,
        h in 20.0f32..500.0f32,
        radius in 0.0f32..100.0f32,
        smoothing in 0.0f32..1.0f32,
    ) {
        let rect = Rect::new(10.0, 10.0, w, h);
        let squircle = Squircle::new(rect, radius, smoothing);
        let center = rect.center();
        prop_assert!(squircle.contains(center));
        prop_assert!(squircle.signed_distance(center) < 0.0);

        // Outside bounding box + 10px is never contained
        let outside = Point::new(rect.right() + 10.0, rect.bottom() + 10.0);
        prop_assert!(!squircle.contains(outside));
        prop_assert!(squircle.signed_distance(outside) > 0.0);
    }

    #[test]
    fn prop_scheduler_idle_never_draws(
        steps in 1usize..60,
        rate in 30.0f32..240.0f32,
    ) {
        let mut scheduler = FrameScheduler::new(rate);
        for i in 0..steps {
            let decision = scheduler.begin(i as f64 * (1.0 / rate as f64), false, false);
            prop_assert!(matches!(decision, nx_ui::FrameDecision::Idle));
            prop_assert!(!scheduler.should_paint());
        }
        prop_assert_eq!(scheduler.redraws(), 0);
        prop_assert_eq!(scheduler.skipped(), steps as u64);
    }
}
