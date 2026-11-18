use crate::error::{PowerError, PowerResult};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Casing transformations for batch renaming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaseType {
    Lowercase,
    Uppercase,
    TitleCase,
    CamelCase,
    SnakeCase,
    KebabCase,
}

impl CaseType {
    pub fn apply(&self, input: &str) -> String {
        match self {
            Self::Lowercase => input.to_lowercase(),
            Self::Uppercase => input.to_uppercase(),
            Self::TitleCase => {
                let mut result = String::with_capacity(input.len());
                let mut capitalize_next = true;
                for c in input.chars() {
                    if c.is_alphanumeric() {
                        if capitalize_next {
                            result.extend(c.to_uppercase());
                            capitalize_next = false;
                        } else {
                            result.extend(c.to_lowercase());
                        }
                    } else {
                        capitalize_next = true;
                        result.push(c);
                    }
                }
                result
            }
            Self::CamelCase => {
                let words = split_words(input);
                let mut result = String::with_capacity(input.len());
                for (i, word) in words.iter().enumerate() {
                    if i == 0 {
                        result.push_str(&word.to_lowercase());
                    } else {
                        let mut chars = word.chars();
                        if let Some(first) = chars.next() {
                            result.extend(first.to_uppercase());
                            result.push_str(&chars.as_str().to_lowercase());
                        }
                    }
                }
                if result.is_empty() {
                    input.to_string()
                } else {
                    result
                }
            }
            Self::SnakeCase => {
                let words = split_words(input);
                if words.is_empty() {
                    input.to_lowercase()
                } else {
                    words
                        .iter()
                        .map(|w| w.to_lowercase())
                        .collect::<Vec<_>>()
                        .join("_")
                }
            }
            Self::KebabCase => {
                let words = split_words(input);
                if words.is_empty() {
                    input.to_lowercase()
                } else {
                    words
                        .iter()
                        .map(|w| w.to_lowercase())
                        .collect::<Vec<_>>()
                        .join("-")
                }
            }
        }
    }
}

fn split_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut prev_is_upper = false;

    for c in s.chars() {
        if c == '_' || c == '-' || c == ' ' || c == '.' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            prev_is_upper = false;
        } else if c.is_uppercase() {
            if !prev_is_upper && !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            current.push(c);
            prev_is_upper = true;
        } else {
            current.push(c);
            prev_is_upper = false;
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// A transformation rule applied to file stems or extensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RenameRule {
    /// Regular expression replace with `$1`, `$2` capture substitution.
    RegexReplace {
        pattern: String,
        replacement: String,
    },
    /// Sequential numbering format: `{prefix}{index:0padding}{suffix}`
    SequentialNumbering {
        prefix: String,
        suffix: String,
        start: usize,
        step: usize,
        padding: usize,
    },
    /// Prepend and/or append strings to file stem.
    PrefixSuffix { prefix: String, suffix: String },
    /// Substring find and replace.
    FindReplace {
        find: String,
        replace: String,
        case_sensitive: bool,
    },
    /// Casing change applied to the file stem.
    ChangeCase { case_type: CaseType },
    /// Change the file extension (without dot).
    ChangeExtension { new_extension: String },
}

fn normalize_replacement(rep: &str) -> String {
    let mut out = String::with_capacity(rep.len() + 8);
    let chars: Vec<char> = rep.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            out.push('$');
            out.push('{');
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                out.push(chars[i]);
                i += 1;
            }
            out.push('}');
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

impl RenameRule {
    /// Applies the rule to `(stem, ext, index)`. Returns updated `(stem, ext)`.
    pub fn apply(
        &self,
        stem: &str,
        ext: Option<&str>,
        index: usize,
    ) -> PowerResult<(String, Option<String>)> {
        match self {
            Self::RegexReplace {
                pattern,
                replacement,
            } => {
                let re = Regex::new(pattern)
                    .map_err(|e| PowerError::RenameError(format!("Invalid regex: {}", e)))?;
                let normalized = normalize_replacement(replacement);
                let new_stem = re.replace_all(stem, normalized.as_str()).to_string();
                Ok((new_stem, ext.map(|s| s.to_string())))
            }
            Self::SequentialNumbering {
                prefix,
                suffix,
                start,
                step,
                padding,
            } => {
                let num = start.saturating_add(index.saturating_mul(*step));
                let formatted_num = format!("{:0width$}", num, width = *padding);
                let new_stem = format!("{}{}{}", prefix, formatted_num, suffix);
                Ok((new_stem, ext.map(|s| s.to_string())))
            }
            Self::PrefixSuffix { prefix, suffix } => {
                let new_stem = format!("{}{}{}", prefix, stem, suffix);
                Ok((new_stem, ext.map(|s| s.to_string())))
            }
            Self::FindReplace {
                find,
                replace,
                case_sensitive,
            } => {
                if find.is_empty() {
                    return Ok((stem.to_string(), ext.map(|s| s.to_string())));
                }
                let new_stem = if *case_sensitive {
                    stem.replace(find, replace)
                } else {
                    let re_str = format!("(?i){}", regex::escape(find));
                    let re = Regex::new(&re_str).map_err(|e| {
                        PowerError::RenameError(format!("Failed to build find regex: {}", e))
                    })?;
                    re.replace_all(stem, replace.as_str()).to_string()
                };
                Ok((new_stem, ext.map(|s| s.to_string())))
            }
            Self::ChangeCase { case_type } => {
                let new_stem = case_type.apply(stem);
                Ok((new_stem, ext.map(|s| s.to_string())))
            }
            Self::ChangeExtension { new_extension } => {
                let clean_ext = new_extension.trim_start_matches('.');
                let ext_out = if clean_ext.is_empty() {
                    None
                } else {
                    Some(clean_ext.to_string())
                };
                Ok((stem.to_string(), ext_out))
            }
        }
    }
}

