use anyhow::{Result, anyhow};
use colored::Colorize;
use similar::{ChangeTag, TextDiff};

use crate::cli::DiffArgs;
use crate::object::resolve_id;
use crate::repo::Repo;
use crate::snapshot::flatten_tree;
use crate::store::Store;

pub fn run(args: DiffArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let (from_id, to_id) = if let Some((a, b)) = args.spec.split_once("..") {
        let a = resolve_id(&repo.objects_dir(), a)?;
        let b = resolve_id(&repo.objects_dir(), b)?;
        (a, b)
    } else {
        // Single event => diff its pre vs post.
        let id = resolve_id(&repo.objects_dir(), &args.spec)?;
        let ev = store.read_event(&id)?;
        return diff_snapshots(&store, &ev.pre_snapshot, &ev.post_snapshot);
    };

    let from_ev = store.read_event(&from_id)?;
    let to_ev = store.read_event(&to_id)?;
    diff_snapshots(&store, &from_ev.post_snapshot, &to_ev.post_snapshot)
}

fn diff_snapshots(store: &Store, from: &str, to: &str) -> Result<()> {
    let from_snap = store.read_snapshot(from)?;
    let to_snap = store.read_snapshot(to)?;
    let from_tree = flatten_tree(store, &from_snap.tree)?;
    let to_tree = flatten_tree(store, &to_snap.tree)?;

    let mut all_paths: Vec<_> = from_tree.keys().chain(to_tree.keys()).collect();
    all_paths.sort();
    all_paths.dedup();

    for path in all_paths {
        let a_id = from_tree.get(path);
        let b_id = to_tree.get(path);
        match (a_id, b_id) {
            (Some(a), Some(b)) if a == b => continue,
            (None, Some(_)) => {
                println!("{} {}", "+++ added:".green().bold(), path.display());
            }
            (Some(_), None) => {
                println!("{} {}", "--- removed:".red().bold(), path.display());
            }
            (Some(a), Some(b)) => {
                println!("{} {}", "~~~ modified:".yellow().bold(), path.display());
                let av = store.read_blob(a)?;
                let bv = store.read_blob(b)?;
                let a_text = String::from_utf8(av).unwrap_or_default();
                let b_text = String::from_utf8(bv).unwrap_or_default();
                let diff = TextDiff::from_lines(&a_text, &b_text);
                for change in diff.iter_all_changes() {
                    let (sign, styled): (char, _) = match change.tag() {
                        ChangeTag::Delete => ('-', change.to_string().red()),
                        ChangeTag::Insert => ('+', change.to_string().green()),
                        ChangeTag::Equal => continue,
                    };
                    print!("  {}{}", sign, styled);
                }
            }
            (None, None) => unreachable!(),
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn _silence_unused() -> Result<()> {
    Err(anyhow!("unused"))
}
