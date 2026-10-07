use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::cli::LensArgs;
use crate::object::Event;
use crate::provenance::{chain_to, line_owners};
use crate::repo::Repo;
use crate::store::Store;

/// `re lens path/to/file.rs`
///
/// Annotate each line with the ledger event last associated with it: the
/// short id, the recorded agent, and an excerpt of the recorded prompt.
/// The association is the ledger's snapshot diff. It is not authorship.
///
/// Algorithm: walk every event from oldest to newest; for each event, diff its
/// pre vs post state of the target file and update an in-memory "line → event"
/// map. Each insertion associates a new line with that event; each deletion
/// frees the line; modifications re-associate. Lines with no event stay
/// unassociated.
pub fn run(args: LensArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let rel = PathBuf::from(args.file.replace('\\', "/"));
    let abs = repo.root.join(&rel);
    let current =
        std::fs::read_to_string(&abs).with_context(|| format!("reading {}", abs.display()))?;

    // One engine: the same owners `re why` would name line by line.
    let head = repo.head_event()?;
    let chain = chain_to(&store, head.as_deref())?;
    let mut owners = line_owners(&store, &chain, &rel)?;

    // Final reality check: align owners to the current on-disk content.
    let actual_lines: Vec<&str> = current.lines().collect();
    if owners.len() != actual_lines.len() {
        // The file changed on disk since the last event. Causari will still
        // annotate lines it can; missing ones get "?".
        owners.resize(actual_lines.len(), None);
    }

    // Cache event metadata for printing.
    let mut meta_cache: HashMap<String, Event> = HashMap::new();
    for o in owners.iter().flatten() {
        if !meta_cache.contains_key(o) {
            meta_cache.insert(o.clone(), store.read_event(o)?);
        }
    }

    // Assign each unique owner a distinct color from a small palette so a
    // screenshot is instantly readable.
    let palette = [
        |s: String| s.bright_cyan().to_string(),
        |s: String| s.bright_magenta().to_string(),
        |s: String| s.bright_green().to_string(),
        |s: String| s.bright_yellow().to_string(),
        |s: String| s.bright_blue().to_string(),
        |s: String| s.bright_red().to_string(),
    ];
    let mut color_of: HashMap<String, usize> = HashMap::new();
    for o in owners.iter().flatten() {
        let next = color_of.len() % palette.len();
        color_of.entry(o.clone()).or_insert(next);
    }

    // Print header with the legend.
    println!("{}", args.file.bold().underline());
    if !color_of.is_empty() {
        println!();
        println!("{}", "legend:".bright_black());
        let mut entries: Vec<(&String, &usize)> = color_of.iter().collect();
        entries.sort_by_key(|(_, c)| **c);
        for (id, c) in entries {
            let ev = meta_cache.get(id).unwrap();
            let label = format!(
                "  {}  {}  {}  [{}]",
                &id[..10],
                ev.agent.as_deref().unwrap_or("?"),
                ev.message.as_deref().unwrap_or(""),
                ev.evidence
                    .as_ref()
                    .map(|e| e.label())
                    .unwrap_or("unrecorded")
            );
            println!("{}", palette[*c](label));
        }
    }
    println!();

    // Render each line: " 42 │ <colored short id> │ <colored source line>".
    let max_line = actual_lines.len();
    let pad = max_line.to_string().len();
    for (i, line) in actual_lines.iter().enumerate() {
        let lineno = format!("{:>width$}", i + 1, width = pad).bright_black();
        let (id_str, line_str) = match owners.get(i).and_then(|o| o.as_ref()) {
            Some(oid) => {
                let c = color_of[oid];
                let short = &oid[..10];
                (palette[c](short.to_string()), palette[c](line.to_string()))
            }
            None => ("?????????? ".bright_black().to_string(), line.to_string()),
        };
        println!("{} {} {} {}", lineno, "│".bright_black(), id_str, line_str);
    }
    Ok(())
}
