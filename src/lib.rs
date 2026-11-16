//! RenameX: batch-rename planning engine.
//!
//! RenameX never renames anything itself — it turns a set of files + rules into a
//! *plan* that the Orbit host shows to the user before touching disk. The plan flags
//! duplicate target names and collisions with existing files; callers must refuse the
//! plan (or demand explicit confirmation) when either occurs.
//!
//! Token pattern syntax (default `{name}`): `{name}` original stem, `{ext}` extension
//! with dot, `{counter}` zero-padded counter, `{date}` date from `date_ms`. An optional
//! regex find/replace pass is applied to the stem *before* token expansion.

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenameError {
    #[error("invalid regex: {0}")]
    InvalidRegex(String),
    #[error("pattern must produce a non-empty file name")]
    EmptyResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameRules {
    /// e.g. `{name}-{counter}` — see crate docs.
    pub pattern: String,
    /// Optional regex applied to the stem before expansion.
    pub find_regex: Option<String>,
    /// Replacement for `find_regex` (supports `$1` capture references).
    pub replace: String,
    pub counter_start: u32,
    /// Zero-padding width for `{counter}` (0 = no padding).
    pub padding: u32,
    /// Milliseconds since Unix epoch, used for `{date}`.
    pub date_ms: u64,
    /// Date format built from tokens: YYYY, MM, DD.
    pub date_format: String,
}

impl Default for RenameRules {
    fn default() -> Self {
        Self {
            pattern: "{name}".into(),
            find_regex: None,
            replace: String::new(),
            counter_start: 1,
            padding: 0,
            date_ms: 0,
            date_format: "YYYY-MM-DD".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePlanEntry {
    pub original: String,
    pub new_name: String,
    /// Non-fatal observations (e.g. lossy case change). Never blocks.
    pub warnings: Vec<String>,
}

/// Split file name into (stem, extension-with-dot). Leading dots (`.gitignore`) → stem only.
fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Days-from-civil (Howard Hinnant's algorithm), days since 1970-01-01.
#[cfg(test)]
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Civil-from-days (Hinnant), inverse of days_from_civil.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn format_date(date_ms: u64, format: &str) -> String {
    let days = (date_ms / 86_400_000) as i64;
    let (y, m, d) = civil_from_days(days);
    format
        .replace("YYYY", &format!("{y:04}"))
        .replace("MM", &format!("{m:02}"))
        .replace("DD", &format!("{d:02}"))
}

fn expand_tokens(pattern: &str, stem: &str, ext: &str, counter: u32, padding: u32, date: &str) -> String {
    let counter_s = if padding > 0 {
        format!("{counter:0width$}", width = padding as usize)
    } else {
        counter.to_string()
    };
    pattern
        .replace("{name}", stem)
        .replace("{counter}", &counter_s)
        .replace("{date}", date)
        .replace("{ext}", ext) // includes leading dot when present
}

/// Build a rename plan. `files` are plain file names (not paths). Unrepresentable
/// input yields an entry whose new_name is empty — callers must treat that as fatal.
pub fn build_plan(rules: &RenameRules, files: &[&str]) -> Result<Vec<RenamePlanEntry>, RenameError> {
    let find = match &rules.find_regex {
        Some(pat) if !pat.is_empty() => {
            Some(Regex::new(pat).map_err(|e| RenameError::InvalidRegex(e.to_string()))?)
        }
        _ => None,
    };
    let date = format_date(rules.date_ms, &rules.date_format);
    let mut plan = Vec::with_capacity(files.len());
    for (i, name) in files.iter().enumerate() {
        let (stem, ext) = split_name(name);
        let stem2 = match &find {
            Some(re) => re.replace_all(stem, rules.replace.as_str()).into_owned(),
            None => stem.to_string(),
        };
        let counter = rules.counter_start + i as u32;
        let mut new_name = expand_tokens(&rules.pattern, &stem2, ext, counter, rules.padding, &date);
        // If the pattern ends with a literal dot before {ext} and the file has no
        // extension, one trailing dot is an artifact — strip it. Internal dots are
        // the user's business and are never touched.
        if ext.is_empty() && new_name.ends_with('.') {
            new_name.pop();
        }
        let mut warnings = Vec::new();
        if new_name.is_empty() {
            warnings.push("pattern produced an empty file name".into());
        }
        if new_name == *name {
            warnings.push("no change".into());
        }
        plan.push(RenamePlanEntry { original: (*name).to_string(), new_name, warnings });
    }
    Ok(plan)
}

/// Target names appearing more than once in the plan (case-insensitive, like most filesystems).
pub fn detect_duplicate_newnames(plan: &[RenamePlanEntry]) -> Vec<String> {
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    for e in plan {
        *seen.entry(e.new_name.to_lowercase()).or_default() += 1;
    }
    seen.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k).collect()
}

/// Which planned target names already exist on disk (exact, case-insensitive match).
/// Callers MUST refuse these unless the user explicitly confirmed overwriting.
pub fn would_overwrite(plan: &[RenamePlanEntry], existing_names: &[&str]) -> Vec<String> {
    let existing: std::collections::HashSet<String> =
        existing_names.iter().map(|s| s.to_lowercase()).collect();
    plan.iter()
        .filter(|e| {
            existing.contains(&e.new_name.to_lowercase())
                && e.new_name.to_lowercase() != e.original.to_lowercase()
        })
        .map(|e| e.new_name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_is_1970_01_01() {
        assert_eq!(format_date(0, "YYYY-MM-DD"), "1970-01-01");
    }

    #[test]
    fn known_date_renders() {
        // 946_684_800 s = 2000-01-01 00:00:00 UTC
        assert_eq!(format_date(946_684_800_000, "YYYY-MM-DD"), "2000-01-01");
        assert_eq!(format_date(946_684_800_000, "DD.MM.YYYY"), "01.01.2000");
    }

    #[test]
    fn days_civil_roundtrip() {
        for days in [0, 10957, 14610, 20000, 20_000, 30_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn token_expansion_with_counter_padding() {
        let rules = RenameRules { pattern: "shot-{counter}{ext}".into(), counter_start: 1, padding: 3, ..Default::default() };
        let plan = build_plan(&rules, &["a.png", "b.png"]).unwrap();
        assert_eq!(plan[0].new_name, "shot-001.png");
        assert_eq!(plan[1].new_name, "shot-002.png");
    }

    #[test]
    fn name_and_ext_tokens() {
        let rules = RenameRules { pattern: "{name}-backup{ext}".into(), ..Default::default() };
        let plan = build_plan(&rules, &["photo.jpg"]).unwrap();
        assert_eq!(plan[0].new_name, "photo-backup.jpg");
    }

    #[test]
    fn regex_pass_applies_before_tokens() {
        let rules = RenameRules {
            pattern: "{name}{ext}".into(),
            find_regex: Some(r"\s+".into()),
            replace: "_".into(),
            ..Default::default()
        };
        let plan = build_plan(&rules, &["my  file  name.txt"]).unwrap();
        assert_eq!(plan[0].new_name, "my_file_name.txt");
    }

    #[test]
    fn regex_capture_groups_work() {
        let rules = RenameRules {
            pattern: "{name}{ext}".into(),
            find_regex: Some(r"(\d+)".into()),
            replace: "[$1]".into(),
            ..Default::default()
        };
        let plan = build_plan(&rules, &["img9.png"]).unwrap();
        assert_eq!(plan[0].new_name, "img[9].png");
    }

    #[test]
    fn invalid_regex_is_error_not_panic() {
        let rules = RenameRules { find_regex: Some("(".into()), ..Default::default() };
        assert!(matches!(build_plan(&rules, &["x"]), Err(RenameError::InvalidRegex(_))));
    }

    #[test]
    fn duplicate_targets_detected_case_insensitively() {
        let rules = RenameRules { pattern: "same{ext}".into(), ..Default::default() };
        let plan = build_plan(&rules, &["a.txt", "b.txt"]).unwrap();
        assert_eq!(detect_duplicate_newnames(&plan), vec!["same.txt"]);
    }

    #[test]
    fn would_overwrite_flags_existing() {
        // Identity plans (target ci-equal to source) are not overwrites.
        let rules = RenameRules { pattern: "{name}{ext}".into(), ..Default::default() };
        let plan = build_plan(&rules, &["report.txt"]).unwrap();
        assert_eq!(would_overwrite(&plan, &["REPORT.txt"]), Vec::<String>::new());

        // A rename whose target ci-equals another existing file must be flagged.
        let rules = RenameRules { pattern: "{name}-bak{ext}".into(), ..Default::default() };
        let plan = build_plan(&rules, &["report.txt"]).unwrap();
        assert_eq!(would_overwrite(&plan, &["REPORT-BAK.TXT"]), vec!["report-bak.txt".to_string()]);
    }
}
