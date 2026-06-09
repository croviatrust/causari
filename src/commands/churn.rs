use anyhow::Result;
use colored::Colorize;
use similar::{ChangeTag, TextDiff};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::cli::ChurnArgs;
use crate::object::Event;
use crate::repo::Repo;
use crate::snapshot::flatten_tree;
use crate::store::Store;

/// Per-agent survival accounting.
#[derive(Default, Clone)]
pub(crate) struct Stat {
    pub introduced: u64,
    pub surviving: u64,
    pub events: u64,
    pub cost: f64,
    pub wasted_cost: f64,
    pub has_cost: bool,
}

impl Stat {
    pub(crate) fn survival_rate(&self) -> f64 {
        if self.introduced == 0 {
            0.0
        } else {
            self.surviving as f64 / self.introduced as f64
        }
    }
    pub(crate) fn waste_rate(&self) -> f64 {
        if self.introduced == 0 {
            0.0
        } else {
            1.0 - self.survival_rate()
        }
    }
}

/// Result of a full-history survival analysis, ready for any renderer
/// (terminal, Markdown summary, or HTML report).
pub(crate) struct Analysis {
    pub by_agent: BTreeMap<String, Stat>,
    pub overall: Stat,
    pub has_cost: bool,
    pub n_events: usize,
}

pub(crate) const UNATTRIBUTED: &str = "unattributed";

/// Agents sorted by lines introduced (desc), with the unattributed baseline last.
pub(crate) fn sorted_agents(by_agent: &BTreeMap<String, Stat>) -> Vec<(&String, &Stat)> {
    let mut rows: Vec<(&String, &Stat)> = by_agent.iter().collect();
    rows.sort_by(|a, b| {
        let a_un = a.0 == UNATTRIBUTED;
        let b_un = b.0 == UNATTRIBUTED;
        a_un.cmp(&b_un)
            .then_with(|| b.1.introduced.cmp(&a.1.introduced))
    });
    rows
}

/// Caches flattened snapshot trees and decoded blobs so a full-history replay
/// touches each unique snapshot/blob only once.
struct Reconstructor<'a> {
    store: &'a Store<'a>,
    tree_cache: HashMap<String, BTreeMap<PathBuf, String>>,
    blob_cache: HashMap<String, Option<String>>,
}

impl<'a> Reconstructor<'a> {
    fn new(store: &'a Store<'a>) -> Self {
        Self {
            store,
            tree_cache: HashMap::new(),
            blob_cache: HashMap::new(),
        }
    }

    fn flat(&mut self, snap_id: &str) -> Result<&BTreeMap<PathBuf, String>> {
        if !self.tree_cache.contains_key(snap_id) {
            let snap = self.store.read_snapshot(snap_id)?;
            let flat = flatten_tree(self.store, &snap.tree)?;
            self.tree_cache.insert(snap_id.to_string(), flat);
        }
        Ok(self.tree_cache.get(snap_id).unwrap())
    }

    /// Decode a file's UTF-8 content at a snapshot. Returns None when the file
    /// is absent or not valid UTF-8 (binary files are intentionally skipped).
    fn file_at(&mut self, snap_id: &str, rel: &Path) -> Result<Option<String>> {
        let blob_id = self.flat(snap_id)?.get(rel).cloned();
        let Some(bid) = blob_id else {
            return Ok(None);
        };
        if let Some(cached) = self.blob_cache.get(&bid) {
            return Ok(cached.clone());
        }
        let bytes = self.store.read_blob(&bid)?;
        let decoded = String::from_utf8(bytes).ok();
        self.blob_cache.insert(bid, decoded.clone());
        Ok(decoded)
    }
}

/// `re churn` — code survival / AI-waste analysis.
///
/// For every recorded event, Causari knows the exact lines it introduced (from
/// the pre→post snapshot diff). By replaying the whole history we can tell how
/// many of those lines are still alive in the latest recorded state versus how
/// many were later rewritten or deleted. Aggregated per agent, this answers the
/// question every AI-spend owner is now asking:
///
///   "How much of what we paid the model to write actually survived?"
pub fn run(args: ChurnArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let analysis = match analyze(&repo, &store)? {
        Some(a) => a,
        None => {
            println!("{} no events recorded yet.", "churn:".yellow().bold());
            return Ok(());
        }
    };

    if args.summary {
        print_summary(&analysis.by_agent, &analysis.overall, analysis.has_cost);
    } else {
        print_terminal(
            &analysis.by_agent,
            &analysis.overall,
            analysis.has_cost,
            analysis.n_events,
        );
    }

    Ok(())
}

