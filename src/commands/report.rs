use anyhow::{Context, Result};
use colored::Colorize;

use crate::cli::ReportArgs;
use crate::commands::churn::{Analysis, Stat, UNATTRIBUTED, analyze, sorted_agents};
use crate::repo::Repo;
use crate::store::Store;

/// `re report` — generate a shareable HTML dashboard of AI code-survival and
/// waste, ready to drop into a Slack message, a PR, or a board deck.
///
/// The whole report is a single self-contained HTML file (inline CSS, no
/// external assets, no network calls) so it stays true to Causari's
/// privacy-first promise: nothing leaves the machine.
pub fn run(args: ReportArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let analysis = match analyze(&repo, &store)? {
        Some(a) => a,
        None => {
            println!("{} no events recorded yet.", "report:".yellow().bold());
            return Ok(());
        }
    };

    let html = render_html(&analysis);
    let out = args
        .output
        .unwrap_or_else(|| "causari-report.html".to_string());
    std::fs::write(&out, html).with_context(|| format!("writing {}", out))?;

    println!(
        "{} report written to {}",
        "✓".green().bold(),
        out.cyan().bold()
    );
    if !analysis.has_cost {
        println!(
            "{} record `cost_usd`/`tokens_out` per event to unlock the wasted-spend figures.",
            "tip:".bright_black()
        );
    }

    if args.open {
        open_in_browser(&out);
    } else {
        println!("  open it with your browser, or re-run with --open");
    }
    Ok(())
}

fn waste_color(waste: f64) -> &'static str {
    if waste >= 0.40 {
        "#EF4444"
    } else if waste >= 0.20 {
        "#F59E0B"
    } else {
        "#22C55E"
    }
}

