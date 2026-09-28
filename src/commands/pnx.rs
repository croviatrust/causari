//! `re pnx` — Proof of Non-Exfiltration from the command line.
//!
//! ```text
//! re proxy --pnx                                   # witness a session; Ctrl-C signs the sheet
//! re pnx list                                      # runs of this repository
//! re pnx sheet [RUN]                               # the signed run sheet (public)
//! re pnx prove --asset api_key=secret.txt          # proof against the latest closed run
//! re pnx verify proof.json --asset api_key=secret.txt
//! ```
//!
//! Exit codes of `verify` (and of `prove --fail-on-present`) follow
//! `tacet-pnx`: 0 proof valid and every asset absent; 1 proof valid but an
//! asset present, undetectable or only partially covered; 2 proof invalid
//! or unverifiable. Proofs from `tacet-pnx` verify here, and proofs from
//! here verify under `tacet-pnx`.

use anyhow::{Context, Result, anyhow, bail};
use colored::Colorize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cli::{PnxArgs, PnxAssetArgs, PnxCommand};
use crate::pnx::{VERDICT_ABSENT, VerifyResult, verify_proof_with};
use crate::pnx_reach::{Policy, VERDICT_OUTSIDE, VERDICT_UNPOLICED, VERDICT_WITHIN};
use crate::pnx_run::{self, Run, RunSummary};
use crate::repo::Repo;
use crate::seal;

pub const EXIT_OK: i32 = 0;
pub const EXIT_PRESENT: i32 = 1;
pub const EXIT_INVALID: i32 = 2;

pub fn run(args: PnxArgs) -> Result<()> {
    match args.command {
        PnxCommand::List => list(),
        PnxCommand::Sheet { run, close } => sheet(run.as_deref(), close),
        PnxCommand::Prove {
            run,
            assets,
            out,
            fail_on_present,
        } => {
            let code = prove(run.as_deref(), &assets, out.as_deref(), fail_on_present)?;
            exit_with(code)
        }
        PnxCommand::Verify {
            proof,
            assets,
            policy,
            name,
            strict,
            json,
        } => {
            let policy = policy.as_deref().map(Policy::load).transpose()?;
            let code = verify(&proof, &assets, policy.as_ref(), &name, strict, json)?;
            exit_with(code)
        }
    }
}

fn exit_with(code: i32) -> Result<()> {
    if code == EXIT_OK {
        return Ok(());
    }
    let _ = std::io::stdout().flush();
    std::process::exit(code)
}

// ---------------------------------------------------------------------------
// list / sheet
// ---------------------------------------------------------------------------

fn list() -> Result<()> {
    let repo = Repo::discover()?;
    let runs = pnx_run::list_runs(&repo)?;
    if runs.is_empty() {
        println!(
            "no PNX runs yet — run {} to witness a session",
            "re proxy --pnx".cyan()
        );
        return Ok(());
    }
    for r in &runs {
        print_summary(r);
    }
    Ok(())
}

fn print_summary(r: &RunSummary) {
    let state = if r.closed {
        "closed".to_string()
    } else {
        "open".yellow().to_string()
    };
    println!(
        "  {}  {}  {}  {} bodies, {} bytes, {} fingerprints{}",
        r.run_id.cyan(),
        r.opened_at.bright_black(),
        state,
        r.bodies,
        r.bytes,
        r.fingerprints,
        r.root
            .as_deref()
            .map(|root| format!("  {}", &root[..root.len().min(23)])
                .bright_black()
                .to_string())
            .unwrap_or_default()
    );
}

/// `--run` resolution: an explicit id / directory / sheet path, or the
/// latest closed run of the repository.
fn resolve(spec: Option<&str>) -> Result<Run> {
    let repo = Repo::discover().ok();
    match spec {
        Some(s) => pnx_run::resolve_run(repo.as_ref(), s),
        None => {
            let repo = repo.ok_or_else(|| {
                anyhow!("no causari repository here; pass --run <run dir or sheet.json>")
            })?;
            let latest = pnx_run::list_runs(&repo)?
                .into_iter()
                .rev()
                .find(|r| r.closed)
                .ok_or_else(|| {
                    anyhow!(
                        "no closed PNX run in this repository; run `re proxy --pnx` and stop it with Ctrl-C, \
                         or `re pnx sheet --close <run>` for a run left open"
                    )
                })?;
            Run::load(&repo, &latest.run_id)
        }
    }
}