/// Run the full-history survival analysis. Returns `None` when the ledger has
/// no events yet. Shared by `re churn` and `re report`.
pub(crate) fn analyze(repo: &Repo, store: &Store) -> Result<Option<Analysis>> {
    // Oldest -> newest chain.
    let mut chain: Vec<(String, Event)> = Vec::new();
    let mut cur = repo.head_event()?;
    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        let parent = ev.parent.clone();
        chain.push((id, ev));
        cur = parent;
    }
    chain.reverse();

    if chain.is_empty() {
        return Ok(None);
    }

    let mut recon = Reconstructor::new(store);

    // Every file that ever existed across the recorded history.
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    for (_, ev) in &chain {
        for path in recon.flat(&ev.post_snapshot)?.keys() {
            files.insert(path.clone());
        }
    }

    let mut introduced: HashMap<String, u64> = HashMap::new();
    let mut surviving: HashMap<String, u64> = HashMap::new();

    for file in &files {
        let owners = replay_file(&mut recon, &chain, file, &mut introduced)?;
        for owner in owners.iter().flatten() {
            *surviving.entry(owner.clone()).or_default() += 1;
        }
    }

    // Aggregate per agent.
    let mut by_agent: BTreeMap<String, Stat> = BTreeMap::new();
    for (id, ev) in &chain {
        let intro = introduced.get(id).copied().unwrap_or(0);
        let surv = surviving.get(id).copied().unwrap_or(0);
        let agent = ev.agent.clone().unwrap_or_else(|| UNATTRIBUTED.to_string());
        let entry = by_agent.entry(agent).or_default();
        entry.introduced += intro;
        entry.surviving += surv;
        entry.events += 1;
        if let Some(cost) = ev.cost_usd {
            entry.has_cost = true;
            entry.cost += cost;
            let waste_rate = if intro > 0 {
                1.0 - (surv as f64 / intro as f64)
            } else {
                0.0
            };
            entry.wasted_cost += cost * waste_rate;
        }
    }

    // Headline = AI-attributed agents only (exclude the pre-existing baseline).
    let mut overall = Stat::default();
    for (agent, stat) in &by_agent {
        if agent == UNATTRIBUTED {
            continue;
        }
        overall.introduced += stat.introduced;
        overall.surviving += stat.surviving;
        overall.cost += stat.cost;
        overall.wasted_cost += stat.wasted_cost;
        overall.has_cost |= stat.has_cost;
    }

    let has_cost = by_agent.values().any(|s| s.has_cost);

    Ok(Some(Analysis {
        by_agent,
        overall,
        has_cost,
        n_events: chain.len(),
    }))
}

/// Replay a single file across the whole chain, returning the final owner of
/// each surviving line and accumulating per-event introduced-line counts.
fn replay_file(
    recon: &mut Reconstructor,
    chain: &[(String, Event)],
    file: &Path,
    introduced: &mut HashMap<String, u64>,
) -> Result<Vec<Option<String>>> {
    let mut owners: Vec<Option<String>> = Vec::new();
    let mut prev_content = String::new();

    for (id, ev) in chain {
        let post_content = match recon.file_at(&ev.post_snapshot, file)? {
            Some(c) => c,
            None => {
                // File absent (deleted, or binary): its lines no longer survive.
                owners.clear();
                prev_content.clear();
                continue;
            }
        };

        // Root event: pre_snapshot == post_snapshot, so claim every line as the
        // baseline that existed when tracking began.
        if ev.parent.is_none() {
            let n = post_content.lines().count();
            owners = vec![Some(id.clone()); n];
            *introduced.entry(id.clone()).or_default() += n as u64;
            prev_content = post_content;
            continue;
        }

        let pre_content = recon.file_at(&ev.pre_snapshot, file)?.unwrap_or_default();
        if pre_content == post_content {
            continue; // event did not touch this file
        }

        let (new_owners, inserts) =
            replay_diff(&owners, &pre_content, &post_content, id, &prev_content);
        *introduced.entry(id.clone()).or_default() += inserts;
        owners = new_owners;
        prev_content = post_content;
    }

    Ok(owners)
}