/// Potential conflict discovered during batch rename planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenameConflict {
    /// Multiple files would be renamed to the same destination path.
    CollisionWithBatch {
        target: PathBuf,
        sources: Vec<PathBuf>,
    },
    /// The target name contains characters forbidden by Windows (`< > : " / \ | ? *`).
    InvalidCharacters { path: PathBuf, invalid_char: char },
    /// Target file name is empty or dot-only.
    EmptyTargetName { source: PathBuf },
}

/// A single file action within a rename plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameAction {
    pub source: PathBuf,
    pub target: PathBuf,
    pub changed: bool,
}

/// Summary report from dry-running a batch rename plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameDryRunReport {
    pub total_files: usize,
    pub changed_count: usize,
    pub unchanged_count: usize,
    pub conflict_count: usize,
    pub conflicts: Vec<RenameConflict>,
    pub actions: Vec<RenameAction>,
}

/// A fully validated, dry-run-able and reversible batch rename plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePlan {
    pub actions: Vec<RenameAction>,
    pub conflicts: Vec<RenameConflict>,
}

impl RenamePlan {
    pub fn is_valid(&self) -> bool {
        self.conflicts.is_empty()
    }

    pub fn dry_run(&self) -> RenameDryRunReport {
        let changed_count = self.actions.iter().filter(|a| a.changed).count();
        let unchanged_count = self.actions.len().saturating_sub(changed_count);
        RenameDryRunReport {
            total_files: self.actions.len(),
            changed_count,
            unchanged_count,
            conflict_count: self.conflicts.len(),
            conflicts: self.conflicts.clone(),
            actions: self.actions.clone(),
        }
    }

    /// Generates an undo plan that maps each target back to its original source.
    pub fn generate_undo_plan(&self) -> RenameUndoPlan {
        let mut undo_actions = Vec::new();
        for action in &self.actions {
            if action.changed {
                undo_actions.push(RenameAction {
                    source: action.target.clone(),
                    target: action.source.clone(),
                    changed: true,
                });
            }
        }
        RenameUndoPlan {
            actions: undo_actions,
        }
    }
}

/// Reversible undo plan for rolled-back rename operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameUndoPlan {
    pub actions: Vec<RenameAction>,
}

impl RenameUndoPlan {
    /// Validates that undoing restores the exact set of original source paths.
    pub fn verify_restores_sources(&self, original_sources: &[PathBuf]) -> bool {
        let restored_targets: HashSet<_> = self.actions.iter().map(|a| &a.target).collect();
        original_sources
            .iter()
            .all(|s| restored_targets.contains(s))
    }
}

/// Batch rename planner and validator.
pub struct RenamePlanner;

impl RenamePlanner {
    const FORBIDDEN_CHARS: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

    /// Plans a batch rename for `files` using an ordered pipeline of `rules`.
    pub fn plan(files: &[PathBuf], rules: &[RenameRule]) -> PowerResult<RenamePlan> {
        let mut actions = Vec::with_capacity(files.len());
        let mut target_map: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        let mut conflicts = Vec::new();

        for (index, source) in files.iter().enumerate() {
            let parent = source.parent().unwrap_or_else(|| Path::new(""));
            let file_name = source
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            if file_name.is_empty() {
                conflicts.push(RenameConflict::EmptyTargetName {
                    source: source.clone(),
                });
                continue;
            }

            // Split into stem and extension
            let (mut current_stem, mut current_ext) = {
                let p = Path::new(&file_name);
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| file_name.clone());
                let ext = p.extension().map(|s| s.to_string_lossy().to_string());
                (stem, ext)
            };

            // Apply all rules sequentially
            for rule in rules {
                let (new_stem, new_ext) =
                    rule.apply(&current_stem, current_ext.as_deref(), index)?;
                current_stem = new_stem;
                current_ext = new_ext;
            }

            // Assemble new file name
            let new_filename = match current_ext {
                Some(ref ext) if !ext.is_empty() => format!("{}.{}", current_stem, ext),
                _ => current_stem,
            };

            if new_filename.is_empty() || new_filename == "." || new_filename == ".." {
                conflicts.push(RenameConflict::EmptyTargetName {
                    source: source.clone(),
                });
                continue;
            }

            // Check for forbidden characters in target filename
            let mut has_invalid_char = false;
            for ch in Self::FORBIDDEN_CHARS {
                if new_filename.contains(ch) {
                    conflicts.push(RenameConflict::InvalidCharacters {
                        path: parent.join(&new_filename),
                        invalid_char: ch,
                    });
                    has_invalid_char = true;
                    break;
                }
            }
            if has_invalid_char {
                continue;
            }

            let target = parent.join(&new_filename);
            let changed = &target != source;

            actions.push(RenameAction {
                source: source.clone(),
                target: target.clone(),
                changed,
            });

            target_map.entry(target).or_default().push(source.clone());
        }

        // Detect collisions where multiple sources map to the same target
        for (target, sources) in target_map {
            if sources.len() > 1 {
                conflicts.push(RenameConflict::CollisionWithBatch { target, sources });
            }
        }

        Ok(RenamePlan { actions, conflicts })
    }
}
