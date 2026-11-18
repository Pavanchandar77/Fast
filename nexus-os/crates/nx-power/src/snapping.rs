use crate::error::{PowerError, PowerResult};
use crate::types::{PlanStatus, Rect, RectF, WindowId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Layout parameters for computing pixel geometry when snapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapGeometry {
    pub work_area: Rect,
    pub margin: u32,
    pub gap: u32,
}

impl SnapGeometry {
    pub const fn new(work_area: Rect, margin: u32, gap: u32) -> Self {
        Self {
            work_area,
            margin,
            gap,
        }
    }
}

/// A single snap zone inside a named zone set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapZone {
    pub id: u32,
    pub name: String,
    pub normalized_rect: RectF,
}

impl SnapZone {
    pub fn new(id: u32, name: impl Into<String>, rect: RectF) -> Self {
        Self {
            id,
            name: name.into(),
            normalized_rect: rect,
        }
    }

    /// Computes the absolute pixel bounds for this zone within a work area, applying margin and gap.
    pub fn calculate_rect(&self, work_area: &Rect, margin: u32, gap: u32) -> Rect {
        let usable = work_area.shrink(margin);
        self.normalized_rect.to_pixel_rect(&usable, gap)
    }
}

/// A collection of zones forming a named layout preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneSet {
    pub id: String,
    pub name: String,
    pub zones: Vec<SnapZone>,
}

impl ZoneSet {
    pub fn new(id: impl Into<String>, name: impl Into<String>, zones: Vec<SnapZone>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            zones,
        }
    }

    /// Preset: Side-by-side 50/50 split.
    pub fn split_50_50() -> Self {
        Self::new(
            "split-50-50",
            "Split 50/50",
            vec![
                SnapZone::new(1, "Left Half", RectF::new(0.0, 0.0, 0.5, 1.0)),
                SnapZone::new(2, "Right Half", RectF::new(0.5, 0.0, 0.5, 1.0)),
            ],
        )
    }

    /// Preset: 3 equal columns for wide monitors.
    pub fn three_column() -> Self {
        Self::new(
            "three-column",
            "3-Column Ultrawide",
            vec![
                SnapZone::new(1, "Left Third", RectF::new(0.0, 0.0, 0.3333, 1.0)),
                SnapZone::new(2, "Center Third", RectF::new(0.3333, 0.0, 0.3334, 1.0)),
                SnapZone::new(3, "Right Third", RectF::new(0.6667, 0.0, 0.3333, 1.0)),
            ],
        )
    }

    /// Preset: 2x2 Quadrant Grid.
    pub fn grid_2x2() -> Self {
        Self::new(
            "grid-2x2",
            "2x2 Grid",
            vec![
                SnapZone::new(1, "Top Left", RectF::new(0.0, 0.0, 0.5, 0.5)),
                SnapZone::new(2, "Top Right", RectF::new(0.5, 0.0, 0.5, 0.5)),
                SnapZone::new(3, "Bottom Left", RectF::new(0.0, 0.5, 0.5, 0.5)),
                SnapZone::new(4, "Bottom Right", RectF::new(0.5, 0.5, 0.5, 0.5)),
            ],
        )
    }

    /// Preset: Main master area on left (60%), two stacked windows on right (40%).
    pub fn priority_grid() -> Self {
        Self::new(
            "priority-grid",
            "Priority Grid",
            vec![
                SnapZone::new(1, "Primary", RectF::new(0.0, 0.0, 0.6, 1.0)),
                SnapZone::new(2, "Top Stack", RectF::new(0.6, 0.0, 0.4, 0.5)),
                SnapZone::new(3, "Bottom Stack", RectF::new(0.6, 0.5, 0.4, 0.5)),
            ],
        )
    }

    /// Preset: Center focus pane (60%) with left and right sidecars (20% each).
    pub fn focus_sidecars() -> Self {
        Self::new(
            "focus-sidecars",
            "Focus + Sidecars",
            vec![
                SnapZone::new(1, "Left Sidecar", RectF::new(0.0, 0.0, 0.2, 1.0)),
                SnapZone::new(2, "Center Focus", RectF::new(0.2, 0.0, 0.6, 1.0)),
                SnapZone::new(3, "Right Sidecar", RectF::new(0.8, 0.0, 0.2, 1.0)),
            ],
        )
    }
}

