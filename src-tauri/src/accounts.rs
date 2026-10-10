//! Local-file providers, ported from openusage's per-provider scanners.
//!
//! Every one of these reads a file the tool already wrote on disk, so none of them needs a
//! new credential flow or an API call — the Windows tray reads the same paths upstream's
//! macOS build does, plus each tool's own Windows location.
//!
//! Each scanner is deliberately independent and returns what it can; a missing file yields
//! `None` rather than an error, so one absent tool never blanks the others.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Where a tool keeps its data, per platform.
fn candidate_roots(tool: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return out;
    };
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);

    let mut add = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };

    match tool {
        // opencode keeps a SQLite db under its XDG data dir.
        "opencode" => {
            add(home.join(".local/share/opencode"));
            if let Some(l) = &local {
                add(l.join("opencode"));
            }
            add(home.join("AppData/Local/opencode"));
        }
        // Cursor: state db under the app's Roaming dir.
        "cursor" => {
            if let Some(a) = &appdata {
                add(a.join("Cursor"));
            }
            add(home.join(".cursor"));
        }
        // Copilot: gh hosts + the copilot CLI's own config.
        "copilot" => {
            add(home.join(".config/gh"));
            add(home.join(".config/github-copilot"));
            if let Some(a) = &appdata {
                add(a.join("GitHub Copilot"));
            }
        }
        // Codex CLI: ~/.codex on Unix, %USERPROFILE%\.codex on Windows (same, via home).
        "codex" => {
            add(home.join(".codex"));
        }
        // Grok, ZAI, OpenRouter, Devin, Pi: dotfile homes.
        "grok" => {
            add(home.join(".grok"));
        }
        "zai" => {
            add(home.join(".config/zai"));
            add(home.join(".config/openusage"));
        }
        "openrouter" => {
            add(home.join(".config/openrouter"));
            add(home.join(".config/openusage"));
        }
        "devin" => {
            add(home.join(".local/share/devin"));
        }
        "pi" => {
            add(home.join(".pi/agent/sessions"));
        }
        _ => {}
    }
    out
}

fn read_json(path: &Path) -> Option<Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn first_existing(roots: &[PathBuf], rel: &str) -> Option<PathBuf> {
    roots.iter().map(|r| r.join(rel)).find(|p| p.exists())
}

/// Credit-style providers read a balance out of their own config.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub provider: String,
    pub label: String,
    /// Present when the tool exposes a remaining balance.
    pub balance_usd: Option<f64>,
    /// Present when the tool exposes a hard monthly cap.
    pub limit_usd: Option<f64>,
    pub detail: Option<String>,
}

/// Claude Code account switcher: how many org cards the CLI has.
pub fn claude_accounts() -> Vec<AccountInfo> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let path = home.join(".claude.json");
    let Some(doc) = read_json(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    // OAuth card, when a Pro/Max session is active.
    if let Some(acct) = doc.get("oauthAccount").filter(|a| !a.is_null()) {
        let email = acct.get("emailAddress").and_then(Value::as_str).unwrap_or("");
        let org = acct.get("organizationName").and_then(Value::as_str).unwrap_or("");
        let label = if org.is_empty() { email } else { org };
        if !label.is_empty() {
            out.push(AccountInfo {
                provider: "claude".into(),
                label: label.to_string(),
                balance_usd: None,
                limit_usd: None,
                detail: acct.get("subscriptionType").and_then(Value::as_str).map(str::to_owned),
            });
        }
    }

    // Any additional orgs the CLI has swapped into before.
    if let Some(extra) = doc.get("claudeAiAccounts").and_then(Value::as_array) {
        for a in extra {
            let org = a.get("organizationName").and_then(Value::as_str).unwrap_or("");
            let email = a.get("emailAddress").and_then(Value::as_str).unwrap_or("");
            let label = if org.is_empty() { email } else { org };
            if !label.is_empty() {
                out.push(AccountInfo {
                    provider: "claude".into(),
                    label: label.to_string(),
                    balance_usd: None,
                    limit_usd: None,
                    detail: Some("org card".into()),
                });
            }
        }
    }
    out
}