/// Replay one event's pre→post diff over the existing owner map, returning the
/// new owner map and the number of lines inserted (introduced) by this event.
fn replay_diff(
    prev_owners: &[Option<String>],
    pre_content: &str,
    post_content: &str,
    event_id: &str,
    last_known_content: &str,
) -> (Vec<Option<String>>, u64) {
    let pre_lines: Vec<&str> = pre_content.lines().collect();
    let last_lines: Vec<&str> = last_known_content.lines().collect();

    let mut working: Vec<Option<String>> = vec![None; pre_lines.len()];
    if last_lines == pre_lines && prev_owners.len() == pre_lines.len() {
        working.clone_from(&prev_owners.to_vec());
    } else {
        for (i, line) in pre_lines.iter().enumerate() {
            if let Some(idx) = last_lines.iter().position(|l| l == line) {
                if let Some(o) = prev_owners.get(idx).cloned() {
                    working[i] = o;
                }
            }
        }
    }

    let diff = TextDiff::from_lines(pre_content, post_content);
    let mut result: Vec<Option<String>> = Vec::new();
    let mut inserts: u64 = 0;
    let mut pre_idx: usize = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                let owner = working.get(pre_idx).cloned().unwrap_or(None);
                result.push(owner);
                pre_idx += 1;
            }
            ChangeTag::Delete => {
                pre_idx += 1;
            }
            ChangeTag::Insert => {
                result.push(Some(event_id.to_string()));
                inserts += 1;
            }
        }
    }
    (result, inserts)
}

fn status_label(waste: f64) -> colored::ColoredString {
    if waste >= 0.40 {
        format!("{:.1}%", waste * 100.0).red().bold()
    } else if waste >= 0.20 {
        format!("{:.1}%", waste * 100.0).yellow().bold()
    } else {
        format!("{:.1}%", waste * 100.0).green().bold()
    }
}

fn print_terminal(
    by_agent: &BTreeMap<String, Stat>,
    overall: &Stat,
    has_cost: bool,
    n_events: usize,
) {
    println!(
        "{} code survival across {} events",
        "causari churn:".green().bold(),
        n_events
    );
    println!();

    // Header
    if has_cost {
        println!(
            "  {:<20} {:>10} {:>10} {:>9} {:>8} {:>10} {:>10}",
            "AGENT".bold(),
            "INTRO".bold(),
            "SURVIVED".bold(),
            "SURVIVAL".bold(),
            "WASTE".bold(),
            "COST $".bold(),
            "WASTED $".bold()
        );
    } else {
        println!(
            "  {:<20} {:>10} {:>10} {:>9} {:>8}",
            "AGENT".bold(),
            "INTRO".bold(),
            "SURVIVED".bold(),
            "SURVIVAL".bold(),
            "WASTE".bold()
        );
    }

    for (agent, stat) in sorted_agents(by_agent) {
        let survival = format!("{:.1}%", stat.survival_rate() * 100.0);
        if has_cost {
            println!(
                "  {:<20} {:>10} {:>10} {:>9} {:>8} {:>10} {:>10}",
                truncate(agent, 20),
                stat.introduced,
                stat.surviving,
                survival.cyan(),
                status_label(stat.waste_rate()),
                format!("{:.2}", stat.cost),
                format!("{:.2}", stat.wasted_cost).red()
            );
        } else {
            println!(
                "  {:<20} {:>10} {:>10} {:>9} {:>8}",
                truncate(agent, 20),
                stat.introduced,
                stat.surviving,
                survival.cyan(),
                status_label(stat.waste_rate())
            );
        }
    }

    println!();
    if overall.introduced == 0 {
        println!(
            "{} no AI-attributed code found. Tag events with `re watch --agent <name>` to enable per-agent analysis.",
            "note:".yellow().bold()
        );
        return;
    }

    println!(
        "{} {}   {} {}",
        "AI survival:".bold(),
        format!("{:.1}%", overall.survival_rate() * 100.0)
            .green()
            .bold(),
        "AI Waste Score:".bold(),
        status_label(overall.waste_rate())
    );
    if overall.has_cost {
        println!(
            "{} ${:.2} of ${:.2} spent on code that did not survive",
            "wasted spend:".bold(),
            overall.wasted_cost,
            overall.cost
        );
    } else {
        println!(
            "{} record `cost_usd`/`tokens_out` per event to see wasted spend in dollars.",
            "tip:".bright_black()
        );
    }
}