/// A snap group associating multiple windows with their respective zones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapGroup {
    pub id: u64,
    pub name: String,
    pub zone_set_id: String,
    pub members: HashMap<WindowId, u32>, // WindowId -> ZoneId
}

/// Dry-run report for a window snap action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapDryRun {
    pub window_id: WindowId,
    pub zone_set_id: String,
    pub zone_id: u32,
    pub target_rect: Rect,
    pub previous_rect: Option<Rect>,
}

/// Reversible mutation plan for snapping a window into a layout zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapPlan {
    pub window_id: WindowId,
    pub zone_set_id: String,
    pub zone_id: u32,
    pub target_rect: Rect,
    pub original_bounds: Option<Rect>,
    pub status: PlanStatus,
}

impl SnapPlan {
    pub fn dry_run(&self) -> SnapDryRun {
        SnapDryRun {
            window_id: self.window_id,
            zone_set_id: self.zone_set_id.clone(),
            zone_id: self.zone_id,
            target_rect: self.target_rect,
            previous_rect: self.original_bounds,
        }
    }

    /// Applies the snap to the store and records the previous bounds for rollback.
    pub fn apply(&mut self, store: &mut SnapStore) -> PowerResult<UnsnapPlan> {
        let revert = UnsnapPlan {
            window_id: self.window_id,
            restore_bounds: self.original_bounds,
        };

        if let Some(orig) = self.original_bounds {
            store.original_bounds.entry(self.window_id).or_insert(orig);
        }
        store
            .window_assignments
            .insert(self.window_id, (self.zone_set_id.clone(), self.zone_id));

        self.status = PlanStatus::Applied;
        Ok(revert)
    }
}

/// Reversible undo plan that restores the un-snapped original bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsnapPlan {
    pub window_id: WindowId,
    pub restore_bounds: Option<Rect>,
}

impl UnsnapPlan {
    pub fn revert(&self, store: &mut SnapStore) -> PowerResult<Option<Rect>> {
        store.window_assignments.remove(&self.window_id);
        let restored = store.original_bounds.remove(&self.window_id);
        Ok(self.restore_bounds.or(restored))
    }
}

/// Store holding layout presets, active window zone assignments, and snap groups.
#[derive(Debug, Clone)]
pub struct SnapStore {
    zone_sets: HashMap<String, ZoneSet>,
    active_zone_set_id: String,
    window_assignments: HashMap<WindowId, (String, u32)>,
    original_bounds: HashMap<WindowId, Rect>,
    snap_groups: HashMap<u64, SnapGroup>,
    next_group_id: u64,
}

impl SnapStore {
    pub fn new() -> Self {
        let mut store = Self {
            zone_sets: HashMap::new(),
            active_zone_set_id: "split-50-50".to_string(),
            window_assignments: HashMap::new(),
            original_bounds: HashMap::new(),
            snap_groups: HashMap::new(),
            next_group_id: 1,
        };

        // Register default presets
        store.register_zone_set(ZoneSet::split_50_50());
        store.register_zone_set(ZoneSet::three_column());
        store.register_zone_set(ZoneSet::grid_2x2());
        store.register_zone_set(ZoneSet::priority_grid());
        store.register_zone_set(ZoneSet::focus_sidecars());

        store
    }

    pub fn register_zone_set(&mut self, zone_set: ZoneSet) {
        self.zone_sets.insert(zone_set.id.clone(), zone_set);
    }