fn pct(x: f64) -> String {
    format!("{:.1}%", x * 100.0)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_html(a: &Analysis) -> String {
    let o = &a.overall;
    let waste = o.waste_rate();
    let color = waste_color(waste);

    let mut html = String::new();
    html.push_str(HEAD);

    // Hero header.
    html.push_str("<header class=\"hero\">");
    html.push_str("<div class=\"brand\">causari</div>");
    html.push_str("<h1>AI Code Waste Report</h1>");
    html.push_str(&format!(
        "<p class=\"sub\">{} events analyzed · code survival across the full ledger</p>",
        a.n_events
    ));
    html.push_str("</header>");

    // Headline cards.
    html.push_str("<section class=\"cards\">");
    if o.introduced == 0 {
        html.push_str(
            "<div class=\"card\"><div class=\"label\">No AI-attributed code</div>\
             <div class=\"hint\">Tag events with <code>re watch --agent &lt;name&gt;</code> to enable per-agent analysis.</div></div>",
        );
    } else {
        html.push_str(&format!(
            "<div class=\"card\"><div class=\"label\">AI Waste Score</div>\
             <div class=\"big\" style=\"color:{}\">{}</div>\
             <div class=\"bar\"><span style=\"width:{}; background:{}\"></span></div></div>",
            color,
            pct(waste),
            pct(waste),
            color
        ));
        html.push_str(&format!(
            "<div class=\"card\"><div class=\"label\">Code Survival</div>\
             <div class=\"big\" style=\"color:#22C55E\">{}</div>\
             <div class=\"hint\">{} of {} AI-written lines still alive</div></div>",
            pct(o.survival_rate()),
            o.surviving,
            o.introduced
        ));
        if o.has_cost {
            html.push_str(&format!(
                "<div class=\"card\"><div class=\"label\">Wasted Spend</div>\
                 <div class=\"big\" style=\"color:{}\">${:.2}</div>\
                 <div class=\"hint\">of ${:.2} spent on code that did not survive</div></div>",
                color, o.wasted_cost, o.cost
            ));
        }
    }
    html.push_str("</section>");

    // Agent leaderboard.
    html.push_str("<section class=\"panel\"><h2>Agent leaderboard</h2>");
    html.push_str("<table><thead><tr>");
    html.push_str("<th>Agent</th><th class=\"r\">Introduced</th><th class=\"r\">Survived</th>");
    html.push_str("<th class=\"r\">Survival</th><th class=\"w\">Waste</th>");
    if a.has_cost {
        html.push_str("<th class=\"r\">Cost $</th><th class=\"r\">Wasted $</th>");
    }
    html.push_str("</tr></thead><tbody>");

    for (agent, stat) in sorted_agents(&a.by_agent) {
        html.push_str(&render_row(agent, stat, a.has_cost));
    }
    html.push_str("</tbody></table></section>");

    html.push_str(
        "<footer>Generated locally by <a href=\"https://causari.dev\">Causari</a> — \
         your code and cost data never left this machine.</footer>",
    );
    html.push_str("</div></body></html>");
    html
}

fn render_row(agent: &str, stat: &Stat, has_cost: bool) -> String {
    let waste = stat.waste_rate();
    let color = waste_color(waste);
    let is_baseline = agent == UNATTRIBUTED;
    let name = if is_baseline {
        "baseline (pre-existing)".to_string()
    } else {
        esc(agent)
    };
    let name_class = if is_baseline { " class=\"muted\"" } else { "" };

    let mut row = String::new();
    row.push_str("<tr>");
    row.push_str(&format!("<td{}>{}</td>", name_class, name));
    row.push_str(&format!("<td class=\"r\">{}</td>", stat.introduced));
    row.push_str(&format!("<td class=\"r\">{}</td>", stat.surviving));
    row.push_str(&format!(
        "<td class=\"r\">{}</td>",
        pct(stat.survival_rate())
    ));
    row.push_str(&format!(
        "<td class=\"w\"><div class=\"minibar\"><span style=\"width:{}; background:{}\"></span></div>\
         <span class=\"wlabel\" style=\"color:{}\">{}</span></td>",
        pct(waste),
        color,
        color,
        pct(waste)
    ));
    if has_cost {
        row.push_str(&format!("<td class=\"r\">${:.2}</td>", stat.cost));
        row.push_str(&format!(
            "<td class=\"r\" style=\"color:{}\">${:.2}</td>",
            color, stat.wasted_cost
        ));
    }
    row.push_str("</tr>");
    row
}

fn open_in_browser(path: &str) {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", path])
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();

    if result.is_err() {
        println!(
            "{} could not open a browser automatically; open {} manually.",
            "note:".yellow().bold(),
            path
        );
    }
}

const HEAD: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Causari — AI Code Waste Report</title>
<style>
:root { --bg:#0B1437; --panel:#111c4e; --ink:#E8ECFF; --muted:#8a93c4; --line:#26326b; }
* { box-sizing:border-box; }
body { margin:0; background:var(--bg); color:var(--ink);
  font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif; }
.wrap { max-width:920px; margin:0 auto; padding:40px 24px 64px; }
.hero { text-align:center; padding:24px 0 8px; }
.brand { font-weight:800; letter-spacing:.18em; text-transform:uppercase;
  color:#7aa2ff; font-size:13px; }
.hero h1 { margin:8px 0 4px; font-size:34px; }
.sub { color:var(--muted); margin:0; font-size:14px; }
.cards { display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr));
  gap:16px; margin:28px 0; }
.card { background:var(--panel); border:1px solid var(--line); border-radius:16px;
  padding:20px; }
.label { color:var(--muted); font-size:13px; text-transform:uppercase; letter-spacing:.06em; }
.big { font-size:42px; font-weight:800; margin:6px 0 10px; }
.hint { color:var(--muted); font-size:13px; }
.bar { height:8px; background:#0a1233; border-radius:999px; overflow:hidden; }
.bar span { display:block; height:100%; border-radius:999px; }
.panel { background:var(--panel); border:1px solid var(--line); border-radius:16px;
  padding:8px 20px 20px; }
.panel h2 { font-size:18px; }
table { width:100%; border-collapse:collapse; font-size:14px; }
th,td { padding:12px 8px; border-bottom:1px solid var(--line); text-align:left; }
th.r,td.r { text-align:right; }
th.w { text-align:left; width:180px; }
.muted { color:var(--muted); }
.minibar { display:inline-block; vertical-align:middle; width:90px; height:8px;
  background:#0a1233; border-radius:999px; overflow:hidden; margin-right:8px; }
.minibar span { display:block; height:100%; }
.wlabel { font-variant-numeric:tabular-nums; font-weight:600; }
code { background:#0a1233; padding:2px 6px; border-radius:6px; font-size:12px; }
footer { text-align:center; color:var(--muted); font-size:12px; margin-top:28px; }
footer a { color:#7aa2ff; text-decoration:none; }
</style></head><body><div class="wrap">"##;
