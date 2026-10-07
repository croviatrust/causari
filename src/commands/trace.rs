use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::cli::TraceArgs;
use crate::provenance::{chain_to, find_line_origin, parse_spec};
use crate::repo::Repo;
use crate::snapshot::{effective_reads, effective_writes};
use crate::store::Store;

/// `re trace path/to/file.rs:42`
///
/// **Declared read/write graph, walked backward.**
///
/// `re why` names the one ledger event whose snapshot last contained the
/// line. `re trace` walks outward from that event:
///
/// 1. Find the ledger event W associated with the target line.
/// 2. For each file F that W read (declared or inferred from snapshots),
///    find the most recent earlier event that wrote F.
/// 3. Repeat for each newly added event.
/// 4. The fixed point is the set of ledger events reachable by those edges.
///
/// The walk is not a proof that those events caused the line, and it is
/// not an intellectual ancestry. Edges exist only where the ledger recorded
/// a read or a write.
pub fn run(args: TraceArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let (file_str, line_no) = parse_spec(&args.spec)?;
    let rel_path = PathBuf::from(&file_str);

    let abs = repo.root.join(&rel_path);
    let current =
        std::fs::read_to_string(&abs).with_context(|| format!("reading {}", abs.display()))?;
    let current_lines: Vec<&str> = current.lines().collect();
    if line_no == 0 || line_no > current_lines.len() {
        return Err(anyhow!(
            "{} has {} lines (asked for line {})",
            file_str,
            current_lines.len(),
            line_no
        ));
    }
    let target_line = current_lines[line_no - 1].to_string();

    // 1. Resolve the writer event W: the same answer `re why` gives.
    let head = repo.head_event()?;
    let (origin, _) = find_line_origin(&store, head.as_deref(), &rel_path, &target_line)?;
    let writer_id = origin.map(|o| o.id).ok_or_else(|| {
        anyhow!("no recorded event introduced this line — it predates the first `re record`")
    })?;

    // 2. Build a fast lookup of (file path -> ordered list of event ids that wrote it),
    //    walking the whole chain from the head event so we can do last_writer_before(F, E).
    let chain = chain_to(&store, head.as_deref())?;
    let writer_history = build_writer_history(&store, &chain)?;
    let chain_order: HashMap<String, usize> = chain
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i))
        .collect();

    // 3. BFS the causal cone.
    let mut cone: HashMap<String, ConeNode> = HashMap::new();
    cone.insert(
        writer_id.clone(),
        ConeNode {
            reason: format!("wrote {}:{}", file_str, line_no),
            upstream: Vec::new(),
        },
    );
    let mut queue: VecDeque<String> = VecDeque::new();
    queue.push_back(writer_id.clone());

    while let Some(eid) = queue.pop_front() {
        let ev = store.read_event(&eid)?;
        let reads = effective_reads(&store, &ev)?;
        let ev_pos = *chain_order
            .get(&eid)
            .ok_or_else(|| anyhow!("event {} not in chain", &eid[..10]))?;

        let mut local_upstream = Vec::new();
        for file in &reads {
            if let Some(writers) = writer_history.get(file) {
                // Find the most recent writer whose position in the chain is
                // STRICTLY BEFORE ev_pos (note: chain is ordered oldest -> newest,
                // so "before" means smaller index).
                if let Some(prior) = writers
                    .iter()
                    .rev()
                    .find(|(pos, _)| *pos < ev_pos)
                    .map(|(_, id)| id.clone())
                {
                    local_upstream.push((file.clone(), prior.clone()));
                    if !cone.contains_key(&prior) {
                        let reason =
                            format!("wrote {} which event {} read", file.display(), &eid[..10]);
                        cone.insert(
                            prior.clone(),
                            ConeNode {
                                reason,
                                upstream: Vec::new(),
                            },
                        );
                        queue.push_back(prior);
                    }
                }
            }
        }
        if let Some(node) = cone.get_mut(&eid) {
            node.upstream = local_upstream;
        }
    }

    print_cone(&store, &writer_id, &cone, &file_str, line_no, &target_line)?;
    Ok(())
}

#[derive(Debug)]
struct ConeNode {
    reason: String,
    /// (file, upstream_event_id) pairs explaining what this event depended on.
    upstream: Vec<(PathBuf, String)>,
}

/// For every file, the list of (chain_position, event_id) that wrote it,
/// in chronological order (oldest first).
fn build_writer_history(
    store: &Store,
    chain: &[String],
) -> Result<HashMap<PathBuf, Vec<(usize, String)>>> {
    let mut out: HashMap<PathBuf, Vec<(usize, String)>> = HashMap::new();
    for (pos, id) in chain.iter().enumerate() {
        let ev = store.read_event(id)?;
        let writes = effective_writes(store, &ev.pre_snapshot, &ev.post_snapshot)?;
        for f in writes {
            out.entry(f).or_default().push((pos, id.clone()));
        }
    }
    Ok(out)
}

fn print_cone(
    store: &Store,
    root_id: &str,
    cone: &HashMap<String, ConeNode>,
    file: &str,
    line_no: usize,
    line: &str,
) -> Result<()> {
    println!("{}:{}", file.bold().underline(), line_no);
    println!("  {}", line.bright_white());
    println!();
    println!(
        "{} {} ledger events reachable by declared reads and writes",
        "trace:".green().bold(),
        cone.len().to_string().cyan()
    );
    println!();

    let mut visited: HashSet<String> = HashSet::new();
    print_node(store, root_id, cone, &mut visited, 0)?;
    Ok(())
}

fn print_node(
    store: &Store,
    id: &str,
    cone: &HashMap<String, ConeNode>,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Result<()> {
    let indent = "  ".repeat(depth);
    let prefix = if depth == 0 { "●" } else { "└─" };

    let ev = store.read_event(id)?;
    let short = &id[..10];

    let head = format!(
        "{}{} {}  {}",
        indent,
        prefix,
        short.yellow(),
        ev.message
            .as_deref()
            .unwrap_or("(no message)")
            .bright_white()
    );
    println!("{}", head);

    let detail_indent = format!("{}   ", indent);
    if let Some(a) = &ev.agent {
        let line = format!(
            "{}{} {}{}",
            detail_indent,
            "agent:".bright_black(),
            a.cyan(),
            ev.model
                .as_deref()
                .map(|m| format!(" / {}", m.bright_black()))
                .unwrap_or_default()
        );
        println!("{}", line);
    }
    if let Some(p) = &ev.prompt {
        let first_line = p.lines().next().unwrap_or("");
        let truncated = if first_line.len() > 90 {
            format!("{}…", &first_line[..90])
        } else {
            first_line.to_string()
        };
        println!(
            "{}{} {}",
            detail_indent,
            "prompt:".bright_black(),
            truncated.italic()
        );
    }
    if let Some(node) = cone.get(id) {
        println!(
            "{}{} {}",
            detail_indent,
            "because:".bright_black(),
            node.reason
        );
    }

    visited.insert(id.to_string());

    if let Some(node) = cone.get(id) {
        let mut deduped: Vec<&String> = node.upstream.iter().map(|(_, u)| u).collect();
        deduped.sort();
        deduped.dedup();
        for up in deduped {
            if visited.contains(up) {
                let ev = store.read_event(up)?;
                println!(
                    "{}   {} {}  {} {}",
                    detail_indent,
                    "↑".bright_black(),
                    (&up[..10]).bright_black(),
                    ev.message.as_deref().unwrap_or("").bright_black(),
                    "(already shown)".bright_black().italic()
                );
                continue;
            }
            print_node(store, up, cone, visited, depth + 1)?;
        }
    }

    Ok(())
}
