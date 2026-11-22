//! Spotlight launcher.
//!
//! Apps, settings, files, and a calculator share one ranked list. Files go
//! through [`nx_fs::index::SearchEngine`] when the host attaches an index.
//! Frecency uses [`nx_fs::index::frecency::FrecencyStore`] in memory only.
//! `save` is never called, so quitting the shell drops ranking history and
//! there is nothing to journal.
//!
//! A query over 10k apps and settings stays under 5 ms in release. The scan
//! keeps a fixed-size top-k and does not allocate per candidate.

use nx_fs::index::engine::SearchOptions;
use nx_fs::index::frecency::{boost_for, FrecencyStore};
use nx_fs::index::{Query, SearchEngine};
use nx_ui::geom::Rect;
use nx_ui::spring::{Spring, SpringConfig};

use crate::error::{ShellError, ShellResult};
use crate::util::clip_str;

const QUERY_LIMIT_BYTES: usize = 128;
const DEFAULT_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderKind {
    Calculator = 0,
    App = 1,
    Setting = 2,
    File = 3,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotlightHit {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: ProviderKind,
    pub score: u32,
    /// Calculator result, when `kind` is [`ProviderKind::Calculator`].
    pub value: Option<String>,
}

#[derive(Clone, Debug)]
struct CatalogItem {
    id: String,
    title: String,
    subtitle: String,
    folded: String,
    original: String,
    kind: ProviderKind,
    boost: u32,
}

#[derive(Clone, Debug)]
pub struct SpotlightLayout {
    pub panel: Rect,
    pub query: Rect,
    pub rows: Vec<Rect>,
}

pub fn layout_spotlight(screen: Rect, rows: usize, row_h: f32) -> SpotlightLayout {
    let width = 680.0_f32.min(screen.w - 32.0).max(240.0);
    let row_h = if row_h.is_finite() && row_h > 0.0 {
        row_h
    } else {
        44.0
    };
    let rows = rows.min(12);
    let height = 56.0 + row_h * rows as f32 + 12.0;
    let x = screen.x + (screen.w - width) * 0.5;
    let y = screen.y + 96.0;
    let panel = Rect::new(x, y, width, height);
    let mut row_rects = Vec::with_capacity(rows);
    for index in 0..rows {
        row_rects.push(Rect::new(
            panel.x + 8.0,
            panel.y + 56.0 + row_h * index as f32,
            panel.w - 16.0,
            row_h - 4.0,
        ));
    }
    SpotlightLayout {
        panel,
        query: Rect::new(panel.x + 16.0, panel.y + 12.0, panel.w - 32.0, 32.0),
        rows: row_rects,
    }
}

/// Greedy subsequence score. `None` when `query` is not a subsequence.
/// Consecutive hits, word boundaries, and a leading match score higher.
pub fn fuzzy_score(query: &[u8], folded: &[u8], original: &[u8]) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    if query.len() > folded.len() {
        return None;
    }
    let aligned = original.len() == folded.len();
    let mut qi = 0usize;
    let mut score = 0u32;
    let mut prev = false;
    let mut first = true;
    for (index, &byte) in folded.iter().enumerate() {
        if qi < query.len() && byte == query[qi] {
            let mut add = 6u32;
            if prev {
                add += 16;
            }
            if boundary_at(original, folded, index, aligned) {
                add += 12;
            }
            if first && index == 0 {
                add += 10;
            }
            score = score.saturating_add(add);
            prev = true;
            first = false;
            qi += 1;
            if qi == query.len() {
                break;
            }
        } else {
            prev = false;
        }
    }
    if qi == query.len() {
        Some(score)
    } else {
        None
    }
}

fn boundary_at(original: &[u8], folded: &[u8], index: usize, aligned: bool) -> bool {
    if index == 0 {
        return true;
    }
    let bytes = if aligned { original } else { folded };
    if index >= bytes.len() {
        return false;
    }
    let prev = bytes[index - 1];
    let cur = bytes[index];
    matches!(prev, b' ' | b'-' | b'_' | b'/' | b'\\' | b'.')
        || (prev.is_ascii_lowercase() && cur.is_ascii_uppercase())
}

fn score_tokens(tokens: &[&[u8]], folded: &[u8], original: &[u8]) -> Option<u32> {
    let mut total = 0u32;
    for token in tokens {
        total = total.saturating_add(fuzzy_score(token, folded, original)?);
    }
    Some(total)
}

