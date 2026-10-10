//! Proves the day-series and rate-limit reader against real Claude Code history.

use openusage_lib::{claude, series};

#[test]
fn real_history_produces_a_usable_series() {
    let files = claude::all_usage_files();
    if files.is_empty() {
        return; // no history on this host
    }
    let s = series::build(&files);

    assert!(!s.days.is_empty(), "no days built from real history");
    assert!(
        s.days.windows(2).all(|w| w[0].day <= w[1].day),
        "days must be chronological or the sparkline reads backwards"
    );
    assert!(
        s.days.iter().any(|d| d.cost_usd > 0.0),
        "every day priced at zero — the cost formula is wrong"
    );
    assert!(
        s.days.iter().all(|d| d.requests > 0),
        "a day with requests == 0 should not exist"
    );
    assert!(
        s.days.iter().any(|d| d.tokens > 0),
        "tokens are zero across every day"
    );

    println!("active days: {}", s.days.len());
    println!("models tracked: {}", s.by_model_per_day.len());
    println!("rate-limit windows: {}", s.rate_limits.len());
    for l in &s.rate_limits {
        println!(
            "  {:<12} util={:?} resets_in={:?}",
            l.name, l.utilization, l.resets_in_minutes
        );
    }
    let last = s.days.last().unwrap();
    println!(
        "most recent: {} ${:.4} {} tokens {} req",
        last.day, last.cost_usd, last.tokens, last.requests
    );
}

#[test]
fn every_tracked_model_has_a_chronological_series() {
    let files = claude::all_usage_files();
    if files.is_empty() {
        return;
    }
    let s = series::build(&files);
    for (model, days) in &s.by_model_per_day {
        assert!(!days.is_empty(), "{model} has an empty series");
        assert!(
            days.windows(2).all(|w| w[0].day <= w[1].day),
            "{model} days are out of order"
        );
    }
}