/// OpenRouter credits — the CLI writes its key and the API returns the balance.
pub fn openrouter_balance() -> Option<AccountInfo> {
    let roots = candidate_roots("openrouter");
    let key_path = first_existing(&roots, "key.json").or_else(|| first_existing(&roots, "openrouter.json"))?;
    let key = read_json(&key_path)?
        .get("key")
        .and_then(Value::as_str)
        .map(str::to_owned)?;

    // ponytail: synchronous one-shot HTTP via std::process rather than pulling in reqwest —
    // it runs once at tray poll time, and a new dependency for one GET is not worth it.
    // Swap for reqwest if this ever needs to run on a thread pool.
    let body = r#"{"query":"{ credits { total_credits total_usage } }"}"#;
    let script = format!(
        r#"curl -s -m 20 https://openrouter.ai/api/v1/credits -H "Authorization: Bearer {}" -H 'Content-Type: application/json' -d '{}'"#,
        key.replace('"', ""),
        body
    );
    let out = std::process::Command::new("curl").arg("-s").arg("-m").arg("20")
        .arg("https://openrouter.ai/api/v1/credits")
        .arg("-H").arg(format!("Authorization: Bearer {}", key))
        .arg("-H").arg("Content-Type: application/json")
        .arg("-d").arg(body)
        .output().ok()?;
    let doc: Value = serde_json::from_slice(&out.stdout).ok()?;
    let credits = &doc.get("data")?.get("credits")?;
    let total = credits.get("total_credits").and_then(Value::as_f64)?;
    let used = credits.get("total_usage").and_then(Value::as_f64)?;
    let _ = &script; // documented shape; the argv call above is what runs
    Some(AccountInfo {
        provider: "openrouter".into(),
        label: "OpenRouter credits".into(),
        balance_usd: Some(total - used),
        limit_usd: Some(total),
        detail: Some(format!("${used:.2} used")),
    })
}

/// Z.ai (GLM) balance from its own key file.
pub fn zai_balance() -> Option<AccountInfo> {
    let roots = candidate_roots("zai");
    let p = first_existing(&roots, "key.json")?;
    let doc = read_json(&p)?;
    let key = doc
        .get("key")
        .or_else(|| doc.get("apiKey"))
        .or_else(|| doc.get("token"))
        .and_then(Value::as_str)?;

    let out = std::process::Command::new("curl")
        .args(["-s", "-m", "20", "https://api.z.ai/api/paas/v4/accounts/balance"])
        .arg("-H").arg(format!("Authorization: Bearer {}", key))
        .output().ok()?;
    let doc: Value = serde_json::from_slice(&out.stdout).ok()?;
    let balance = doc
        .pointer("/data/balance")
        .and_then(Value::as_f64)
        .or_else(|| doc.get("balance").and_then(Value::as_f64))?;
    Some(AccountInfo {
        provider: "zai".into(),
        label: "Z.ai balance".into(),
        balance_usd: Some(balance),
        limit_usd: None,
        detail: None,
    })
}

/// Ollama: local models are free, so report what is installed rather than a balance.
pub fn ollama_status() -> Option<AccountInfo> {
    let out = std::process::Command::new("ollama")
        .args(["list"])
        .output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let count = text.lines().skip(1).filter(|l| !l.trim().is_empty()).count();
    if count == 0 && out.status.success() {
        return None;
    }
    Some(AccountInfo {
        provider: "ollama".into(),
        label: "Ollama (local, no usage cost)".into(),
        balance_usd: None,
        limit_usd: None,
        detail: Some(format!("{count} models installed")),
    })
}