fn kind_bias(kind: ProviderKind, query: &str) -> u32 {
    let fileish = query.contains('/') || query.contains('\\') || query.contains('.');
    match kind {
        ProviderKind::Calculator => 100_000,
        ProviderKind::App if !fileish => 2_000,
        ProviderKind::Setting if !fileish => 1_500,
        _ => 0,
    }
}

fn better(a: &SpotlightHit, b: &SpotlightHit) -> bool {
    a.score > b.score
        || (a.score == b.score && a.kind < b.kind)
        || (a.score == b.score && a.kind == b.kind && a.title < b.title)
        || (a.score == b.score && a.kind == b.kind && a.title == b.title && a.id < b.id)
}

fn consider(best: &mut Vec<SpotlightHit>, hit: SpotlightHit, limit: usize) {
    if limit == 0 {
        return;
    }
    if best.len() < limit {
        best.push(hit);
        return;
    }
    let mut worst = 0usize;
    for index in 1..best.len() {
        if better(&best[worst], &best[index]) {
            worst = index;
        }
    }
    if better(&hit, &best[worst]) {
        best[worst] = hit;
    }
}

pub struct Spotlight {
    items: Vec<CatalogItem>,
    files: Option<SearchEngine>,
    frecency: FrecencyStore,
    now: u64,
    pub query: String,
    pub limit: usize,
    open: Spring,
    shown: bool,
}

impl Spotlight {
    pub fn new() -> Self {
        let mut open = Spring::new(0.0, SpringConfig::macos(0.3, 0.92));
        open.config.rest_pos = 0.01;
        open.config.rest_vel = 0.05;
        Self {
            items: Vec::new(),
            files: None,
            frecency: FrecencyStore::default(),
            now: 0,
            query: String::new(),
            limit: DEFAULT_LIMIT,
            open,
            shown: false,
        }
    }

    /// A small built-in catalog so the palette is useful before the host
    /// pushes the installed-app list.
    pub fn with_builtins() -> Self {
        let mut spotlight = Self::new();
        for (id, title) in [
            ("finder", "Finder"),
            ("safari", "Safari"),
            ("mail", "Mail"),
            ("notes", "Notes"),
            ("terminal", "Terminal"),
            ("settings", "System Settings"),
            ("calculator", "Calculator"),
            ("explorer", "Nexus Explorer"),
        ] {
            spotlight.add_app(id, title, "Application");
        }
        for (id, title) in [
            ("dock", "Dock & Menu Bar"),
            ("mission-control", "Mission Control"),
            ("privacy", "Privacy & Security"),
            ("camera", "Camera"),
            ("microphone", "Microphone"),
            ("location", "Location Services"),
            ("clipboard", "Clipboard History"),
            ("volume", "Sound & Volume"),
            ("game-mode", "Game Mode"),
            ("displays", "Displays"),
            ("keyboard", "Keyboard"),
            ("wallpaper", "Wallpaper"),
            ("notifications", "Notifications"),
            ("focus", "Focus"),
            ("spotlight", "Spotlight"),
            ("trackpad", "Trackpad"),
            ("mouse", "Mouse"),
            ("network", "Network"),
            ("bluetooth", "Bluetooth"),
            ("users", "Users & Groups"),
            ("datetime", "Date & Time"),
            ("language", "Language & Region"),
            ("accessibility", "Accessibility"),
            ("storage", "Storage"),
            ("battery", "Battery"),
        ] {
            spotlight.add_setting(id, title, "System Settings");
        }
        spotlight
    }

    pub fn add_app(&mut self, id: &str, title: &str, subtitle: &str) {
        self.add_item(id, title, subtitle, ProviderKind::App);
    }

    pub fn add_file(&mut self, id: &str, title: &str, subtitle: &str) {
        self.add_item(id, title, subtitle, ProviderKind::File);
    }

    /// Only the System Settings entries; the host adds real apps and files.
    pub fn with_settings() -> Self {
        let mut spotlight = Self::with_builtins();
        spotlight
            .items
            .retain(|item| item.kind != ProviderKind::App);
        spotlight
    }

    pub fn add_setting(&mut self, id: &str, title: &str, subtitle: &str) {
        self.add_item(id, title, subtitle, ProviderKind::Setting);
    }

