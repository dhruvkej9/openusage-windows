//! Per-day usage series and rate-limit snapshots, read from the same Claude Code jsonl.
//!
//! Upstream shows a sparkline per row, a usage-trend detail popover, and rate-limit reset
//! countdowns. All three need data the jsonl already carries, so they are reconstructed here
//! rather than pulled from a separate API.

use std::collections::HashMap;

use serde::Deserialize;

use crate::claude::{ModelUsage, TokenBreakdown};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayPoint {
    /// `YYYY-MM-DD` in local time.
    pub day: String,
    pub cost_usd: f64,
    pub tokens: u64,
    pub requests: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub name: String,
    /// 0.0–1.0, or None when the build did not report one.
    pub utilization: Option<f64>,
    /// Minutes until reset; None when unknown.
    pub resets_in_minutes: Option<i64>,
    pub window: Option<String>,
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub days: Vec<DayPoint>,
    pub by_model_per_day: HashMap<String, Vec<DayPoint>>,
    pub rate_limits: Vec<RateLimit>,
}

#[derive(Deserialize, Default)]
struct RawLimits {
    #[serde(default)]
    rate_limit: Option<String>,
}

#[derive(Deserialize)]
struct RawEntryFull {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    message: Option<RawMessageFull>,
}

#[derive(Deserialize)]
struct RawMessageFull {
    model: Option<String>,
    usage: Option<RawTokens>,
    #[serde(rename = "rateLimitInfo", default)]
    rate_limit_info: Option<RawRateLimitInfo>,
}

#[derive(Deserialize)]
struct RawTokens {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct RawRateLimitInfo {
    #[serde(default)]
    five_hour: Option<RawWindow>,
    #[serde(default)]
    seven_day: Option<RawWindow>,
    #[serde(rename = "seven_day_opus", default)]
    seven_day_opus: Option<RawWindow>,
}

#[derive(Deserialize)]
struct RawWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

fn cost_of(model: &str, t: &TokenBreakdown) -> f64 {
    // Same rates as claude.rs; the shared version there is private, so this mirrors it.
    let m = model.to_ascii_lowercase();
    let (input, output) = if m.contains("opus") {
        (15.0, 75.0)
    } else if m.contains("haiku") {
        (1.0, 5.0)
    } else {
        (3.0, 15.0)
    };
    (t.input as f64 * input
        + t.output as f64 * output
        + t.cache_read as f64 * input * 0.1
        + t.cache_write as f64 * input * 1.25)
        / 1_000_000.0
}

fn day_of(ts: &str) -> Option<String> {
    // "2026-09-03T12:46:33.558Z" -> "2026-09-03" (UTC; local would need chrono-tz for a
    // stable answer across zones, and a day boundary off by hours does not change a sparkline).
    let d = ts.get(..10)?;
    if d.len() != 10 || !d.contains('-') {
        return None;
    }
    // Shape alone is not enough: "2026-13-99" has the right length but is not a date, and
    // a bogus day key would silently become its own bar on the sparkline.
    let (y, mo, day) = (d.get(..4)?, d.get(5..7)?, d.get(8..10)?);
    let (Ok(y), Ok(mo), Ok(day)) = (y.parse::<u32>(), mo.parse::<u32>(), day.parse::<u32>()) else {
        return None;
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&day) || y < 1970 {
        return None;
    }
    Some(d.to_string())
}

fn minutes_until(ts: &str) -> Option<i64> {
    // RFC3339 with an optional fractional part and Z. Hand-parsed so this stays
    // dependency-free; returns None on anything it does not understand.
    let bytes = ts.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let num = |a: usize, b: usize| -> Option<i64> { ts.get(a..b)?.parse().ok() };
    let (y, mo, d, h, mi) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    // Days since epoch for the date part (Howard Hinnant's civil-from-days, inverted).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + h * 3600 + mi * 60;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((secs - now) / 60)
}

pub fn build(files: &[std::path::PathBuf]) -> Series {
    let mut days: HashMap<String, (f64, u64, u64)> = HashMap::new();
    let mut per_model: HashMap<String, HashMap<String, (f64, u64, u64)>> = HashMap::new();
    let mut latest_limits: HashMap<String, RateLimit> = HashMap::new();

    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines() {
            let Ok(e) = serde_json::from_str::<RawEntryFull>(line) else {
                continue;
            };
            let Some(msg) = e.message else { continue };

            // Rate limits: keep the newest reading per window.
            if let Some(info) = &msg.rate_limit_info {
                for (name, w, window) in [
                    ("5-hour", &info.five_hour, "5h"),
                    ("7-day", &info.seven_day, "7d"),
                    ("7-day Opus", &info.seven_day_opus, "7d-opus"),
                ] {
                    if let Some(w) = w {
                        latest_limits.insert(
                            name.to_string(),
                            RateLimit {
                                name: name.to_string(),
                                utilization: w.utilization,
                                resets_in_minutes: w.resets_at.as_deref().and_then(minutes_until),
                                window: Some(window.to_string()),
                            },
                        );
                    }
                }
            }

            let (Some(ts), Some(usage)) = (e.timestamp.as_deref(), &msg.usage) else {
                continue;
            };
            let Some(day) = day_of(ts) else { continue };
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
            let cost = cost_of(&model, &tokens);
            let total = tokens.total();

            let slot = days.entry(day.clone()).or_insert((0.0, 0, 0));
            slot.0 += cost;
            slot.1 += total;
            slot.2 += 1;

            let ms = per_model.entry(model).or_default();
            let mslot = ms.entry(day.clone()).or_insert((0.0, 0, 0));
            mslot.0 += cost;
            mslot.1 += total;
            mslot.2 += 1;
        }
    }

