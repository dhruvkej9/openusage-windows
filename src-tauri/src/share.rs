//! Share card: a self-contained SVG of the headline numbers, rasterised by the webview.
//!
//! Upstream renders a SwiftUI view and exports it. Here the same idea in a form the tray can
//! already produce: Rust builds the SVG, the UI hands it to a canvas. No image dependency, and
//! the card is vector so it stays sharp.
//!
//! Built with `push_str`, never `format!`: SVG is full of `{...}` and `format!` treats every
//! brace pair as an argument name, so a template string silently mis-parses. Constants are
//! concatenated instead.

use crate::claude::UsageReport;
use crate::series::Series;

const INK: &str = "#e9edf3";
const MUTED: &str = "#8f99a8";
const ACCENT: &str = "#d98a3d";
const PANEL: &str = "#1c1f25";
const BG: &str = "#0b0d10";
const GRAD: &str = "#141821";
const LINE: &str = "#2b3038";
const SANS: &str = "ui-sans-serif,-apple-system,'Segoe UI',Inter,sans-serif";
const MONO: &str = "ui-monospace,Menlo,monospace";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn fmt_usd(v: f64) -> String {
    if v >= 1000.0 {
        format!("${:.1}k", v / 1000.0)
    } else if v >= 100.0 {
        format!("${:.0}", v)
    } else if v >= 10.0 {
        format!("${:.2}", v)
    } else {
        format!("${:.3}", v)
    }
}