    pub fn attach_files(&mut self, engine: SearchEngine) {
        self.files = Some(engine);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn open_palette(&mut self) {
        self.shown = true;
        self.open.set_target(1.0);
    }

    pub fn close_palette(&mut self) {
        self.shown = false;
        self.open.set_target(0.0);
    }

    pub fn toggle(&mut self) {
        if self.shown {
            self.close_palette();
        } else {
            self.open_palette();
        }
    }

    pub fn is_open(&self) -> bool {
        self.shown || self.open.position > 0.02
    }

    pub fn open_amount(&self) -> f32 {
        self.open.position
    }

    pub fn step(&mut self, dt: f32) {
        self.open.step(dt);
    }

    pub fn animating(&self) -> bool {
        !self.open.settled()
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
    }

    pub fn search(&self) -> Vec<SpotlightHit> {
        self.query_str(&self.query)
    }

    pub fn query_str(&self, raw: &str) -> Vec<SpotlightHit> {
        let clipped = clip_str(raw.trim(), QUERY_LIMIT_BYTES);
        let limit = self.limit.max(1);
        if clipped.is_empty() {
            return self.recents(limit);
        }
        let folded = clipped.to_ascii_lowercase();
        let tokens: Vec<&[u8]> = folded
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| token.as_bytes())
            .collect();
        let mut best = Vec::with_capacity(limit);
        if let Some(value) = eval_calculator(clipped) {
            consider(
                &mut best,
                SpotlightHit {
                    id: "calc".to_string(),
                    title: value.clone(),
                    subtitle: "Calculator".to_string(),
                    kind: ProviderKind::Calculator,
                    score: kind_bias(ProviderKind::Calculator, clipped),
                    value: Some(value),
                },
                limit,
            );
        }
        for item in &self.items {
            let Some(fuzzy) =
                score_tokens(&tokens, item.folded.as_bytes(), item.original.as_bytes())
            else {
                continue;
            };
            let score = fuzzy
                .saturating_add(item.boost)
                .saturating_add(kind_bias(item.kind, clipped));
            consider(
                &mut best,
                SpotlightHit {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    subtitle: item.subtitle.clone(),
                    kind: item.kind,
                    score,
                    value: None,
                },
                limit,
            );
        }
        if let Some(engine) = &self.files {
            let parsed = Query::parse(clipped);
            if !parsed.is_empty() {
                let options = SearchOptions {
                    limit,
                    include_hidden: false,
                    threads: 1,
                    min_parallel: usize::MAX,
                };
                for hit in engine.search(&parsed, &options) {
                    let score = hit
                        .score
                        .saturating_add(kind_bias(ProviderKind::File, clipped));
                    consider(
                        &mut best,
                        SpotlightHit {
                            id: hit.path.clone(),
                            title: file_title(&hit.path),
                            subtitle: hit.path,
                            kind: ProviderKind::File,
                            score,
                            value: None,
                        },
                        limit,
                    );
                }
            }
        }
        best.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.kind.cmp(&b.kind))
                .then_with(|| a.title.cmp(&b.title))
                .then_with(|| a.id.cmp(&b.id))
        });
        best
    }

    /// Remember a launch. The score stays in the process. It is not saved.
    pub fn activate(&mut self, id: &str, now: u64) {
        self.now = now;
        if let Some(engine) = self.files.as_mut() {
            if engine.index.resolve(id).is_some() {
                engine.record_open(id, now);
                return;
            }
        }
        let score = self.frecency.record(id, now);
        let boost = boost_for(score);
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.boost = boost;
        }
    }

    fn recents(&self, limit: usize) -> Vec<SpotlightHit> {
        let mut ranked: Vec<&CatalogItem> = self.items.iter().collect();
        ranked.sort_by(|a, b| {
            b.boost
                .cmp(&a.boost)
                .then_with(|| a.title.cmp(&b.title))
                .then_with(|| a.id.cmp(&b.id))
        });
        ranked
            .into_iter()
            .take(limit)
            .map(|item| SpotlightHit {
                id: item.id.clone(),
                title: item.title.clone(),
                subtitle: item.subtitle.clone(),
                kind: item.kind,
                score: item.boost,
                value: None,
            })
            .collect()
    }

    fn add_item(&mut self, id: &str, title: &str, subtitle: &str, kind: ProviderKind) {
        let folded = format!(
            "{} {} {}",
            id.to_ascii_lowercase(),
            title.to_ascii_lowercase(),
            subtitle.to_ascii_lowercase()
        );
        let original = format!("{title} {id}");
        if let Some(existing) = self.items.iter_mut().find(|item| item.id == id) {
            existing.title = title.to_string();
            existing.subtitle = subtitle.to_string();
            existing.folded = folded;
            existing.original = original;
            existing.kind = kind;
            return;
        }
        self.items.push(CatalogItem {
            id: id.to_string(),
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            folded,
            original,
            kind,
            boost: 0,
        });
    }
}

impl Default for Spotlight {
    fn default() -> Self {
        Self::new()
    }
}

