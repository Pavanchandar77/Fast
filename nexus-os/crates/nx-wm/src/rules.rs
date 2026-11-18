use crate::geometry::Rect;
use crate::types::WindowInfo;
use serde::{Deserialize, Serialize};

/// Matcher conditions for a window rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleMatcher {
    TitleEquals(String),
    TitleContains(String),
    ClassEquals(String),
    ClassContains(String),
    ProcessEquals(String),
    ProcessContains(String),
    IsDialog,
}

impl RuleMatcher {
    pub fn matches(&self, info: &WindowInfo) -> bool {
        match self {
            Self::TitleEquals(val) => info.title.eq_ignore_ascii_case(val),
            Self::TitleContains(val) => {
                info.title.to_ascii_lowercase().contains(&val.to_ascii_lowercase())
            }
            Self::ClassEquals(val) => info.class_name.eq_ignore_ascii_case(val),
            Self::ClassContains(val) => info
                .class_name
                .to_ascii_lowercase()
                .contains(&val.to_ascii_lowercase()),
            Self::ProcessEquals(val) => info.process_name.eq_ignore_ascii_case(val),
            Self::ProcessContains(val) => info
                .process_name
                .to_ascii_lowercase()
                .contains(&val.to_ascii_lowercase()),
            Self::IsDialog => info.is_dialog_or_popup,
        }
    }
}

/// Action applied when a window rule matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleAction {
    Float,
    Tile,
    AssignWorkspace(u8),
    AutoExclude,
    SetInitialSize { width: u32, height: u32 },
    PiP,
}

/// Declarative window management rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowRule {
    pub matchers: Vec<RuleMatcher>,
    pub actions: Vec<RuleAction>,
}

impl WindowRule {
    pub fn new(matchers: Vec<RuleMatcher>, actions: Vec<RuleAction>) -> Self {
        Self { matchers, actions }
    }

    pub fn matches(&self, info: &WindowInfo) -> bool {
        if self.matchers.is_empty() {
            return false;
        }
        // All matchers in a rule must match (conjunction)
        self.matchers.iter().all(|m| m.matches(info))
    }
}

/// Rule evaluation engine with sensible built-in defaults for Windows dialogs, PiP, and games.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RuleEngine {
    pub rules: Vec<WindowRule>,
}

impl RuleEngine {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn with_defaults() -> Self {
        let mut engine = Self::new();

        // 1. Standard dialogs and popups should float by default
        engine.add_rule(WindowRule::new(
            vec![RuleMatcher::IsDialog],
            vec![RuleAction::Float],
        ));

        // 2. Picture-in-Picture windows float and stay compact
        engine.add_rule(WindowRule::new(
            vec![RuleMatcher::TitleContains("Picture in picture".into())],
            vec![
                RuleAction::Float,
                RuleAction::PiP,
                RuleAction::SetInitialSize {
                    width: 480,
                    height: 270,
                },
            ],
        ));

        // 3. Common dialog classes (Open/Save file dialogs, Task Manager, Control Panel)
        let dialog_classes = ["#32770", "OperationStatusWindow", "TaskTaskManagerWindow"];
        for class in dialog_classes {
            engine.add_rule(WindowRule::new(
                vec![RuleMatcher::ClassEquals(class.into())],
                vec![RuleAction::Float],
            ));
        }

        // 4. Game launchers & overlay clients
        let excluded_processes = ["steamwebhelper.exe", "epicgameslauncher.exe", "riotclientservices.exe"];
        for proc in excluded_processes {
            engine.add_rule(WindowRule::new(
                vec![RuleMatcher::ProcessEquals(proc.into())],
                vec![RuleAction::Float],
            ));
        }

        engine
    }

    pub fn add_rule(&mut self, rule: WindowRule) {
        self.rules.push(rule);
    }

    /// Evaluates all rules against `info` and collects applied actions.
    pub fn evaluate(&self, info: &WindowInfo) -> Vec<RuleAction> {
        let mut actions = Vec::new();
        for rule in &self.rules {
            if rule.matches(info) {
                actions.extend(rule.actions.clone());
            }
        }
        actions
    }

    /// Automatically detects full-screen exclusive games or apps.
    /// If a window spans the full display bounds and is borderless or matches game processes,
    /// it is auto-excluded from tiling so it doesn't disrupt tiling or get warped.
    pub fn is_auto_excluded_game(info: &WindowInfo, monitor_bounds: Rect) -> bool {
        // Known games or exclusive fullscreen
        if !monitor_bounds.is_empty()
            && info.rect.x <= monitor_bounds.x
            && info.rect.y <= monitor_bounds.y
            && info.rect.width >= monitor_bounds.width
            && info.rect.height >= monitor_bounds.height
        {
            // If the window has no title bar / thick frame, it's borderless fullscreen
            let has_title_or_thick_frame = (info.style & 0x00C00000) != 0 || (info.style & 0x00040000) != 0;
            if !has_title_or_thick_frame {
                return true;
            }
        }

        // Known game engine / game processes
        let game_processes = ["game.exe", "cyberpunk2077.exe", "valheim.exe", "dota2.exe", "cs2.exe"];
        for gp in game_processes {
            if info.process_name.eq_ignore_ascii_case(gp) {
                return true;
            }
        }

        false
    }
}