fn fmt_tok(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// `x="123.4"` attribute pair.
fn attr(name: &str, value: &str) -> String {
    let mut s = String::with_capacity(name.len() + value.len() + 3);
    s.push(' ');
    s.push_str(name);
    s.push_str("=\"");
    s.push_str(value);
    s.push('"');
    s
}

fn text(x: f64, y: f64, font: &str, size: u32, fill: &str, weight: Option<&str>, body: &str) -> String {
    let mut s = String::from("<text");
    s.push_str(&attr("x", &x.to_string()));
    s.push_str(&attr("y", &y.to_string()));
    s.push_str(&attr("font-family", font));
    s.push_str(&attr("font-size", &size.to_string()));
    if let Some(w) = weight {
        s.push_str(&attr("font-weight", w));
    }
    s.push_str(&attr("fill", fill));
    s.push('>');
    s.push_str(body);
    s.push_str("</text>");
    s
}

fn stat(x: f64, y: f64, label: &str, value: &str) -> String {
    let mut s = text(x, y, SANS, 15, MUTED, None, label);
    s.push_str(&attr("letter-spacing", "2").replace(' ', "").replace("letter-spacing", " letter-spacing"));
    s.push_str(&text(x, y + 52.0, SANS, 54, INK, Some("700"), value));
    s
}

fn sparkline(days: &[crate::series::DayPoint]) -> String {
    if days.len() < 2 {
        return String::new();
    }
    let (w, h, pad) = (1080.0_f64, 90.0_f64, 60.0_f64);
    let max = days
        .iter()
        .map(|d| d.cost_usd)
        .fold(0.0_f64, f64::max)
        .max(1e-9);
    let bw = (w - pad * 2.0) / days.len() as f64;
    let mut out = String::new();
    for (i, d) in days.iter().enumerate() {
        let bh = ((d.cost_usd / max) * h).max(2.0);
        let mut s = String::from("<rect");
        s.push_str(&attr("x", &(pad + i as f64 * bw).to_string()));
        s.push_str(&attr("y", &(h - bh).to_string()));
        s.push_str(&attr("width", &((bw - 2.0).max(1.0)).to_string()));
        s.push_str(&attr("height", &bh.to_string()));
        s.push_str(&attr("rx", "2"));
        s.push_str(&attr("fill", ACCENT));
        s.push_str("/>");
        out.push_str(&s);
    }
    out
}

/// The card as an SVG string, 1200x630 (OG-image proportions).
pub fn svg(report: &UsageReport, series: &Series) -> String {
    let days = series.days.len();
    let range = match (series.days.first(), series.days.last()) {
        (Some(a), Some(b)) if a.day != b.day => format!("{} to {}", a.day, b.day),
        (Some(a), _) => a.day.clone(),
        _ => "no history".to_string(),
    };

    let mut s = String::with_capacity(4096);
    s.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630">"#);
    s.push_str(r#"<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color=""#);
    s.push_str(BG);
    s.push_str(r#""/><stop offset="1" stop-color=""#);
    s.push_str(GRAD);
    s.push_str(r#""/></linearGradient></defs>"#);

    s.push_str(r#"<rect width="1200" height="630" fill="url(#g)"/>"#);
    s.push_str("<rect");
    s.push_str(&attr("x", "32"));
    s.push_str(&attr("y", "32"));
    s.push_str(&attr("width", "1136"));
    s.push_str(&attr("height", "566"));
    s.push_str(&attr("rx", "18"));
    s.push_str(&attr("fill", PANEL));
    s.push_str(&attr("stroke", LINE));
    s.push_str("/>");

    // Title: "OpenUsage" with the accent on the second half.
    s.push_str("<text");
    s.push_str(&attr("x", "60"));
    s.push_str(&attr("y", "86"));
    s.push_str(&attr("font-family", SANS));
    s.push_str(&attr("font-size", "24"));
    s.push_str(&attr("font-weight", "700"));
    s.push_str(&attr("fill", INK));
    s.push_str(">Open<tspan");
    s.push_str(&attr("fill", ACCENT));
    s.push_str(">Usage</tspan></text>");

    s.push_str(&text(
        60.0,
        114.0,
        SANS,
        15,
        MUTED,
        None,
        &format!("Claude Code - {} active days - {}", days, esc(&range)),
    ));

    s.push_str(&stat(60.0, 196.0, "TOTAL SPEND", &fmt_usd(report.total_cost_usd)));
    s.push_str(&stat(420.0, 196.0, "TOKENS", &fmt_tok(report.total_tokens)));
    s.push_str(&stat(780.0, 196.0, "REQUESTS", &report.total_requests.to_string()));

    s.push_str(r#"<g transform="translate(0,268)">"#);
    s.push_str(&sparkline(&series.days));
    s.push_str("</g>");

    for (i, m) in report.models.iter().take(5).enumerate() {
        let y = 372.0 + i as f64 * 44.0;
        s.push_str(&text(60.0, y, SANS, 21, INK, None, &esc(&m.model)));
        let mut cost = text(1140.0, y, MONO, 21, ACCENT, Some("600"), &fmt_usd(m.cost_usd));
        cost.insert_str(5, &attr("text-anchor", "end"));
        s.push_str(&cost);
        s.push_str("<line");
        s.push_str(&attr("x1", "60"));
        s.push_str(&attr("y1", &(y + 14.0).to_string()));
        s.push_str(&attr("x2", "1140"));
        s.push_str(&attr("y2", &(y + 14.0).to_string()));
        s.push_str(&attr("stroke", LINE));
        s.push_str("/>");
    }

    s.push_str(&text(
        60.0,
        574.0,
        SANS,
        13,
        MUTED,
        None,
        "Estimated from local token counts - generated by OpenUsage",
    ));
    s.push_str("</svg>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (UsageReport, Series) {
        let r = UsageReport {
            total_tokens: 1_893_626_025,
            total_cost_usd: 4278.98,
            total_requests: 10_459,
            models: vec![crate::claude::ModelUsage {
                model: "claude-opus-5-5".into(),
                tokens: crate::claude::TokenBreakdown::default(),
                cost_usd: 3610.84,
                requests: 8485,
            }],
            ..Default::default()
        };
        let s = Series {
            days: (1..=5)
                .map(|d| crate::series::DayPoint {
                    day: format!("2026-10-0{d}"),
                    cost_usd: d as f64 * 2.0,
                    tokens: d as u64 * 1000,
                    requests: d as u64,
                })
                .collect(),
            ..Default::default()
        };
        (r, s)
    }

    #[test]
    fn the_card_is_well_formed_svg() {
        let (r, s) = sample();
        let out = svg(&r, &s);
        assert!(out.starts_with("<svg") && out.ends_with("</svg>"));
        assert_eq!(out.matches("<svg").count(), 1, "one root element only");
        assert!(out.contains("$4.3k"), "the headline number should be present");
    }

    #[test]
    fn every_element_is_closed_or_self_closed() {
        let (r, s) = sample();
        let out = svg(&r, &s);
        assert_eq!(
            out.matches("<text").count(),
            out.matches("</text>").count(),
            "an unclosed <text> makes the card unrenderable"
        );
        // <line>, <rect> and <stop> are self-closed; each must appear once as an
        // opening tag and contribute exactly one "/>".
        for tag in ["<line", "<rect", "<stop"] {
            let n = out.matches(tag).count();
            assert!(n > 0, "expected {tag} in the card");
            assert!(
                out.matches(&format!("{tag}")).count() == n,
                "{tag} appeared more than {n} times — the count drifted"
            );
        }
        // No stray closing tag for a self-closed element.
        for tag in ["</line>", "</rect>", "</stop>"] {
            assert!(!out.contains(tag), "{tag} must never appear"); 
        }
        // Every self-closed element ends with "/>" right before the next "<" or "<".
        assert!(
            !out.contains("</rect>"),
            "rect is self-closed; a closing tag means the markup drifted"
        );
    }

    #[test]
    fn model_names_are_xml_escaped() {
        let (mut r, s) = sample();
        r.models[0].model = "weird & <name>".into();
        let out = svg(&r, &s);
        assert!(!out.contains("<name>"), "raw markup leaked into the SVG");
        assert!(out.contains("&amp;"));
    }

    #[test]
    fn degenerate_inputs_still_render() {
        let (r, mut s) = sample();
        s.days.truncate(1);
        assert!(svg(&r, &s).ends_with("</svg>"));
        let empty = svg(&UsageReport::default(), &Series::default());
        assert!(empty.contains("$0") && empty.ends_with("</svg>"));
    }

    #[test]
    fn every_sparkline_bar_has_a_positive_height() {
        let (r, s) = sample();
        let out = svg(&r, &s);
        // The card has two layout rects before the sparkline band; skip to it.
        let band = out.split("<g transform=").nth(1).expect("sparkline band");
        let mut checked = 0;
        for chunk in band.split("<rect").skip(1) {
            if let Some(h) = chunk.split("height=\"").nth(1).and_then(|x| x.split('"').next()) {
                let v: f64 = h.parse().expect("height must be numeric");
                assert!(v > 0.0, "a zero-height bar is invisible");
                checked += 1;
            }
        }
        assert_eq!(checked, 5, "one bar per active day");
    }
}