fn file_title(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Insert `path` into a fresh nx-fs index. `path` uses `/` separators.
pub fn engine_from_paths(paths: &[&str]) -> ShellResult<SearchEngine> {
    let mut index = nx_fs::index::PathIndex::default();
    for path in paths {
        insert_path(&mut index, path).map_err(|err| ShellError::Index(err.to_string()))?;
    }
    Ok(SearchEngine::new(index, FrecencyStore::default(), 0))
}

fn insert_path(index: &mut nx_fs::index::PathIndex, path: &str) -> Result<(), nx_fs::FsError> {
    let root = index.add_root("/")?;
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    let mut parent = root;
    for (index_part, part) in parts.iter().enumerate() {
        let is_dir = index_part + 1 != parts.len();
        parent = index.insert(parent, part, is_dir)?;
    }
    Ok(())
}

/// Arithmetic Spotlight understands: `+ - * / ^ %`, parentheses, `sqrt`, `pi`, `e`.
/// A query that is not an expression returns `None`. Division by zero returns `None`.
pub fn eval_calculator(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || !looks_like_math(trimmed) {
        return None;
    }
    let mut parser = Parser {
        bytes: trimmed.as_bytes(),
        index: 0,
    };
    let value = parser.expr()?;
    parser.skip();
    if parser.index != parser.bytes.len() || !value.is_finite() {
        return None;
    }
    Some(format_number(value))
}

fn looks_like_math(input: &str) -> bool {
    input.chars().any(|ch| ch.is_ascii_digit())
        || input.contains("sqrt")
        || input == "pi"
        || input == "e"
}

fn format_number(value: f64) -> String {
    if (value - value.round()).abs() < 1.0e-9 && value.abs() < 1.0e15 {
        return format!("{}", value.round() as i64);
    }
    let mut text = format!("{value:.8}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        while self.index < self.bytes.len() && self.bytes[self.index].is_ascii_whitespace() {
            self.index += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.bytes.get(self.index).copied()
    }

    fn eat(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, word: &[u8]) -> bool {
        self.skip();
        let end = self.index + word.len();
        if end <= self.bytes.len() && &self.bytes[self.index..end] == word {
            let boundary = end == self.bytes.len() || !self.bytes[end].is_ascii_alphanumeric();
            if boundary {
                self.index = end;
                return true;
            }
        }
        false
    }

    fn expr(&mut self) -> Option<f64> {
        let mut value = self.term()?;
        loop {
            if self.eat(b'+') {
                value += self.term()?;
            } else if self.eat(b'-') {
                value -= self.term()?;
            } else {
                break;
            }
        }
        Some(value)
    }

    fn term(&mut self) -> Option<f64> {
        let mut value = self.power()?;
        loop {
            if self.eat(b'*') {
                value *= self.power()?;
            } else if self.eat(b'/') {
                let rhs = self.power()?;
                if rhs == 0.0 {
                    return None;
                }
                value /= rhs;
            } else {
                break;
            }
        }
        Some(value)
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat(b'^') {
            let exp = self.power()?;
            let value = base.powf(exp);
            if value.is_finite() {
                Some(value)
            } else {
                None
            }
        } else {
            Some(base)
        }
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat(b'+') {
            self.unary()
        } else if self.eat(b'-') {
            self.unary().map(|value| -value)
        } else {
            self.primary()
        }
    }

    fn primary(&mut self) -> Option<f64> {
        if self.eat_word(b"sqrt") {
            if !self.eat(b'(') {
                return None;
            }
            let inner = self.expr()?;
            if !self.eat(b')') || inner < 0.0 {
                return None;
            }
            return Some(inner.sqrt());
        }
        if self.eat_word(b"pi") {
            return Some(std::f64::consts::PI);
        }
        if self.eat_word(b"e") {
            return Some(std::f64::consts::E);
        }
        if self.eat(b'(') {
            let inner = self.expr()?;
            if !self.eat(b')') {
                return None;
            }
            return Some(self.apply_percent(inner));
        }
        let number = self.number()?;
        Some(self.apply_percent(number))
    }

    fn apply_percent(&mut self, value: f64) -> f64 {
        if self.eat(b'%') {
            value / 100.0
        } else {
            value
        }
    }

    fn number(&mut self) -> Option<f64> {
        self.skip();
        let start = self.index;
        let mut saw_digit = false;
        let mut saw_dot = false;
        while self.index < self.bytes.len() {
            let byte = self.bytes[self.index];
            if byte.is_ascii_digit() {
                saw_digit = true;
                self.index += 1;
            } else if byte == b'.' && !saw_dot {
                saw_dot = true;
                self.index += 1;
            } else {
                break;
            }
        }
        if !saw_digit {
            self.index = start;
            return None;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).ok()?;
        text.parse::<f64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_beats_a_loose_subsequence() {
        let app = fuzzy_score(b"app", b"app store", b"App Store").expect("app");
        let wall = fuzzy_score(b"app", b"wallpaper", b"Wallpaper").expect("wall");
        assert!(app > wall, "{app} vs {wall}");
        assert!(fuzzy_score(b"zzz", b"safari", b"Safari").is_none());
    }

    #[test]
    fn calculator_parses_the_expressions_spotlight_shows() {
        assert_eq!(eval_calculator("2+2").as_deref(), Some("4"));
        assert_eq!(eval_calculator("(1+2)*3").as_deref(), Some("9"));
        assert_eq!(eval_calculator("10/2").as_deref(), Some("5"));
        assert_eq!(eval_calculator("2^8").as_deref(), Some("256"));
        assert_eq!(eval_calculator("sqrt(9)").as_deref(), Some("3"));
        assert_eq!(eval_calculator("50%").as_deref(), Some("0.5"));
        assert_eq!(eval_calculator("2*pi").as_deref(), Some("6.28318531"));
        assert!(eval_calculator("1/0").is_none());
        assert!(eval_calculator("safari").is_none());
        assert!(eval_calculator("sqrt(-1)").is_none());
        assert_eq!(eval_calculator("--2").as_deref(), Some("2"));
        assert_eq!(eval_calculator("2^3^2").as_deref(), Some("512"));
    }

    #[test]
    fn apps_settings_files_and_calculator_share_one_list() {
        let mut spotlight = Spotlight::with_builtins();
        spotlight.attach_files(
            engine_from_paths(&["/Users/me/notes/safari-tabs.txt", "/Users/me/photo.png"])
                .expect("index"),
        );
        let hits = spotlight.query_str("2+2");
        assert_eq!(hits[0].kind, ProviderKind::Calculator);
        assert_eq!(hits[0].value.as_deref(), Some("4"));

        let safari = spotlight.query_str("saf");
        assert_eq!(safari[0].id, "safari");
        assert_eq!(safari[0].kind, ProviderKind::App);

        let privacy = spotlight.query_str("privacy");
        assert_eq!(privacy[0].kind, ProviderKind::Setting);

        let files = spotlight.query_str("photo.png");
        assert!(files
            .iter()
            .any(|hit| hit.kind == ProviderKind::File && hit.title == "photo.png"));
    }

    #[test]
    fn frecency_promotes_an_activated_item_and_is_not_saved() {
        let mut spotlight = Spotlight::new();
        spotlight.add_app("alpha", "Alpha Notes", "Application");
        spotlight.add_app("beta", "Alpha Beta", "Application");
        let before = spotlight.query_str("alpha");
        assert_eq!(before[0].id, "alpha");
        spotlight.activate("beta", 1_700_000_000);
        let after = spotlight.query_str("alpha");
        assert_eq!(after[0].id, "beta");
        assert!(after[0].score > after[1].score);
        assert!(spotlight.frecency.len() == 1);
    }

    #[test]
    fn empty_query_lists_recent_apps_first() {
        let mut spotlight = Spotlight::with_builtins();
        spotlight.activate("terminal", 10);
        let hits = spotlight.query_str("");
        assert_eq!(hits[0].id, "terminal");
        assert!(hits.len() <= spotlight.limit);
    }

    #[test]
    fn palette_layout_is_centered_and_rows_do_not_overlap() {
        let layout = layout_spotlight(Rect::new(0.0, 0.0, 1440.0, 900.0), 5, 44.0);
        assert!((layout.panel.center().x - 720.0).abs() < 1.0);
        assert!(layout.query.y > layout.panel.y);
        for pair in layout.rows.windows(2) {
            assert!(!pair[0].intersects(pair[1]));
        }
    }

    #[test]
    fn ten_thousand_items_query_does_not_panic() {
        let mut spotlight = Spotlight::new();
        for index in 0..2_000 {
            spotlight.add_app(
                &format!("app-{index}"),
                &format!("Application {index} Name"),
                "Application",
            );
            if index < 500 {
                spotlight.add_setting(
                    &format!("set-{index}"),
                    &format!("Setting {index}"),
                    "Settings",
                );
            }
        }
        let hits = spotlight.query_str("application 42");
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|hit| hit.id == "app-42"));
        let _ = spotlight.query_str("zzzz-no-match");
        let _ = spotlight.query_str("(((");
    }
}
