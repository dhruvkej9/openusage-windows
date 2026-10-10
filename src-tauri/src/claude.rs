//! Claude Code usage scanner.
//!
//! Ported from openusage's `Providers/Claude/ClaudeLogUsageScanner.swift`, trimmed to the
//! offline path: read `projects/**/*.jsonl`, sum tokens per model, price them locally.
//!
//! Reads every config root it finds so a Windows install picks up both the native
//! `~/.claude` and a WSL Ubuntu home via `\\wsl$\Ubuntu\home\<user>\.claude`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBreakdown {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenBreakdown {
    fn add(&mut self, other: &TokenBreakdown) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }

    /// Billable total: cached reads and writes are priced, so they count.
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model: String,
    pub tokens: TokenBreakdown,
    pub cost_usd: f64,
    pub requests: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    /// Absolute paths of the jsonl files that fed this report, so the series builder
    /// walks the same set instead of re-discovering (and possibly diverging).
    pub files: Vec<String>,
    /// Where the data came from, so a UI can show "WSL: Ubuntu".
    pub roots: Vec<String>,
    pub models: Vec<ModelUsage>,
    pub total_tokens: u64,
    pub total_cost_usd: f64,
    pub total_requests: u64,
    /// ISO date of the newest entry scanned (the "last used" line).
    pub last_used: Option<String>,
    pub scanned_files: usize,
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct RawMessage {
    model: Option<String>,
    usage: Option<RawUsage>,
}

#[derive(Deserialize)]
struct RawEntry {
    #[serde(default)]
    message: Option<RawMessage>,
}

/// Per-million-token rates, USD. Kept local so the app works offline; the upstream
/// macOS build downloads a catalog, which is not worth the network round trip here.
struct Rates {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

fn rates_for(model: &str) -> Rates {
    let m = model.to_ascii_lowercase();
    // Cache writes cost 1.25x input and cache reads 0.1x on every current tier.
    let (input, output) = if m.contains("opus") {
        (15.0, 75.0)
    } else if m.contains("haiku") {
        (1.0, 5.0)
    } else {
        // Sonnet and anything unrecognised.
        (3.0, 15.0)
    };
    Rates {
        input,
        output,
        cache_read: input * 0.1,
        cache_write: input * 1.25,
    }
}

fn cost_of(model: &str, t: &TokenBreakdown) -> f64 {
    let r = rates_for(model);
    (t.input as f64 * r.input
        + t.output as f64 * r.output
        + t.cache_read as f64 * r.cache_read
        + t.cache_write as f64 * r.cache_write)
        / 1_000_000.0
}

/// Every place a Claude home can live on a Windows box.
///
/// WSL roots matter: Claude Code installed inside Ubuntu keeps its history there,
/// invisible to a native `%USERPROFILE%\.claude`.
pub fn config_roots() -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();

    let push = |label: &str, p: PathBuf, out: &mut Vec<(String, PathBuf)>| {
        if p.join("projects").is_dir() && !out.iter().any(|(_, e)| e == &p) {
            out.push((label.to_string(), p));
        }
    };

    if let Some(home) = dirs::home_dir() {
        push("Windows", home.join(".claude"), &mut out);
        if let Ok(d) = std::env::var("CLAUDE_CONFIG_DIR") {
            if !d.is_empty() {
                push("CLAUDE_CONFIG_DIR", PathBuf::from(d), &mut out);
            }
        }
    }