    pub fn set_active_zone_set(&mut self, id: &str) -> PowerResult<()> {
        if self.zone_sets.contains_key(id) {
            self.active_zone_set_id = id.to_string();
            Ok(())
        } else {
            Err(PowerError::SnapError(format!(
                "Zone set '{}' not found",
                id
            )))
        }
    }

    pub fn active_zone_set(&self) -> &ZoneSet {
        if let Some(zs) = self.zone_sets.get(&self.active_zone_set_id) {
            zs
        } else if let Some(zs) = self.zone_sets.values().next() {
            zs
        } else {
            // Static fallback ensuring zero panics or unwraps
            static FALLBACK: std::sync::OnceLock<ZoneSet> = std::sync::OnceLock::new();
            FALLBACK.get_or_init(ZoneSet::split_50_50)
        }
    }

    pub fn get_zone_set(&self, id: &str) -> Option<&ZoneSet> {
        self.zone_sets.get(id)
    }

    /// Finds the nearest snap zone in the active zone set for a given cursor position (px, py).
    /// Respects `snap_threshold_px` distance.
    pub fn find_nearest_zone(
        &self,
        px: i32,
        py: i32,
        work_area: &Rect,
        margin: u32,
        gap: u32,
        snap_threshold_px: u32,
    ) -> Option<&SnapZone> {
        let active = self.active_zone_set();
        let mut best: Option<(&SnapZone, u64)> = None;
        let threshold_sq = (snap_threshold_px as u64) * (snap_threshold_px as u64);

        for zone in &active.zones {
            let rect = zone.calculate_rect(work_area, margin, gap);
            if rect.contains(px, py) {
                return Some(zone);
            }
            let dist_sq = rect.distance_squared_to_center(px, py);
            if dist_sq <= threshold_sq {
                match best {
                    None => best = Some((zone, dist_sq)),
                    Some((_, current_min)) if dist_sq < current_min => {
                        best = Some((zone, dist_sq));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(zone, _)| zone)
    }

    /// Creates a snap plan for a window into a specific zone.
    pub fn create_snap_plan(
        &self,
        window_id: WindowId,
        zone_set_id: &str,
        zone_id: u32,
        current_bounds: Option<Rect>,
        geo: &SnapGeometry,
    ) -> PowerResult<SnapPlan> {
        let zone_set = self.get_zone_set(zone_set_id).ok_or_else(|| {
            PowerError::SnapError(format!("Zone set '{}' not found", zone_set_id))
        })?;

        let zone = zone_set
            .zones
            .iter()
            .find(|z| z.id == zone_id)
            .ok_or_else(|| PowerError::ZoneNotFound(zone_id, zone_set_id.to_string()))?;

        let target_rect = zone.calculate_rect(&geo.work_area, geo.margin, geo.gap);
        let orig = current_bounds.or_else(|| self.original_bounds.get(&window_id).copied());

        Ok(SnapPlan {
            window_id,
            zone_set_id: zone_set_id.to_string(),
            zone_id,
            target_rect,
            original_bounds: orig,
            status: PlanStatus::DryRun,
        })
    }

    /// Creates a multi-window snap group.
    pub fn create_snap_group(
        &mut self,
        name: impl Into<String>,
        zone_set_id: impl Into<String>,
        members: HashMap<WindowId, u32>,
    ) -> u64 {
        let id = self.next_group_id;
        self.next_group_id = self.next_group_id.saturating_add(1);

        let group = SnapGroup {
            id,
            name: name.into(),
            zone_set_id: zone_set_id.into(),
            members,
        };
        self.snap_groups.insert(id, group);
        id
    }

    pub fn get_snap_group(&self, id: u64) -> Option<&SnapGroup> {
        self.snap_groups.get(&id)
    }

    pub fn list_snap_groups(&self) -> Vec<&SnapGroup> {
        self.snap_groups.values().collect()
    }
}

impl Default for SnapStore {
    fn default() -> Self {
        Self::new()
    }
}
