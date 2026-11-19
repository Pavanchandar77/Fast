//! Mission Control / Exposé.
//!
//! The grid is pure: windows on one workspace, stable tree order, no overlaps,
//! source aspect preserved. Live thumbnails are [`ThumbnailSlot`]s the Windows
//! host registers with `DwmRegisterThumbnail`. Open, close, and workspace
//! changes are nx-ui springs. The slide is a translation of the settled grid.

use nx_ui::geom::{Rect, Size};
use nx_ui::spring::{Spring, SpringConfig};
use nx_wm::{WindowId, WindowManagerModel, WorkspaceId};

use crate::util::{finite_f32, lerp};

#[derive(Clone, Debug, PartialEq)]
pub struct ExposeWindow {
    pub id: WindowId,
    pub title: String,
    pub order: u64,
    pub source: Size,
    pub from: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExposeTile {
    pub id: WindowId,
    pub title: String,
    pub order: u64,
    pub cell: Rect,
    pub content: Rect,
    pub label: Rect,
    pub from: Rect,
    pub source: Size,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposeParams {
    pub gap: f32,
    pub padding: f32,
    pub label_h: f32,
}

impl Default for ExposeParams {
    fn default() -> Self {
        Self {
            gap: 16.0,
            padding: 28.0,
            label_h: 22.0,
        }
    }
}

/// Column count for `n` tiles. Deterministic. Wider canvases get more columns.
pub fn grid_shape(count: usize, width: f32, height: f32) -> (usize, usize) {
    if count == 0 {
        return (0, 0);
    }
    let aspect = if height > 1.0 {
        (width / height).clamp(0.25, 8.0)
    } else {
        1.0
    };
    let raw = ((count as f32) * aspect).sqrt().ceil();
    let cols = if raw.is_finite() && raw > 0.0 {
        (raw as usize).clamp(1, count)
    } else {
        1
    };
    let rows = count.div_ceil(cols);
    (cols, rows)
}

fn fit_aspect(source: Size, box_rect: Rect) -> Rect {
    let sw = finite_f32(source.w, 1.0).max(1.0);
    let sh = finite_f32(source.h, 1.0).max(1.0);
    if box_rect.w <= 0.0 || box_rect.h <= 0.0 {
        return Rect::new(box_rect.x, box_rect.y, 0.0, 0.0);
    }
    let src_aspect = sw / sh;
    let box_aspect = box_rect.w / box_rect.h;
    let (w, h) = if src_aspect > box_aspect {
        (box_rect.w, box_rect.w / src_aspect)
    } else {
        (box_rect.h * src_aspect, box_rect.h)
    };
    Rect::new(
        box_rect.x + (box_rect.w - w) * 0.5,
        box_rect.y + (box_rect.h - h) * 0.5,
        w.max(0.0),
        h.max(0.0),
    )
}

/// Grid of `windows` inside `canvas`. Order is `(order, id)`, not input order.
/// Content rects preserve aspect and do not overlap.
pub fn layout_expose(
    windows: &[ExposeWindow],
    canvas: Rect,
    params: ExposeParams,
) -> Vec<ExposeTile> {
    if windows.is_empty() || canvas.w <= 0.0 || canvas.h <= 0.0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..windows.len()).collect();
    order.sort_by(|&a, &b| {
        windows[a]
            .order
            .cmp(&windows[b].order)
            .then(windows[a].id.0.cmp(&windows[b].id.0))
    });
    let count = order.len();
    let gap = params.gap.max(0.0);
    let padding = params.padding.max(0.0);
    let label_h = params.label_h.max(0.0);
    let inner = canvas.inset(nx_ui::geom::Insets::all(padding));
    if inner.w <= 0.0 || inner.h <= 0.0 {
        return Vec::new();
    }
    let (cols, rows) = grid_shape(count, inner.w, inner.h);
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let avail_w = (inner.w - gap * cols.saturating_sub(1) as f32).max(0.0);
    let avail_h = (inner.h - gap * rows.saturating_sub(1) as f32).max(0.0);
    let cell_w = avail_w / cols as f32;
    let cell_h = avail_h / rows as f32;
    let mut tiles = Vec::with_capacity(count);
    for (place, index) in order.into_iter().enumerate() {
        let window = &windows[index];
        let col = place % cols;
        let row = place / cols;
        let cell = Rect::new(
            inner.x + col as f32 * (cell_w + gap),
            inner.y + row as f32 * (cell_h + gap),
            cell_w,
            cell_h,
        );
        let content_box = Rect::new(cell.x, cell.y, cell.w, (cell.h - label_h).max(0.0));
        let content = fit_aspect(window.source, content_box);
        let label = Rect::new(
            cell.x,
            cell.bottom() - label_h.min(cell.h),
            cell.w,
            label_h.min(cell.h),
        );
        tiles.push(ExposeTile {
            id: window.id,
            title: window.title.clone(),
            order: window.order,
            cell,
            content,
            label,
            from: window.from,
            source: window.source,
        });
    }
    tiles
}

pub fn lerp_rect(from: Rect, to: Rect, t: f32) -> Rect {
    Rect::new(
        lerp(from.x, to.x, t),
        lerp(from.y, to.y, t),
        lerp(from.w, to.w, t),
        lerp(from.h, to.h, t),
    )
}

impl ExposeTile {
    /// Spring-interpolated thumbnail rect. `open` is 0 closed, 1 settled grid.
    pub fn displayed(&self, open: f32, slide_x: f32) -> Rect {
        let mut rect = lerp_rect(self.from, self.content, open);
        rect.x += slide_x;
        rect
    }
}

/// A live thumbnail the Windows host registers. `source_hwnd` 0 means the
/// slot is not yet bound to a window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbnailSlot {
    pub window: WindowId,
    pub source_hwnd: u64,
    pub dest: Rect,
    pub opacity: u8,
    pub client_only: bool,
}