    let to_points = |m: HashMap<String, (f64, u64, u64)>| -> Vec<DayPoint> {
        let mut v: Vec<DayPoint> = m
            .into_iter()
            .map(|(day, (c, t, r))| DayPoint { day, cost_usd: c, tokens: t, requests: r })
            .collect();
        v.sort_by(|a, b| a.day.cmp(&b.day));
        v
    };

    let mut limits: Vec<RateLimit> = latest_limits.into_values().collect();
    limits.sort_by(|a, b| a.name.cmp(&b.name));

    Series {
        days: to_points(days),
        by_model_per_day: per_model.into_iter().map(|(k, v)| (k, to_points(v))).collect(),
        rate_limits: limits,
    }
}

/// Sparkline geometry for the UI: one normalised bar per day, oldest first.
pub fn spark(values: &[f64]) -> Vec<f64> {
    let max = values.iter().cloned().fold(0.0_f64, f64::max).max(1e-9);
    values.iter().map(|v| (v / max).clamp(0.0, 1.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_is_parsed_from_an_rfc3339_timestamp() {
        assert_eq!(day_of("2026-09-03T12:46:33.558Z").unwrap(), "2026-09-03");
        assert_eq!(day_of("2026-09-03T00:00:00Z").unwrap(), "2026-09-03");
    }

    #[test]
    fn bad_timestamps_yield_none_not_a_panic() {
        for s in ["", "nope", "2026-13-99", "20260903"] {
            assert_eq!(day_of(s), None, "day_of({s:?}) should be None");
        }
    }

    #[test]
    fn minutes_until_understands_a_future_and_a_past_reset() {
        // A reset far in the future is positive, an old one negative.
        assert!(minutes_until("2099-01-01T00:00:00Z").unwrap() > 0);
        assert!(minutes_until("2000-01-01T00:00:00Z").unwrap() < 0);
        assert_eq!(minutes_until("garbage"), None);
    }

    #[test]
    fn sparkline_is_normalised_to_the_peak() {
        let bars = spark(&[1.0, 2.0, 4.0]);
        assert_eq!(bars[2], 1.0, "the peak must be full height");
        assert_eq!(bars[0], 0.25);
        assert_eq!(bars[1], 0.5);
    }

    #[test]
    fn an_all_zero_series_does_not_divide_by_zero() {
        for b in spark(&[0.0, 0.0]) {
            assert!(b.is_finite());
        }
    }

    #[test]
    fn builds_a_series_from_real_history() {
        let mut files = Vec::new();
        crate::claude::config_roots();
        for (label, root) in crate::claude::config_roots() {
            if label == "Windows" || label.starts_with("WSL") || label == "CLAUDE_CONFIG_DIR" {
                let projects = root.join("projects");
                let mut found = Vec::new();
                walk(&projects, &mut found);
                files.extend(found);
            }
        }
        if files.is_empty() {
            return; // no history on this host; nothing to prove
        }
        let series = build(&files);
        assert!(!series.days.is_empty(), "no days built from real history");
        assert!(
            series.days.windows(2).all(|w| w[0].day <= w[1].day),
            "days must be chronological for a sparkline"
        );
    }

    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "jsonl") {
                out.push(p);
            }
        }
    }
}