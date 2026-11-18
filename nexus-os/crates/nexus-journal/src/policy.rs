//! Priority policy that cannot brick the machine.
//!
//! Below-normal and idle are never applied. High and realtime are applied
//! only when the executable name is on an explicit whitelist. Everyone else
//! is clamped to at most above-normal, and only in the foreground role.

use crate::mutation::PriorityClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityRole {
    Foreground,
    Background,
}

/// Parse the config strings (`"High"`, `"BelowNormal"`, `"above_normal"`, ...).
/// Unknown text becomes [`PriorityClass::Normal`].
pub fn parse_priority(text: &str) -> PriorityClass {
    let flat: String = text
        .chars()
        .filter(|c| *c != '_' && *c != '-' && *c != ' ')
        .flat_map(|c| c.to_lowercase())
        .collect();
    match flat.as_str() {
        "idle" => PriorityClass::Idle,
        "belownormal" => PriorityClass::BelowNormal,
        "normal" => PriorityClass::Normal,
        "abovenormal" => PriorityClass::AboveNormal,
        "high" => PriorityClass::High,
        "realtime" => PriorityClass::Realtime,
        _ => PriorityClass::Normal,
    }
}

fn whitelisted(process_name: &str, whitelist: &[String]) -> bool {
    let name = process_name
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(process_name);
    whitelist
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(name))
}

/// Clamp a requested class to the never-brick policy.
pub fn clamp_priority(
    requested: PriorityClass,
    role: PriorityRole,
    process_name: &str,
    whitelist: &[String],
) -> PriorityClass {
    match role {
        PriorityRole::Background => match requested {
            PriorityClass::Idle | PriorityClass::BelowNormal => PriorityClass::Normal,
            PriorityClass::High | PriorityClass::Realtime => {
                if whitelisted(process_name, whitelist) {
                    requested
                } else {
                    PriorityClass::Normal
                }
            }
            other => other,
        },
        PriorityRole::Foreground => match requested {
            PriorityClass::Idle | PriorityClass::BelowNormal => PriorityClass::Normal,
            PriorityClass::High | PriorityClass::Realtime => {
                if whitelisted(process_name, whitelist) {
                    requested
                } else {
                    // One class above normal. HIGH stays opt-in.
                    PriorityClass::AboveNormal
                }
            }
            other => other,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_normal_is_never_applied() {
        for role in [PriorityRole::Foreground, PriorityRole::Background] {
            assert_eq!(
                clamp_priority(PriorityClass::BelowNormal, role, "chrome.exe", &[]),
                PriorityClass::Normal
            );
            assert_eq!(
                clamp_priority(PriorityClass::Idle, role, "chrome.exe", &[]),
                PriorityClass::Normal
            );
        }
    }

    #[test]
    fn high_requires_whitelist() {
        assert_eq!(
            clamp_priority(
                PriorityClass::High,
                PriorityRole::Foreground,
                "chrome.exe",
                &[]
            ),
            PriorityClass::AboveNormal
        );
        assert_eq!(
            clamp_priority(
                PriorityClass::Realtime,
                PriorityRole::Foreground,
                r"C:\Games\game.exe",
                &["game.exe".into()]
            ),
            PriorityClass::Realtime
        );
        assert_eq!(
            clamp_priority(
                PriorityClass::High,
                PriorityRole::Background,
                "chrome.exe",
                &[]
            ),
            PriorityClass::Normal
        );
    }

    #[test]
    fn parse_priority_accepts_config_spellings() {
        assert_eq!(parse_priority("High"), PriorityClass::High);
        assert_eq!(parse_priority("BelowNormal"), PriorityClass::BelowNormal);
        assert_eq!(parse_priority("above_normal"), PriorityClass::AboveNormal);
        assert_eq!(parse_priority("nope"), PriorityClass::Normal);
    }
}