pub fn thumbnail_slots(
    tiles: &[ExposeTile],
    hwnds: &[(WindowId, u64)],
    open: f32,
    slide_x: f32,
) -> Vec<ThumbnailSlot> {
    let opacity = (finite_f32(open, 0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    tiles
        .iter()
        .map(|tile| {
            let source_hwnd = hwnds
                .iter()
                .find(|(id, _)| *id == tile.id)
                .map(|(_, hwnd)| *hwnd)
                .unwrap_or(0);
            ThumbnailSlot {
                window: tile.id,
                source_hwnd,
                dest: tile.displayed(open, slide_x),
                opacity,
                client_only: true,
            }
        })
        .collect()
}

fn open_spring(position: f32) -> Spring {
    let mut spring = Spring::new(position, SpringConfig::macos(0.42, 1.0));
    spring.config.rest_pos = 0.01;
    spring.config.rest_vel = 0.05;
    spring
}

#[derive(Clone, Debug)]
pub struct ExposeModel {
    pub workspace: WorkspaceId,
    pub canvas: Rect,
    pub params: ExposeParams,
    windows: Vec<ExposeWindow>,
    tiles: Vec<ExposeTile>,
    open: Spring,
    slide: Spring,
    slide_width: f32,
    slide_dir: f32,
    shown: bool,
}

impl ExposeModel {
    pub fn new(canvas: Rect) -> Self {
        Self {
            workspace: WorkspaceId(1),
            canvas,
            params: ExposeParams::default(),
            windows: Vec::new(),
            tiles: Vec::new(),
            open: open_spring(0.0),
            slide: open_spring(1.0),
            slide_width: canvas.w.max(0.0),
            slide_dir: 1.0,
            shown: false,
        }
    }

    pub fn tiles(&self) -> &[ExposeTile] {
        &self.tiles
    }

    pub fn open_amount(&self) -> f32 {
        self.open.position
    }

    pub fn shown(&self) -> bool {
        self.shown || self.open.position > 0.02
    }

    pub fn open(&mut self) {
        self.shown = true;
        self.open.set_target(1.0);
    }

    pub fn close(&mut self) {
        self.shown = false;
        self.open.set_target(0.0);
    }

    /// Jump to the settled grid. Used by golden images and reduced-motion.
    pub fn show_settled(&mut self) {
        self.shown = true;
        self.open.position = 1.0;
        self.open.velocity = 0.0;
        self.open.set_target(1.0);
        self.slide.position = 1.0;
        self.slide.velocity = 0.0;
        self.slide.set_target(1.0);
    }

    pub fn slide_offset(&self) -> f32 {
        let t = self.slide.position.clamp(0.0, 1.0);
        (1.0 - t) * self.slide_dir * self.slide_width
    }

    pub fn step(&mut self, dt: f32) {
        self.open.step(dt);
        self.slide.step(dt);
    }

    pub fn animating(&self) -> bool {
        !self.open.settled() || !self.slide.settled()
    }

    pub fn set_canvas(&mut self, canvas: Rect) {
        self.canvas = canvas;
        self.slide_width = canvas.w.max(0.0);
        self.tiles = layout_expose(&self.windows, self.canvas, self.params);
    }

    /// Rebuild from nx-wm. A workspace change starts the slide. Repeating the
    /// same workspace does not restart it.
    pub fn follow_workspace(&mut self, wm: &WindowManagerModel, workspace: WorkspaceId) {
        if workspace != self.workspace {
            self.begin_slide(workspace);
            self.workspace = workspace;
        }
        self.rebuild(wm);
    }

    pub fn begin_slide(&mut self, to: WorkspaceId) {
        let from = self.workspace.0;
        let dest = to.0;
        self.slide_dir = if dest >= from { 1.0 } else { -1.0 };
        self.slide_width = self.canvas.w.max(0.0);
        self.slide.position = 0.0;
        self.slide.velocity = 0.0;
        self.slide.set_target(1.0);
    }

    pub fn rebuild(&mut self, wm: &WindowManagerModel) {
        self.windows.clear();
        if let Some(workspace) = wm.workspace(self.workspace) {
            for (order, id) in workspace.tree.iter().enumerate() {
                let Some(window) = wm.window(*id) else {
                    continue;
                };
                let source_rect = if window.target_rect.width > 0 && window.target_rect.height > 0 {
                    window.target_rect
                } else {
                    window.floating_rect
                };
                self.windows.push(ExposeWindow {
                    id: *id,
                    title: if window.title.is_empty() {
                        window.process_name.clone()
                    } else {
                        window.title.clone()
                    },
                    order: order as u64,
                    source: Size::new(
                        source_rect.width.max(1) as f32,
                        source_rect.height.max(1) as f32,
                    ),
                    from: Rect::new(
                        source_rect.x as f32,
                        source_rect.y as f32,
                        source_rect.width as f32,
                        source_rect.height as f32,
                    ),
                });
            }
        }
        self.tiles = layout_expose(&self.windows, self.canvas, self.params);
    }

    pub fn slots(&self, hwnds: &[(WindowId, u64)]) -> Vec<ThumbnailSlot> {
        thumbnail_slots(&self.tiles, hwnds, self.open.position, self.slide_offset())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nx_wm::{Rect as WmRect, WindowInfo, WindowManagerModel};

    fn window(id: u64, order: u64, w: f32, h: f32) -> ExposeWindow {
        ExposeWindow {
            id: WindowId(id),
            title: format!("Window {id}"),
            order,
            source: Size::new(w, h),
            from: Rect::new(id as f32 * 10.0, 40.0, w, h),
        }
    }

    fn canvas() -> Rect {
        Rect::new(0.0, 28.0, 1440.0, 760.0)
    }

    #[test]
    fn grid_has_no_overlaps_and_preserves_aspect() {
        let windows = vec![
            window(1, 0, 800.0, 600.0),
            window(2, 1, 400.0, 800.0),
            window(3, 2, 1600.0, 900.0),
            window(4, 3, 300.0, 300.0),
            window(5, 4, 1280.0, 720.0),
        ];
        let tiles = layout_expose(&windows, canvas(), ExposeParams::default());
        assert_eq!(tiles.len(), 5);
        for pair in tiles.windows(2) {
            assert!(
                !pair[0].content.intersects(pair[1].content),
                "{:?} overlaps {:?}",
                pair[0].content,
                pair[1].content
            );
            assert!(!pair[0].cell.intersects(pair[1].cell));
        }
        for tile in &tiles {
            assert!(tile.content.x >= canvas().x - 0.1);
            assert!(tile.content.y >= canvas().y - 0.1);
            assert!(tile.content.right() <= canvas().right() + 0.1);
            assert!(tile.content.bottom() <= canvas().bottom() + 0.1);
            let src = tile.source.w / tile.source.h;
            let got = tile.content.w / tile.content.h;
            assert!((src - got).abs() < 1.0e-3, "aspect {got} vs {src}");
            assert!(tile.content.w > 1.0 && tile.content.h > 1.0);
        }
    }

    #[test]
    fn ordering_is_stable_under_shuffle() {
        let mut windows = vec![
            window(3, 2, 100.0, 100.0),
            window(1, 0, 200.0, 100.0),
            window(2, 1, 100.0, 200.0),
        ];
        let first = layout_expose(&windows, canvas(), ExposeParams::default());
        windows.reverse();
        let second = layout_expose(&windows, canvas(), ExposeParams::default());
        let ids: Vec<_> = first.iter().map(|tile| tile.id).collect();
        let ids2: Vec<_> = second.iter().map(|tile| tile.id).collect();
        assert_eq!(ids, vec![WindowId(1), WindowId(2), WindowId(3)]);
        assert_eq!(ids, ids2);
        assert_eq!(first[0].content, second[0].content);
    }

    #[test]
    fn empty_and_single_window() {
        assert!(layout_expose(&[], canvas(), ExposeParams::default()).is_empty());
        let tiles = layout_expose(
            &[window(9, 0, 1600.0, 900.0)],
            canvas(),
            ExposeParams::default(),
        );
        assert_eq!(tiles.len(), 1);
        let src = 1600.0 / 900.0;
        let got = tiles[0].content.w / tiles[0].content.h;
        assert!((src - got).abs() < 1.0e-3);
    }

    #[test]
    fn displayed_rect_matches_the_grid_when_open() {
        let tiles = layout_expose(
            &[window(1, 0, 400.0, 300.0)],
            canvas(),
            ExposeParams::default(),
        );
        let shown = tiles[0].displayed(1.0, 0.0);
        assert!((shown.x - tiles[0].content.x).abs() < 1.0e-3);
        assert!((shown.w - tiles[0].content.w).abs() < 1.0e-3);
        let closed = tiles[0].displayed(0.0, 0.0);
        assert!((closed.x - tiles[0].from.x).abs() < 1.0e-3);
    }

    #[test]
    fn workspace_slide_starts_at_full_offset_and_settles() {
        let mut expose = ExposeModel::new(canvas());
        expose.show_settled();
        expose.begin_slide(WorkspaceId(3));
        assert!((expose.slide_offset() - canvas().w).abs() < 1.0);
        expose.begin_slide(WorkspaceId(1));
        // workspace field is still 1, begin_slide uses self.workspace which is 1,
        // and `to` is 1, direction is +1. Drive a downward switch explicitly.
        expose.workspace = WorkspaceId(4);
        expose.begin_slide(WorkspaceId(2));
        assert!(expose.slide_offset() < 0.0);
        assert!((expose.slide_offset().abs() - canvas().w).abs() < 1.0);
        for _ in 0..160 {
            expose.step(1.0 / 120.0);
        }
        assert!(!expose.animating());
        assert!(expose.slide_offset().abs() < 1.0);
    }

    #[test]
    fn sync_uses_workspace_tree_order_not_hash_order() {
        let mut wm =
            WindowManagerModel::new(WmRect::new(0, 0, 1440, 900), WmRect::new(0, 0, 1440, 860));
        for (id, w, h) in [(5u64, 800u32, 600u32), (2, 400, 800), (9, 200, 200)] {
            wm.on_window_show(WindowInfo {
                id: WindowId(id),
                title: format!("T{id}"),
                class_name: "Nx".to_string(),
                process_name: "app.exe".to_string(),
                rect: WmRect::new(0, 0, w, h),
                is_visible: true,
                is_minimized: false,
                is_cloaked: false,
                is_dialog_or_popup: false,
                style: 0,
                ex_style: 0,
            });
        }
        let mut expose = ExposeModel::new(canvas());
        expose.follow_workspace(&wm, WorkspaceId(1));
        let ids: Vec<_> = expose.tiles().iter().map(|tile| tile.id.0).collect();
        let tree: Vec<_> = wm
            .workspace(WorkspaceId(1))
            .expect("ws")
            .tree
            .iter()
            .map(|id| id.0)
            .collect();
        assert_eq!(ids, tree);
        expose.show_settled();
        let slots = expose.slots(&[(WindowId(5), 0x1000), (WindowId(2), 0x2000)]);
        assert_eq!(slots.len(), 3);
        assert_eq!(slots[0].source_hwnd, 0x1000);
        assert!(slots.iter().any(|slot| slot.source_hwnd == 0));
        assert!(slots.iter().all(|slot| slot.client_only));
        assert_eq!(slots[0].opacity, 255);
    }

    #[test]
    fn moving_a_window_to_another_workspace_removes_it_from_the_grid() {
        let mut wm =
            WindowManagerModel::new(WmRect::new(0, 0, 1440, 900), WmRect::new(0, 0, 1440, 860));
        wm.on_window_show(WindowInfo {
            id: WindowId(1),
            title: "One".to_string(),
            class_name: "Nx".to_string(),
            process_name: "a.exe".to_string(),
            rect: WmRect::new(0, 0, 500, 400),
            is_visible: true,
            is_minimized: false,
            is_cloaked: false,
            is_dialog_or_popup: false,
            style: 0,
            ex_style: 0,
        });
        wm.on_window_show(WindowInfo {
            id: WindowId(2),
            title: "Two".to_string(),
            class_name: "Nx".to_string(),
            process_name: "b.exe".to_string(),
            rect: WmRect::new(20, 20, 640, 480),
            is_visible: true,
            is_minimized: false,
            is_cloaked: false,
            is_dialog_or_popup: false,
            style: 0,
            ex_style: 0,
        });
        wm.move_window_to_workspace(WindowId(2), WorkspaceId(2));
        let mut expose = ExposeModel::new(canvas());
        expose.follow_workspace(&wm, WorkspaceId(1));
        assert_eq!(expose.tiles().len(), 1);
        assert_eq!(expose.tiles()[0].id, WindowId(1));
        expose.follow_workspace(&wm, WorkspaceId(2));
        assert_eq!(expose.tiles().len(), 1);
        assert_eq!(expose.tiles()[0].id, WindowId(2));
        assert!(expose.animating());
    }
}
