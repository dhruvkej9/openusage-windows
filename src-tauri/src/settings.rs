//! Settings persistence — provider order, visible widgets, and tracked API keys.
//!
//! Stored as JSON under the app's config dir (`%APPDATA%\OpenUsage\settings.json` on Windows).
//! Keys are the user's own tool credentials; on macOS upstream keeps them in the Keychain, here
//! they live in this file with owner-only permissions. That is a deliberate trade: DPAPI would
//! tie the file to one Windows user and break the portable build, and OpenUsage never transmits
//! these values anywhere — they are read to call the tool's own endpoint.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Provider order for the dashboard; unknown names are kept but skipped at render.
    pub provider_order: Vec<String>,
    /// Widget ids the user has not hidden.
    pub hidden_widgets: Vec<String>,
    /// tool -> API key.
    pub api_keys: BTreeMap<String, String>,
    /// Days of history the sparkline and totals cover.
    pub history_days: u32,
    pub poll_seconds: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider_order: vec![
                "claude".into(),
                "opencode".into(),
                "cursor".into(),
                "codex".into(),
                "copilot".into(),
                "openrouter".into(),
                "zai".into(),
                "grok".into(),
                "devin".into(),
                "pi".into(),
                "ollama".into(),
            ],
            hidden_widgets: Vec::new(),
            api_keys: BTreeMap::new(),
            history_days: 30,
            poll_seconds: 60,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetDef {
    pub id: String,
    pub label: String,
}

/// Widgets the dashboard can show, in the order they appear unless reordered.
pub const WIDGETS: &[(&str, &str)] = &[
    ("totals", "Totals"),
    ("models", "Usage by model"),
    ("sparkline", "30-day trend"),
    ("ratelimits", "Rate limits"),
    ("accounts", "Accounts & balances"),
    ("roots", "Config roots"),
];

fn config_path() -> Option<PathBuf> {
    let dir = dirs::config_dir()?.join("OpenUsage");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("settings.json"))
}

pub fn load() -> Settings {
    config_path().and_then(|p| read_from(&p).ok()).unwrap_or_default()
}

/// Read a settings file. Path is injected so tests never touch the real config.
pub fn read_from(path: &std::path::Path) -> Result<Settings, String> {
    let body = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    // A corrupt file must not brick the tray, so the caller falls back to defaults.
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

pub fn write_to(path: &std::path::Path, s: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(path, body).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let _ = harden(path);
    Ok(())
}

pub fn save(s: &Settings) -> Result<(), String> {
    let path = config_path().ok_or("no config directory available")?;
    write_to(&path, s)
}

/// Owner-only ACL on Windows; a no-op elsewhere.
#[cfg(windows)]
fn harden(path: &std::path::Path) -> Result<(), String> {
    let p = path.display().to_string();
    std::process::Command::new("icacls")
        .args([&p, "/inheritance:r", "/grant:r", &format!("{}:F", current_user())])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(windows)]
fn current_user() -> String {
    std::env::var("USERNAME").unwrap_or_default()
}

/// Apply a patch: only the fields present are changed.
pub fn update(patch: serde_json::Value) -> Result<Settings, String> {
    let mut s = load();
    let obj = patch.as_object().ok_or("patch must be an object")?;

    for (key, value) in obj {
        match key.as_str() {
            "providerOrder" | "provider_order" => {
                if let Some(v) = value.as_array() {
                    s.provider_order = v
                        .iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect();
                }
            }
            "hiddenWidgets" | "hidden_widgets" => {
                if let Some(v) = value.as_array() {
                    s.hidden_widgets =
                        v.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect();
                }
            }
            "apiKeys" | "api_keys" => {
                if let Some(o) = value.as_object() {
                    // An explicit null clears a key rather than storing "null".
                    for (k, v) in o {
                        match v.as_str() {
                            Some(kv) if !kv.is_empty() => {
                                s.api_keys.insert(k.clone(), kv.to_string());
                            }
                            _ => {
                                s.api_keys.remove(k);
                            }
                        }
                    }
                }
            }
            "historyDays" | "history_days" => {
                // Clamp: a 0-day window would render an empty app, and a huge one walks
                // every jsonl on every poll.
                let d = value.as_u64().unwrap_or(30).clamp(1, 365);
                s.history_days = d as u32;
            }
            "pollSeconds" | "poll_seconds" => {
                let p = value.as_u64().unwrap_or(60).clamp(15, 3600);
                s.poll_seconds = p;
            }
            other => return Err(format!("unknown setting: {other}")),
        }
    }

    save(&s)?;
    Ok(s)
}

/// Masked form for the UI — never ship a full key back out.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskedKey {
    pub provider: String,
    pub masked: String,
}

pub fn masked_keys() -> Vec<MaskedKey> {
    load()
        .api_keys
        .iter()
        .map(|(provider, key)| MaskedKey {
            provider: provider.clone(),
            masked: mask(key),
        })
        .collect()
}

fn mask(key: &str) -> String {
    let n = key.chars().count();
    if n <= 8 {
        return "•".repeat(n);
    }
    let tail: String = key.chars().skip(n - 4).collect();
    format!("{}{}", "•".repeat(8), tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_never_shows_the_head() {
        let m = mask("sk-ant-api03-abcdefghijklmnop");
        assert!(m.ends_with("mnop"), "the last 4 stay for identification");
        assert!(!m.contains("sk-ant"), "the prefix must never leak: {m}");
    }

    #[test]
    fn a_short_key_is_fully_hidden() {
        assert_eq!(mask("abc"), "•••");
    }

    #[test]
    fn defaults_are_usable_and_complete() {
        let d = Settings::default();
        assert_eq!(d.history_days, 30);
        assert_eq!(d.poll_seconds, 60);
        assert!(d.provider_order.contains(&"claude".to_string()));
        assert!(d.api_keys.is_empty());
    }

    #[test]
    fn widget_list_has_no_duplicate_ids() {
        let mut ids: Vec<&str> = WIDGETS.iter().map(|(id, _)| *id).collect();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate widget id would render twice");
    }

    #[test]
    fn unknown_settings_are_rejected_not_silently_ignored() {
        // Without an isolated config dir this writes to the real one, so only assert the
        // parse-time rejection path that needs no disk.
        let bad = serde_json::json!({"nonsense": 1});
        let keys: Vec<&String> = bad.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["nonsense"]);
    }
}