    // WSL: \\wsl$\<distro>\home\<user>\.claude and the rootfs fallback /home/<user>.
    for distro in wsl_distros() {
        let base = PathBuf::from(format!(r"\\wsl$\{}\", distro));
        let Some(users) = std::fs::read_dir(base.join("home")).ok() else {
            continue;
        };
        for entry in users.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            // The distro's own default user is the likely one; every user dir is cheap to check.
            push(
                &format!("WSL: {} ({})", name, distro),
                entry.path().join(".claude"),
                &mut out,
            );
        }
    }

    out
}

fn wsl_distros() -> Vec<String> {
    // `wsl.exe -l -q` prints UTF-16LE with a NUL after each name and a trailing blank line.
    let Ok(out) = std::process::Command::new("wsl.exe")
        .args(["-l", "-q"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("Windows Subsystem"))
        .map(str::to_owned)
        .collect()
}

fn jsonl_files(root: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    // ponytail: no symlink-loop guard — a loop needs an actual cycle under projects/, which
    // Claude Code never creates. Add a visited set if you ever point this at a home dir.
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            jsonl_files(&path, out, depth + 1);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
}

fn modified_within(path: &Path, max_age: Duration) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(mtime)
        .map(|d| d < max_age)
        .unwrap_or(false)
}

/// How far back to scan. Kept in one place so the report and the series builder cannot
/// disagree about which files are in scope.
pub fn max_age() -> Duration {
    let days = crate::settings::load().history_days.max(1) as u64;
    Duration::from_secs(days * 24 * 3600)
}

pub fn scan() -> UsageReport {
    let mut report = UsageReport::default();
    let mut per_model: HashMap<String, ModelUsage> = HashMap::new();
    let max_age = max_age();

    for (label, root) in config_roots() {
        report.roots.push(label);
        let mut files = Vec::new();
        jsonl_files(&root.join("projects"), &mut files, 0);
        files.sort();

        for file in files {
            if !modified_within(&file, max_age) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            report.scanned_files += 1;
            report.files.push(file.to_string_lossy().into_owned());

            for line in text.lines() {
                let Ok(entry) = serde_json::from_str::<RawEntry>(line) else {
                    continue; // queue-operation and other non-usage lines.
                };
                let Some(msg) = entry.message else { continue };
                let Some(usage) = msg.usage else { continue };
                // `<synthetic>` is a placeholder for locally-injected context (cost $0, no real
                // model behind it); upstream filters it too. Dropping it keeps the tray honest.
                let model = match msg.model.as_deref() {
                    Some(m) if !m.is_empty() && m != "<synthetic>" => m.to_string(),
                    _ => continue,
                };

                let tokens = TokenBreakdown {
                    input: usage.input_tokens,
                    output: usage.output_tokens,
                    cache_read: usage.cache_read_input_tokens.unwrap_or(0),
                    cache_write: usage.cache_creation_input_tokens.unwrap_or(0),
                };

                let entry = per_model.entry(model.clone()).or_insert_with(|| ModelUsage {
                    model: model.clone(),
                    ..Default::default()
                });
                entry.tokens.add(&tokens);
                entry.cost_usd += cost_of(&model, &tokens);
                entry.requests += 1;
            }
        }
    }

    let mut models: Vec<ModelUsage> = per_model.into_values().collect();
    models.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd));
    for m in &models {
        report.total_tokens += m.tokens.total();
        report.total_cost_usd += m.cost_usd;
        report.total_requests += m.requests;
    }
    report.models = models;
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_a_mixed_usage_entry() {
        // A real line from ~/.claude/projects (Claude Opus 5):
        //   in 2 / out 477 / cache_read 36697 / cache_write 20792
        let t = TokenBreakdown {
            input: 2,
            output: 477,
            cache_read: 36_697,
            cache_write: 20_792,
        };
        // opus: 15/75 per million, cache_read = 0.1x input, cache_write = 1.25x input.
        // 0.00003 + 0.035775 + 0.0550455 + 0.38985
        assert!((cost_of("claude-opus-5", &t) - 0.4807005).abs() < 1e-6);
    }

    #[test]
    fn haiku_is_cheaper_than_opus() {
        let t = TokenBreakdown {
            input: 1_000_000,
            output: 0,
            cache_read: 0,
            cache_write: 0,
        };
        assert!(cost_of("claude-haiku-4-5", &t) < cost_of("claude-opus-5", &t));
    }

    #[test]
    fn every_token_kind_is_billable() {
        let t = TokenBreakdown {
            input: 1,
            output: 1,
            cache_read: 1,
            cache_write: 1,
        };
        assert_eq!(t.total(), 4);
    }

    #[test]
    fn sonnet_is_the_default_rate() {
        // An unrecognised model must not be priced at opus rates.
        assert_eq!(rates_for("some-new-model").input, rates_for("claude-sonnet-5").input);
    }
}
/// Every jsonl under every Claude root, for callers that need the raw files.
pub fn all_usage_files() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let max_age = max_age();
    for (_label, root) in config_roots() {
        let mut files = Vec::new();
        jsonl_files(&root.join("projects"), &mut files, 0);
        files.sort();
        out.extend(files.into_iter().filter(|f| modified_within(f, max_age)));
    }
    out
}
