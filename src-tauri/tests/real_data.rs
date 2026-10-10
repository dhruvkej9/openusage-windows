//! Proves the scanner against the real ~/.claude history on this host.
//!
//! Not a unit test: it reads whatever Claude Code actually wrote, so a schema change in
//! the jsonl (a renamed usage field, a new envelope type) fails here instead of silently
//! showing zero in the tray.

use openusage_lib::claude;

#[test]
fn scans_this_hosts_real_claude_history() {
    let report = claude::scan();

    assert!(
        !report.roots.is_empty(),
        "no Claude config root found — scanner is not finding ~/.claude"
    );
    assert!(
        report.scanned_files > 0,
        "found a root but scanned zero jsonl files"
    );
    assert!(
        report.total_requests > 0,
        "parsed files but found no usage entries"
    );
    assert!(
        report.total_cost_usd > 0.0,
        "usage found but priced at zero — rates or token names are wrong"
    );

    let top = &report.models[0];
    assert!(
        top.tokens.total() > 0,
        "model {} has requests but zero tokens",
        top.model
    );

    println!("roots:        {:?}", report.roots);
    println!("files:        {}", report.scanned_files);
    println!("requests:     {}", report.total_requests);
    println!("tokens:       {}", report.total_tokens);
    println!("cost:         ${:.4}", report.total_cost_usd);
    for m in &report.models {
        println!(
            "  {:<26} {:>4} req  {:>12} tok  ${:.4}",
            m.model,
            m.requests,
            m.tokens.total(),
            m.cost_usd
        );
    }
}

#[test]
fn models_are_sorted_by_spend() {
    let report = claude::scan();
    for pair in report.models.windows(2) {
        assert!(
            pair[0].cost_usd >= pair[1].cost_usd,
            "models are not ordered by spend: {} then {}",
            pair[0].model,
            pair[1].model
        );
    }
}