fn print_summary(by_agent: &BTreeMap<String, Stat>, overall: &Stat, has_cost: bool) {
    let waste = overall.waste_rate();
    let status = if overall.introduced == 0 {
        "ℹ️ no AI-attributed code"
    } else if waste >= 0.40 {
        "🔴 high waste"
    } else if waste >= 0.20 {
        "🟡 moderate waste"
    } else {
        "🟢 healthy"
    };

    println!("## Causari Churn — {}", status);
    println!();
    if overall.introduced > 0 {
        println!(
            "**AI survival: {:.1}%** · **AI Waste Score: {:.1}%**",
            overall.survival_rate() * 100.0,
            waste * 100.0
        );
        if overall.has_cost {
            println!();
            println!(
                "💸 **${:.2}** of **${:.2}** spent on code that did not survive.",
                overall.wasted_cost, overall.cost
            );
        }
        println!();
    }

    if has_cost {
        println!("| Agent | Introduced | Survived | Survival | Waste | Cost $ | Wasted $ |");
        println!("|---|---:|---:|---:|---:|---:|---:|");
    } else {
        println!("| Agent | Introduced | Survived | Survival | Waste |");
        println!("|---|---:|---:|---:|---:|");
    }

    for (agent, stat) in sorted_agents(by_agent) {
        if has_cost {
            println!(
                "| {} | {} | {} | {:.1}% | {:.1}% | {:.2} | {:.2} |",
                agent,
                stat.introduced,
                stat.surviving,
                stat.survival_rate() * 100.0,
                stat.waste_rate() * 100.0,
                stat.cost,
                stat.wasted_cost
            );
        } else {
            println!(
                "| {} | {} | {} | {:.1}% | {:.1}% |",
                agent,
                stat.introduced,
                stat.surviving,
                stat.survival_rate() * 100.0,
                stat.waste_rate() * 100.0
            );
        }
    }
    println!();
    println!("<sub>Powered by [Causari](https://causari.dev)</sub>");
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_pure_insertions() {
        // Adding two lines to an empty file => 2 introduced, both owned by E1.
        let (owners, inserts) = replay_diff(&[], "", "a\nb\n", "E1", "");
        assert_eq!(inserts, 2);
        assert_eq!(owners, vec![Some("E1".to_string()), Some("E1".to_string())]);
    }

    #[test]
    fn overwriting_a_line_reattributes_ownership() {
        // E1 owns two lines; E2 rewrites the second one.
        let prev = vec![Some("E1".to_string()), Some("E1".to_string())];
        let (owners, inserts) = replay_diff(&prev, "a\nb\n", "a\nc\n", "E2", "a\nb\n");
        assert_eq!(inserts, 1); // one new line introduced by E2
        assert_eq!(owners[0], Some("E1".to_string())); // unchanged line keeps E1
        assert_eq!(owners[1], Some("E2".to_string())); // rewritten line now E2
    }

    #[test]
    fn deletion_drops_lines_without_new_inserts() {
        let prev = vec![Some("E1".to_string()), Some("E1".to_string())];
        let (owners, inserts) = replay_diff(&prev, "a\nb\n", "a\n", "E2", "a\nb\n");
        assert_eq!(inserts, 0);
        assert_eq!(owners, vec![Some("E1".to_string())]);
    }

    #[test]
    fn waste_rate_is_complement_of_survival() {
        let stat = Stat {
            introduced: 10,
            surviving: 6,
            ..Default::default()
        };
        assert!((stat.survival_rate() - 0.6).abs() < 1e-9);
        assert!((stat.waste_rate() - 0.4).abs() < 1e-9);
    }

    #[test]
    fn empty_stat_is_zero_not_nan() {
        let stat = Stat::default();
        assert_eq!(stat.survival_rate(), 0.0);
        assert_eq!(stat.waste_rate(), 0.0);
    }
}