fn sheet(spec: Option<&str>, close: bool) -> Result<()> {
    let run = if close {
        let repo = Repo::discover()?;
        let id = match spec {
            Some(s) => s.to_string(),
            None => pnx_run::latest_open_run(&repo)?
                .ok_or_else(|| anyhow!("no open PNX run to close"))?,
        };
        let mut run = pnx_run::resolve_run(Some(&repo), &id)?;
        if run.is_closed() {
            bail!("run {:?} is already closed", run.run_id());
        }
        let key = pnx_run::witness_key(&repo)?;
        run.close(&key)?;
        eprintln!(
            "run {} closed — {} bodies, {} bytes, {} fingerprints; sheet at {}",
            run.run_id(),
            run.witness.bodies,
            run.witness.bytes,
            run.witness.map.len(),
            run.sheet_path().display()
        );
        run
    } else {
        resolve(spec)?
    };
    let sheet = run.sheet()?;
    println!("{}", serde_json::to_string_pretty(&sheet)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// assets
// ---------------------------------------------------------------------------

/// Labelled asset bytes from `--asset LABEL=PATH`, `--assets-dir DIR`
/// (label = path relative to DIR, `/`-separated) and `--asset-env VAR`
/// (label = `env:VAR`), in that order, as `tacet-pnx` does.
pub fn collect_assets(args: &PnxAssetArgs) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    for spec in &args.asset {
        let (label, path) = spec
            .split_once('=')
            .ok_or_else(|| anyhow!("--asset expects LABEL=PATH, got {spec:?}"))?;
        if label.is_empty() {
            bail!("--asset {spec:?}: empty label");
        }
        let bytes =
            std::fs::read(path).with_context(|| format!("reading asset {label} from {path}"))?;
        out.push((label.to_string(), bytes));
    }
    for dir in &args.assets_dir {
        let mut files = Vec::new();
        walk(dir, &mut files).with_context(|| format!("reading assets under {}", dir.display()))?;
        files.sort();
        for f in files {
            let rel = f
                .strip_prefix(dir)
                .unwrap_or(&f)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let bytes = std::fs::read(&f).with_context(|| format!("reading {}", f.display()))?;
            out.push((rel, bytes));
        }
    }
    for var in &args.asset_env {
        let value = std::env::var_os(var)
            .ok_or_else(|| anyhow!("environment variable {var} is not set"))?;
        out.push((
            format!("env:{var}"),
            value.to_string_lossy().as_bytes().to_vec(),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for (label, _) in &out {
        if !seen.insert(label) {
            bail!("duplicate asset label {label:?}");
        }
    }
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn has_assets(args: &PnxAssetArgs) -> bool {
    !(args.asset.is_empty() && args.assets_dir.is_empty() && args.asset_env.is_empty())
}

// ---------------------------------------------------------------------------
// prove
// ---------------------------------------------------------------------------

fn prove(
    run_spec: Option<&str>,
    assets: &PnxAssetArgs,
    out: Option<&Path>,
    fail_on_present: bool,
) -> Result<i32> {
    let mut run = resolve(run_spec)?;
    if !run.is_closed() {
        bail!(
            "run {:?} ({}) is still open; stop the proxy or run `re pnx sheet --close {}` first",
            run.run_id(),
            run.dir().display(),
            run.run_id()
        );
    }
    let labelled = collect_assets(assets)?;
    if labelled.is_empty() {
        bail!("no assets: use --asset LABEL=PATH, --assets-dir DIR or --asset-env VAR");
    }
    let sheet = run.sheet()?;
    let proof = run.witness.prove(&sheet, &labelled)?;
    let text = format!("{}\n", serde_json::to_string_pretty(&proof)?);

    let dest = match out {
        Some(p) if p == Path::new("-") => None,
        Some(p) => Some(p.to_path_buf()),
        None => Some(run.proof_path()),
    };
    match &dest {
        Some(p) => {
            if let Some(parent) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            crate::keys::write_atomic(p, text.as_bytes())?;
        }
        None => print!("{text}"),
    }

    let per = |v: &str| {
        proof["assets"]
            .as_array()
            .map(|a| a.iter().filter(|x| x["verdict"] == v).count())
            .unwrap_or(0)
    };
    let verdict = proof["verdict"].as_str().unwrap_or("?");
    eprintln!(
        "run {}: verdict {} over {} asset(s) (absent {}, partial {}, present {}, undetectable {})",
        run.run_id().cyan(),
        verdict.bold(),
        labelled.len(),
        per("absent"),
        per("absent-partial"),
        per("present"),
        per("undetectable")
    );
    for a in proof["assets"].as_array().into_iter().flatten() {
        let v = a["verdict"].as_str().unwrap_or("?");
        if v != VERDICT_ABSENT {
            eprintln!(
                "  {:<14} {} ({} bytes)",
                v,
                a["label"].as_str().unwrap_or("?"),
                a["asset_len"]
            );
        }
    }
    if let Some(p) = &dest {
        eprintln!(
            "  proof   {}  {}",
            p.display(),
            "(no traffic or asset bytes inside; verify with `re pnx verify` or `tacet-pnx verify`)"
                .bright_black()
        );
    }
    Ok(if fail_on_present && verdict != VERDICT_ABSENT {
        EXIT_PRESENT
    } else {
        EXIT_OK
    })
}

// ---------------------------------------------------------------------------
// verify
// ---------------------------------------------------------------------------

/// What surrounded the proof: nothing, or a `crovia.seal.v1` delivered by
/// `tacet-pnx prove --seal-key`. The Seal binds the query (run and asset
/// hashes) and the proof by hash; an invalid outer Seal fails the whole
/// verification.
#[derive(Debug, Default)]
struct Outer {
    sealed: bool,
    /// The Seal verifies and binds the query and the proof.
    seal_ok: Option<bool>,
    /// The Seal's own signature and structure verify, whatever it binds.
    /// A valid seal over a forged proof is still a valid seal: `tacet-pnx`
    /// reports the two separately and so does this.
    seal_signature_ok: Option<bool>,
    seal_errors: Vec<String>,
    issuer_id: Option<String>,
    seal_id: Option<String>,
}

/// The question a sealed PNX proof answers, as `tacet-pnx` writes it.
fn pnx_query(proof: &Value) -> Value {
    json!({
        "profile": crate::pnx::PROFILE,
        "run_id": proof["sheet"]["run_id"],
        "assets": proof["assets"].as_array().into_iter().flatten().map(|a| json!({
            "label": a["label"], "asset_sha256": a["asset_sha256"]
        })).collect::<Vec<_>>(),
    })
}

fn verify_any(
    obj: &Value,
    assets: Option<&BTreeMap<String, Vec<u8>>>,
    policy: Option<&Policy>,
    names: &[String],
) -> (VerifyResult, Outer) {
    let mut outer = Outer::default();
    let proof = if obj.get("seal").is_some() && obj.get("proof").is_some() {
        outer.sealed = true;
        let s = &obj["seal"];
        let mut errs = Vec::new();
        let signature_ok = match seal::verify_seal(s) {
            Ok(()) => true,
            Err(e) => {
                errs.push(format!("{e:#}"));
                false
            }
        };
        let bind = |field: &str, value: &Value| -> Option<String> {
            let hash = seal::csc1_serialize(value)
                .map(|b| format!("sha256:{}", seal::sha256_hex(&b)))
                .ok()?;
            (s["subject"][field].as_str() != Some(hash.as_str())).then(|| field.to_string())
        };
        if bind("input_hash", obj.get("query").unwrap_or(&json!({}))).is_some() {
            errs.push("seal.subject.input_hash does not bind the query".to_string());
        }
        if bind("output_hash", &obj["proof"]).is_some() {
            errs.push("seal.subject.output_hash does not bind the proof".to_string());
        }
        if obj.get("query") != Some(&pnx_query(&obj["proof"])) {
            errs.push("query does not describe this proof".to_string());
        }
        outer.seal_ok = Some(errs.is_empty());
        outer.seal_signature_ok = Some(signature_ok);
        outer.seal_errors = errs;
        outer.issuer_id = s["issuer"]["id"].as_str().map(String::from);
        outer.seal_id = s["seal_id"].as_str().map(String::from);
        &obj["proof"]
    } else {
        obj
    };
    let mut res = verify_proof_with(proof, assets, policy, names);
    if outer.seal_ok == Some(false) {
        res.ok = false;
        let mut errors = outer.seal_errors.clone();
        errors.append(&mut res.errors);
        res.errors = errors;
    }
    (res, outer)
}

fn verify(
    path: &Path,
    assets: &PnxAssetArgs,
    policy: Option<&Policy>,
    names: &[String],
    strict: bool,
    json_out: bool,
) -> Result<i32> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let obj: Value =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    let supplied: Option<BTreeMap<String, Vec<u8>>> = if has_assets(assets) {
        Some(collect_assets(assets)?.into_iter().collect())
    } else {
        None
    };
    let (res, outer) = verify_any(&obj, supplied.as_ref(), policy, names);
    let proof = if outer.sealed { &obj["proof"] } else { &obj };
    let sheet = &proof["sheet"];
    let reach_verdict = res.reach.as_ref().map(|r| r.verdict.as_str());

    if json_out {
        let assets: serde_json::Map<String, Value> = res
            .assets
            .iter()
            .map(|(l, v)| (l.clone(), Value::String(v.clone())))
            .collect();
        let mut report = json!({
            "ok": res.ok,
            "verdict": res.verdict,
            "assets": assets,
            "errors": res.errors,
            "warnings": res.warnings,
            "sealed": outer.sealed,
        });
        if let Some(r) = &res.reach {
            report["reach"] = json!({
                "verdict": r.verdict, "outside": r.outside, "reached": r.reached,
            });
        }
        if outer.sealed {
            report["seal_ok"] = json!(outer.seal_ok);
            report["seal_signature_ok"] = json!(outer.seal_signature_ok);
            report["seal_errors"] = json!(outer.seal_errors);
            report["issuer_id"] = json!(outer.issuer_id);
            report["seal_id"] = json!(outer.seal_id);
        }
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        let status = if res.ok {
            "VALID".green().bold()
        } else {
            "INVALID".red().bold()
        };
        println!(
            "{status} · verdict {} · run {} · witness {}{}",
            res.verdict.bold(),
            sheet["run_id"].as_str().unwrap_or("?").cyan(),
            sheet["witness"]["id"].as_str().unwrap_or("?"),
            outer
                .issuer_id
                .as_deref()
                .map(|id| format!(" · sealed by {id}"))
                .unwrap_or_default()
        );
        let eg = &sheet["egress"];
        println!(
            "  egress {} bodies, {} bytes, {} → {}; guarantee for shared substrings ≥ {} bytes",
            eg["bodies"],
            eg["bytes"],
            eg["first_at"].as_str().unwrap_or("?"),
            eg["last_at"].as_str().unwrap_or("?"),
            sheet["params"]["threshold"]
        );
        for (label, v) in &res.assets {
            println!("  {:<14} {}", v, label);
        }
        if let (Some(r), Some(rec)) = (&res.reach, sheet.get("reach").and_then(Value::as_object)) {
            let sm = rec.get("summary").cloned().unwrap_or_default();
            let pol = rec.get("policy").cloned().unwrap_or_default();
            let verdict = if r.verdict == VERDICT_OUTSIDE {
                r.verdict.red().bold()
            } else {
                r.verdict.bold()
            };
            println!(
                "  reach  {verdict} · {} destinations, {} connections ({} allowed, {} blocked, {} failed) · policy {} {}{} · {} · {}",
                sm["destinations"],
                sm["connections"],
                sm["allowed"],
                sm["blocked"],
                sm["failed"],
                pol["kind"].as_str().unwrap_or("?"),
                pol["mode"].as_str().unwrap_or("?"),
                pol["hash"]
                    .as_str()
                    .map(|h| format!(" {h}"))
                    .unwrap_or_default(),
                rec.get("disclosure").and_then(Value::as_str).unwrap_or("?"),
                rec.get("capture").and_then(Value::as_str).unwrap_or("?"),
            );
            if rec.get("disclosure").and_then(Value::as_str) == Some("clear") {
                for d in rec
                    .get("destinations")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    println!(
                        "  {:<14} {}:{} ({} connections)",
                        d["outcome"].as_str().unwrap_or("?"),
                        d["host"].as_str().unwrap_or("?"),
                        d["port"],
                        d["connections"]
                    );
                }
            }
            for o in &r.outside {
                println!("  {}  {} reached outside the policy", "outside".red(), o);
            }
            for (n, hit) in &r.reached {
                println!("  {:<14} {n}", if *hit { "reached" } else { "absent" });
            }
        }
        for e in &res.errors {
            println!("  {}    {}", "error".red(), e);
        }
        for w in &res.warnings {
            println!("  {}  {}", "warning".yellow(), w);
        }
        println!(
            "  {} a valid proof speaks for the bytes this witness saw, normalised as the sheet declares; \
             nothing about traffic that bypassed it, other encodings, or assets shorter than {} bytes.",
            "note:".bright_black(),
            sheet["params"]["k_gram"]
        );
    }

    let reach_unsettled =
        strict && reach_verdict.is_some_and(|v| v != VERDICT_WITHIN && v != VERDICT_UNPOLICED);
    Ok(if !res.ok {
        EXIT_INVALID
    } else if res.verdict != VERDICT_ABSENT
        || reach_verdict == Some(VERDICT_OUTSIDE)
        || reach_unsettled
        || (strict && !res.warnings.is_empty())
    {
        EXIT_PRESENT
    } else {
        EXIT_OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset_args(asset: &[&str], dirs: &[&Path], env: &[&str]) -> PnxAssetArgs {
        PnxAssetArgs {
            asset: asset.iter().map(|s| s.to_string()).collect(),
            assets_dir: dirs.iter().map(|d| d.to_path_buf()).collect(),
            asset_env: env.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn assets_are_collected_in_tacet_pnx_order_with_the_same_labels() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("key.txt"), b"KEY").unwrap();
        std::fs::create_dir_all(d.join("protected/sub")).unwrap();
        std::fs::write(d.join("protected/b.txt"), b"B").unwrap();
        std::fs::write(d.join("protected/sub/a.txt"), b"A").unwrap();
        // SAFETY: tests in this module run in one process; the variable is
        // unique to this test.
        unsafe { std::env::set_var("CAUSARI_PNX_TEST_ASSET", "from-env") };
        let key = format!("api={}", d.join("key.txt").display());
        let got = collect_assets(&asset_args(
            &[&key],
            &[&d.join("protected")],
            &["CAUSARI_PNX_TEST_ASSET"],
        ))
        .unwrap();
        let labels: Vec<&str> = got.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            ["api", "b.txt", "sub/a.txt", "env:CAUSARI_PNX_TEST_ASSET"]
        );
        assert_eq!(got[3].1, b"from-env");

        assert!(collect_assets(&asset_args(&["nolabel"], &[], &[])).is_err());
        assert!(collect_assets(&asset_args(&["=x"], &[], &[])).is_err());
        assert!(collect_assets(&asset_args(&[&key, &key], &[], &[])).is_err());
        assert!(collect_assets(&asset_args(&[], &[], &["CAUSARI_PNX_UNSET_VAR"])).is_err());
        assert!(!has_assets(&asset_args(&[], &[], &[])));
    }

    #[test]
    fn a_bare_proof_and_a_broken_seal_wrapper_are_told_apart() {
        let doc: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/vectors/pnx/witness.json"
        )))
        .unwrap();
        let proof = &doc["proofs"][0]["proof"];
        let (res, outer) = verify_any(proof, None, None, &[]);
        assert!(res.ok && !outer.sealed);

        // A sealed bundle whose Seal does not verify fails closed, even
        // though the inner proof is fine.
        let bundle = json!({"seal": {"seal_version": "crovia.seal.v1"}, "query": pnx_query(proof), "proof": proof});
        let (res, outer) = verify_any(&bundle, None, None, &[]);
        assert!(outer.sealed && outer.seal_ok == Some(false));
        assert!(!res.ok);
        assert!(!res.errors.is_empty());
    }
}