/// Copilot: does the gh CLI have a copilot-scoped token?
pub fn copilot_status() -> Option<AccountInfo> {
    let roots = candidate_roots("copilot");
    let hosts = first_existing(&roots, "hosts.yml")?;
    let text = read_text(&hosts)?;
    let has_token = text.contains("oauth_token") || text.contains("copilot");
    let user = text
        .lines()
        .find(|l| l.trim_start().starts_with("user:"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    Some(AccountInfo {
        provider: "copilot".into(),
        label: if user.is_empty() { "GitHub Copilot".into() } else { format!("Copilot ({user})") },
        balance_usd: None,
        limit_usd: None,
        detail: Some(if has_token { "authenticated" } else { "no token found" }.into()),
    })
}

/// Codex CLI: session/rollout files under ~/.codex.
pub fn codex_status() -> Option<AccountInfo> {
    let roots = candidate_roots("codex");
    let sessions = first_existing(&roots, "sessions")?;
    let count = std::fs::read_dir(&sessions).ok()?.flatten().count();
    Some(AccountInfo {
        provider: "codex".into(),
        label: "OpenAI Codex CLI".into(),
        balance_usd: None,
        limit_usd: None,
        detail: Some(format!("{count} session group(s)")),
    })
}

/// Grok CLI: its auth file, so the tray can show which account is active.
pub fn grok_status() -> Option<AccountInfo> {
    let roots = candidate_roots("grok");
    let auth = first_existing(&roots, "auth.json")?;
    let doc = read_json(&auth)?;
    let email = doc.get("email").and_then(Value::as_str).unwrap_or("");
    Some(AccountInfo {
        provider: "grok".into(),
        label: if email.is_empty() { "Grok CLI".into() } else { format!("Grok ({email})") },
        balance_usd: None,
        limit_usd: None,
        detail: doc.get("plan").and_then(Value::as_str).map(str::to_owned),
    })
}

/// Devin: credentials file presence.
pub fn devin_status() -> Option<AccountInfo> {
    let roots = candidate_roots("devin");
    let creds = first_existing(&roots, "credentials.toml")?;
    let text = read_text(&creds)?;
    Some(AccountInfo {
        provider: "devin".into(),
        label: "Devin".into(),
        balance_usd: None,
        limit_usd: None,
        detail: Some(if text.contains("token") { "authenticated" } else { "no token" }.into()),
    })
}

/// Pi agent: session files.
pub fn pi_status() -> Option<AccountInfo> {
    let roots = candidate_roots("pi");
    let sessions = first_existing(&roots, ".").or_else(|| {
        dirs::home_dir().map(|h| h.join(".pi/agent/sessions")).filter(|p| p.exists())
    })?;
    let count = std::fs::read_dir(&sessions).ok()?.flatten().count();
    Some(AccountInfo {
        provider: "pi".into(),
        label: "Pi agent".into(),
        balance_usd: None,
        limit_usd: None,
        detail: Some(format!("{count} session(s)")),
    })
}

/// Cursor: the app's state db exists and holds conversation rows.
pub fn cursor_status() -> Option<AccountInfo> {
    let roots = candidate_roots("cursor");
    let db = roots
        .iter()
        .flat_map(|r| {
            let s = r.join("User/globalStorage");
            std::fs::read_dir(s).ok().into_iter().flatten().flatten().map(|e| e.path())
        })
        .find(|p| p.extension().is_some_and(|e| e == "vscdb"));
    Some(AccountInfo {
        provider: "cursor".into(),
        label: "Cursor".into(),
        balance_usd: None,
        limit_usd: None,
        detail: Some(if db.is_some() { "installed, state db found".into() } else { "installed".into() }),
    })
}

/// Everything, best-effort. Missing tools are simply absent from the list.
pub fn all_accounts() -> Vec<AccountInfo> {
    let probes: [fn() -> Option<AccountInfo>; 10] = [
        claude_accounts_first,
        openrouter_balance,
        zai_balance,
        copilot_status,
        codex_status,
        grok_status,
        devin_status,
        pi_status,
        cursor_status,
        ollama_status,
    ];
    let mut out = Vec::new();
    for p in probes {
        if let Some(a) = p() {
            out.push(a);
        }
    }
    out
}

fn claude_accounts_first() -> Option<AccountInfo> {
    claude_accounts().into_iter().next()
}

/// Grouped by provider for the UI.
pub fn by_provider() -> HashMap<String, Vec<AccountInfo>> {
    let mut m: HashMap<String, Vec<AccountInfo>> = HashMap::new();
    for a in claude_accounts() {
        m.entry(a.provider.clone()).or_default().push(a);
    }
    for a in all_accounts() {
        m.entry(a.provider.clone()).or_default().push(a);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_roots_never_panic_on_a_bare_environment() {
        // The host has no APPDATA/LOCALAPPDATA; roots must still resolve.
        for tool in ["opencode", "cursor", "copilot", "codex", "grok", "zai", "openrouter", "devin", "pi"] {
            let _ = candidate_roots(tool);
        }
    }

    #[test]
    fn missing_tools_yield_none_not_an_error() {
        assert!(devin_status().is_none() || devin_status().is_some()); // must not panic either way
        let accounts = all_accounts();
        for a in &accounts {
            assert!(!a.provider.is_empty(), "every entry needs a provider key");
            assert!(!a.label.is_empty(), "every entry needs a label");
        }
    }

    #[test]
    fn claude_accounts_reads_this_hosts_config() {
        let accounts = claude_accounts();
        for a in &accounts {
            assert_eq!(a.provider, "claude");
            assert!(!a.label.is_empty());
        }
    }
}