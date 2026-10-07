#!/usr/bin/env python3
"""Weekly Survival Report: a numbered, dated, citable report of AI code survival.

Built from the per-repository output of ``re audit <owner/repo> --json``
(method v2) and nothing else. The report states counts and intervals: how
many lines were introduced by commits that carry machine-readable AI
authorship metadata, and how many of them ``git blame`` still attributes to
those commits at HEAD. No rank, no colour, no verdict; rows are alphabetical.
Same pipeline shape as the Crovia Silence Report so the two read as siblings.

Inputs (a run directory)::

    run.json                 {"generated_at", "tool", "tool_version", "method",
                              "command", "repos", "failed", "opted_out"}
    <owner>__<repo>.json     the exact bytes of `re audit --json` per repository

Outputs (under site/reports/survival/)::

    <YYYY>/<NN>/index.html   the report page
    <YYYY>/<NN>/report.json  machine-readable counts, intervals, coverage
    <YYYY>/<NN>/report.md    plain-text version (Zenodo description)
    <YYYY>/<NN>/card.svg     Open Graph card, identity style
    <YYYY>/<NN>/card.png     same card as PNG (needs Pillow; skipped otherwise)
    <YYYY>/<NN>/repos/*.json the per-repository audit bytes every number links to
    index.html               archive, newest first
    feed.xml                 Atom, one entry per report
    latest.json              copy of the newest report.json
    plus managed blocks in site/_redirects and site/sitemap.xml

and, for every repository measured in any report (under site/r/, paths
always lowercase)::

    <owner>/<repo>/index.html   the repository page: latest counts, by agent, history
    <owner>/<repo>/badge.svg    one-colour text badge, ink on paper (badge-dark.svg: paper on ink)
    <owner>/<repo>/latest.json  compact latest counts, report id, links to page and bytes
    index.html                  alphabetical index of every measured repository

Usage::

    python3 scripts/survival_report.py merge-shards --run /tmp/run   # run-shard-*.json → run.json
    python3 scripts/survival_report.py build --run /tmp/run [--number N] [--date YYYY-MM-DD]
    python3 scripts/survival_report.py rebuild            # report pages, archive, feed, latest, /r/, redirects, sitemap
    python3 scripts/survival_report.py from-existing site/survival-data.json --out /tmp/run

Standard library only; Pillow is optional (card.png).
"""

from __future__ import annotations

import argparse
import datetime as dt
import glob
import hashlib
import html
import json
import random
import re
import shutil
import sys
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
from site_version import asset_url  # noqa: E402  content-versioned /styles.css and /app.js
SITE_URL = "https://causari.dev"
REPO_URL = "https://github.com/croviatrust/causari"
REPORTS_REL = "reports/survival"
REPOS_REL = "r"
SCHEMA = "causari.survival_report.v1"
REPO_SCHEMA = "causari.repo_survival.v1"
RESAMPLES = 2000
LICENSE = "CC-BY-4.0"
LICENSE_URL = "https://creativecommons.org/licenses/by/4.0/"

# Identity palette (canon/canon.json → glyphs.palette).
INK = "#0b0d10"
PAPER = "#f5f4ef"
GRAPHITE = "#3b4252"
MIST = "#9aa3ad"

# The run's reach receipt (PNX draft 0.4 §4a): the signed run sheet the egress
# witness of the audit shards produced, its verification, the policy it
# applied. Published next to the report when the run has one.
REACH_SHEET = "reach.sheet.json"
REACH_VERIFY = "reach.verify.json"
REACH_POLICY = "egress-policy.json"
REACH_POLICY_SOURCE = f"{REPO_URL}/blob/main/.github/egress-policy.json"
REACH_VERDICTS = ("within-policy", "outside-policy", "unchecked", "unpoliced")
REACH_VERIFIER_URL = "https://croviatrust.com/registry/seal/verify/"

REDIRECT_BEGIN = "# survival-report: begin (managed by scripts/survival_report.py)"
REDIRECT_END = "# survival-report: end"
SITEMAP_BEGIN = "<!-- survival-report: begin (managed by scripts/survival_report.py) -->"
SITEMAP_END = "<!-- survival-report: end -->"


# ----------------------------------------------------------------------------- helpers

def load_json(path: Path, default: Any = None) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return default


def dump_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")


def esc(s: Any) -> str:
    return html.escape(str(s), quote=True)


def fmt_int(n: int | None) -> str:
    return "—" if n is None else f"{n:,}"


def fmt_pct(x: float | None) -> str:
    return "—" if x is None else f"{x * 100:.1f} %"


def fmt_share(x: float | None) -> str:
    return "—" if x is None else f"{x * 100:.0f} %"


def repo_slug(repo: str) -> str:
    return repo.replace("/", "__")


def slug_repo(slug: str) -> str:
    return slug.replace("__", "/", 1)


def repo_path(repo: str) -> str:
    """Site path of a repository page. Always lowercase: GitHub names are
    case-insensitive, so one canonical URL serves every spelling and two
    spellings can never collide on a case-insensitive file system. The page
    itself shows the name as measured."""
    return f"/{REPOS_REL}/{repo.lower()}/"


def report_id(year: int, number: int) -> str:
    return f"{year}/{number:02d}"


def report_url(rid: str) -> str:
    return f"{SITE_URL}/{REPORTS_REL}/{rid}/"


def read_optout(root: Path) -> set[str]:
    """`.github/survival-optout.txt`: one owner/repo per line, `#` comments,
    matched case-insensitively. Same semantics as the workflow."""
    path = root / ".github" / "survival-optout.txt"
    out: set[str] = set()
    if not path.exists():
        return out
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.add(line.lower())
    return out


def read_repo_list(root: Path) -> list[str]:
    """`.github/survival-repos.txt`: one owner/repo per line, `#` comments,
    in file order. The workflow shards this list by index."""
    path = root / ".github" / "survival-repos.txt"
    if not path.exists():
        return []
    return [l.strip() for l in path.read_text(encoding="utf-8").splitlines() if l.strip() and not l.strip().startswith("#")]


SELECTION_BY_PR = "the repositories were added by pull request, not drawn at random."
SELECTION_SERIES = (
    "the repositories were selected, not drawn at random: a hand-picked list and the public repositories in which "
    "GitHub commit search finds at least five commits carrying the same AI authorship metadata, most-starred first; each report states its own selection."
)


def selection_sentence(root: Path) -> str:
    """How the measured list was put together, for the positioning statement.
    With `.github/survival-discovery.json` present the list was filled by
    scripts/survival_discover.py and the sentence says so, with the floor and
    the date; without it the list is the hand-picked one."""
    d = load_json(root / ".github" / "survival-discovery.json", None)
    if not isinstance(d, dict) or not isinstance(d.get("repositories"), list):
        return SELECTION_BY_PR
    rows = [r for r in d["repositories"] if isinstance(r, dict)]
    seeds = sum(1 for r in rows if r.get("seed"))
    found = len(rows) - seeds
    floor = (d.get("selection") or {}).get("floor", 5)
    min_stars = (d.get("selection") or {}).get("min_stars")
    stars = f" and at least {min_stars} stars, most-starred first" if min_stars else ""
    date = str(d.get("discovered_at") or "")[:10]
    retained = len(d.get("retained") or []) if isinstance(d.get("retained"), list) else 0
    kept = (f", {retained} of them found in an earlier week and counted again this week" if retained else "")
    return (
        f"the repositories were selected, not drawn at random: {seeds} hand-picked and {found} found by GitHub commit search "
        f"as public repositories with at least {floor} commits carrying the same AI authorship metadata{stars}"
        f"{' (discovered ' + date + kept + ')' if date else ''}; the selection rule and the counts behind it are public."
    )


# ----------------------------------------------------------------------------- statistics

def percentile(sorted_values: list[float], q: float) -> float:
    """Nearest-rank percentile on an already sorted list."""
    if not sorted_values:
        raise ValueError("empty")
    idx = int(round(q * (len(sorted_values) - 1)))
    return sorted_values[max(0, min(idx, len(sorted_values) - 1))]


def bootstrap_rate(pairs: list[tuple[int, int]], seed: int, resamples: int = RESAMPLES) -> dict[str, Any] | None:
    """95 % interval on Σ surviving / Σ introduced, resampling repositories
    with replacement. Seeded so the same report always yields the same bytes.
    Returns None with fewer than two repositories: an interval over one
    repository would be a fiction."""
    pairs = [(i, s) for i, s in pairs if i > 0]
    if len(pairs) < 2:
        return None
    rng = random.Random(seed)
    n = len(pairs)
    rates: list[float] = []
    for _ in range(resamples):
        intro = surv = 0
        for _ in range(n):
            i, s = pairs[rng.randrange(n)]
            intro += i
            surv += s
        rates.append(surv / intro)
    rates.sort()
    return {"low": percentile(rates, 0.025), "high": percentile(rates, 0.975)}


def bootstrap_median(values: list[float], seed: int, resamples: int = RESAMPLES) -> dict[str, Any] | None:
    values = [v for v in values if v is not None]
    if len(values) < 2:
        return None
    rng = random.Random(seed)
    n = len(values)
    meds: list[float] = []
    for _ in range(resamples):
        sample = sorted(values[rng.randrange(n)] for _ in range(n))
        meds.append(sample[n // 2] if n % 2 else (sample[n // 2 - 1] + sample[n // 2]) / 2)
    meds.sort()
    return {"low": percentile(meds, 0.025), "high": percentile(meds, 0.975)}


def median(values: list[float]) -> float | None:
    values = sorted(v for v in values if v is not None)
    if not values:
        return None
    n = len(values)
    return values[n // 2] if n % 2 else (values[n // 2 - 1] + values[n // 2]) / 2


# ----------------------------------------------------------------------------- collect

def is_v2(audit: dict[str, Any]) -> bool:
    cov = audit.get("coverage")
    return isinstance(cov, dict) and "method" in cov and "small_sample" in cov and "shallow" in cov


def stat_view(stat: dict[str, Any]) -> dict[str, Any]:
    return {
        "commits": int(stat.get("commits") or 0),
        "introduced": int(stat.get("introduced") or 0),
        "surviving": int(stat.get("surviving") or 0),
        "survival_rate": stat.get("survival_rate"),
        "median_survival": stat.get("median_survival"),
        "capped_survival_rate": stat.get("capped_survival_rate"),
        "cap_lines": stat.get("cap_lines"),
        "largest_commit_share": stat.get("largest_commit_share"),
    }


def cohort_view(stat: dict[str, Any]) -> dict[str, Any]:
    """One cohort of one age window: the three counts and the rate."""
    return {
        "commits": int(stat.get("commits") or 0),
        "introduced": int(stat.get("introduced") or 0),
        "surviving": int(stat.get("surviving") or 0),
        "survival_rate": stat.get("survival_rate"),
    }


def baseline_view(audit: dict[str, Any]) -> dict[str, Any] | None:
    """The method v3 `baseline` block of one audit, or None for an audit
    that predates it (method v2 rows keep building, without a baseline)."""
    b = audit.get("baseline")
    if not isinstance(b, dict) or not isinstance(b.get("untagged"), dict):
        return None
    m = b.get("age_matched")
    o = b.get("oldest_surviving")
    total = int(audit.get("total_commits") or 0)
    return {
        "untagged": stat_view(b["untagged"]),
        "by_age": [
            {"from_days": int(w.get("from_days") or 0), "to_days": w.get("to_days"),
             "tagged": cohort_view(w.get("tagged") or {}), "untagged": cohort_view(w.get("untagged") or {})}
            for w in (b.get("by_age") or []) if isinstance(w, dict)
        ],
        "age_matched": None if not isinstance(m, dict) else {
            "tagged_rate": m.get("tagged_rate"), "untagged_rate": m.get("untagged_rate"), "gap": m.get("gap"),
            "buckets_used": int(m.get("buckets_used") or 0), "tagged_lines_covered": m.get("tagged_lines_covered"),
        },
        "oldest_surviving": None if not isinstance(o, dict) else {
            "date": o.get("date"), "age_days": int(o.get("age_days") or 0),
            "commits_before": int(o.get("commits_before") or 0), "introduced_before": int(o.get("introduced_before") or 0),
            "tagged_commits_before": int(o.get("tagged_commits_before") or 0),
            "tagged_introduced_before": int(o.get("tagged_introduced_before") or 0),
            # share of every commit of the repository that predates the
            # oldest line still at HEAD; above one half the repository was
            # cleared or rewritten and the page says so
            "commits_before_share": (int(o.get("commits_before") or 0) / total) if total else None,
        },
    }


REWRITTEN_SHARE = 0.5


def rewritten(row: dict[str, Any]) -> bool:
    """More than half of a repository's commits predate the oldest line
    still at HEAD: nothing from before that date survives, tagged or not."""
    o = ((row.get("baseline") or {}).get("oldest_surviving")) or {}
    share = o.get("commits_before_share")
    return share is not None and share > REWRITTEN_SHARE


def window_label(w: dict[str, Any]) -> str:
    return f"{w['from_days']}–{w['to_days']} d" if w.get("to_days") is not None else f"{w['from_days']}+ d"


def aggregate_baseline(aggregated: list[dict[str, Any]], seed: int, sample_floor: int) -> dict[str, Any]:
    """What the baselines of the aggregated repositories say together: the
    median age-matched gap with its bootstrap interval, how many gaps fall on
    each side of zero, and the age windows pooled across repositories with
    the same direct standardisation `re audit` applies inside one."""
    with_gap = [r for r in aggregated if ((r.get("baseline") or {}).get("age_matched") or {}).get("gap") is not None]
    gaps = [r["baseline"]["age_matched"]["gap"] for r in with_gap]
    with_baseline = [r for r in aggregated if r.get("baseline")]
    pooled: dict[tuple[int, Any], dict[str, Any]] = {}
    for r in with_baseline:
        for w in r["baseline"]["by_age"]:
            key = (w["from_days"], w["to_days"])
            acc = pooled.setdefault(key, {"from_days": w["from_days"], "to_days": w["to_days"],
                                          "tagged": {"commits": 0, "introduced": 0, "surviving": 0},
                                          "untagged": {"commits": 0, "introduced": 0, "surviving": 0}})
            for cohort in ("tagged", "untagged"):
                for k in ("commits", "introduced", "surviving"):
                    acc[cohort][k] += w[cohort][k]
    pooled_by_age = []
    for key in sorted(pooled, key=lambda k: k[0]):
        acc = pooled[key]
        for cohort in ("tagged", "untagged"):
            c = acc[cohort]
            c["survival_rate"] = (c["surviving"] / c["introduced"]) if c["introduced"] else None
        pooled_by_age.append(acc)
    # No gap is computed across repositories: pooled windows mix one
    # repository's rewrite with another's bulk commit, which is exactly what
    # the within-repository gap avoids. The windows are published as counts;
    # only the median of per-repository gaps crosses repositories.
    return {
        "repositories": len(with_baseline),
        "repositories_with_gap": len(with_gap),
        "median_gap": median(gaps),
        "median_gap_interval_95": bootstrap_median(gaps, seed=seed),
        "gaps_negative": sum(1 for g in gaps if g < 0),
        "gaps_positive": sum(1 for g in gaps if g > 0),
        "rewritten": sorted((r["repo"] for r in aggregated if rewritten(r)), key=str.lower),
        "rewritten_rule": f"more than {REWRITTEN_SHARE:.0%} of the repository's commits predate the oldest line still at HEAD",
        "pooled_by_age": pooled_by_age,
        "pooled_by_age_note": (
            "counts summed across the repositories with a baseline, one row per age window; one large repository "
            "can dominate a window, so no gap is computed from these rows: the gap is computed inside each "
            "repository and only its median crosses repositories"
        ),
        "definition": (
            "gap = AI-tagged line-weighted survival minus untagged survival re-weighted to the age mix of the "
            "AI-tagged lines of the same repository, over age windows where both cohorts hold at least "
            f"{sample_floor} commits, computed inside each repository; untagged = commits with no machine-readable AI signal (human-written, "
            "inline-completed and untagged-agent code alike); age = commit date to HEAD date. A negative gap "
            "means AI-tagged lines survive less than untagged lines in the matched age windows of the same repository."
        ),
    }


def audit_identity(fp: Path, audit: dict[str, Any]) -> tuple[str, str]:
    """What makes two audits the same measurement: the commit they measured
    (`repository.head`, written by `re audit` from 0.2.1 on) and, for older
    outputs, the exact bytes. Two names for one repository — GitHub keeps the
    old name working after a rename — produce the same bytes."""
    repo = audit.get("repository")
    head = repo.get("head") if isinstance(repo, dict) else None
    return (str(head or ""), hashlib.sha256(fp.read_bytes()).hexdigest())


def drop_duplicate_audits(rows: list[dict[str, Any]], listed: list[str]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """One repository counts once, whatever it is called. Rows whose audits
    measured the same commit, or are byte-identical, are one measurement: the
    name present in `.github/survival-repos.txt` is kept (the list is where a
    rename gets corrected); when both or neither are listed, the first name
    in case-insensitive order. Every dropped name is recorded with the name
    it was counted under, so the report says so rather than silently
    shrinking."""
    listed_keys = {name.lower() for name in listed}

    def rank(r: dict[str, Any]) -> tuple[int, str]:
        return (0 if r["repo"].lower() in listed_keys else 1, r["repo"].lower())

    by_head: dict[str, list[dict[str, Any]]] = {}
    by_bytes: dict[str, list[dict[str, Any]]] = {}
    for r in rows:
        head, digest = audit_identity(r["_source"], load_json(r["_source"], {}) or {})
        r["_head"] = head
        if head:
            by_head.setdefault(head, []).append(r)
        by_bytes.setdefault(digest, []).append(r)

    dropped: dict[str, dict[str, Any]] = {}
    for reason, groups in (("same commit at HEAD", by_head), ("byte-identical audit output", by_bytes)):
        for group in groups.values():
            if len(group) < 2:
                continue
            group = sorted(group, key=rank)
            keep = group[0]
            for r in group[1:]:
                if r["repo"] in dropped or r is keep:
                    continue
                dropped[r["repo"]] = {"dropped": r["repo"], "kept": keep["repo"], "reason": reason,
                                      "audit_file": r["audit_file"], "kept_audit_file": keep["audit_file"]}
    kept_rows = [r for r in rows if r["repo"] not in dropped]
    for r in kept_rows:
        r.pop("_head", None)
    return kept_rows, sorted(dropped.values(), key=lambda d: d["dropped"].lower())


def reach_log_scope(run_dir: Path) -> dict[str, int]:
    """How many shard logs were actually uploaded, against how many shards
    reported a fragment. Absent when no log is in the run directory: the
    sheet then speaks only for the bytes it contains, and the sentence must
    not invent a count of shards."""
    uploaded = len(list(run_dir.glob("reach-shard-*.jsonl")))
    if not uploaded:
        return {}
    run = load_json(run_dir / "run.json", None)
    out: dict[str, int] = {"uploaded": uploaded}
    if isinstance(run, dict) and isinstance(run.get("shards"), list) and run["shards"]:
        out["shards"] = len(run["shards"])
    return out


def collect_reach(run_dir: Path, root: Path) -> dict[str, Any] | None:
    """The run's reach receipt: `reach.sheet.json`, a `crovia.pnx.v1` run
    sheet signed by the egress witness the audit shards ran behind, whose
    reach record says where the measurement connected and under which
    policy. The workflow verifies it (`tacet-pnx verify --policy --json`)
    into `reach.verify.json` before the build; a sheet without a passing
    verification is not published. A run without a sheet has no receipt."""
    sheet_path = run_dir / REACH_SHEET
    if not sheet_path.exists():
        return None
    sheet = load_json(sheet_path, None)
    if not isinstance(sheet, dict) or sheet.get("profile") != "crovia.pnx.v1" or not isinstance(sheet.get("reach"), dict):
        raise SystemExit(f"{sheet_path}: not a crovia.pnx.v1 run sheet with a reach record")
    verify = load_json(run_dir / REACH_VERIFY, None)
    if not isinstance(verify, dict):
        raise SystemExit(f"{run_dir / REACH_VERIFY} missing: the reach sheet must be verified "
                         f"(tacet-pnx verify {REACH_SHEET} --policy ... --json) before it is published")
    if verify.get("ok") is not True or verify.get("verdict") != "sheet-only":
        why = "; ".join(str(e) for e in (verify.get("errors") or [])) or "not ok"
        raise SystemExit(f"{run_dir / REACH_VERIFY}: the reach sheet did not verify ({why}); refusing to publish it")
    rv = verify.get("reach") or {}
    verdict = rv.get("verdict")
    if verdict not in REACH_VERDICTS:
        raise SystemExit(f"{run_dir / REACH_VERIFY}: unknown reach verdict {verdict!r}")
    rec = sheet["reach"]
    policy_path = root / ".github" / "egress-policy.json"
    files = {REACH_SHEET: sheet_path}
    if policy_path.exists():
        files[REACH_POLICY] = policy_path
    witness = sheet.get("witness") or {}
    return {
        "profile": "crovia.pnx.v1",
        "record": rec.get("version"),
        "sheet": REACH_SHEET,
        "run_id": sheet.get("run_id"),
        "closed_at": sheet.get("closed_at"),
        "witness": {"id": witness.get("id"), "key_hex": (witness.get("pubkey") or {}).get("key_hex")},
        "capture": rec.get("capture"),
        "disclosure": rec.get("disclosure"),
        "policy": {
            **{k: (rec.get("policy") or {}).get(k) for k in ("kind", "mode", "hash", "rules")},
            "file": REACH_POLICY if REACH_POLICY in files else None,
            "source": REACH_POLICY_SOURCE,
        },
        "summary": rec.get("summary"),
        "destinations": [
            {k: d.get(k) for k in ("host", "host_hash", "port", "outcome", "connections", "bytes_out", "bytes_in", "first_at", "last_at") if k in d}
            for d in (rec.get("destinations") or [])
        ],
        "verdict": verdict,
        "outside": list(rv.get("outside") or []),
        "warnings": [str(w) for w in (verify.get("warnings") or [])],
        "logs": reach_log_scope(run_dir),
        "covers": "the connection attempts in the shard logs that were uploaded and concatenated into this sheet",
        "not_covered": "any shard that uploaded no log; what the runner does outside the measurement step "
                       "(checking out this repository, installing the tool, uploading the shard); "
                       "and any connection that did not go through the witness",
        "verify": [f"tacet-pnx verify {REACH_SHEET} --policy {REACH_POLICY}", f"re pnx verify {REACH_SHEET} --policy {REACH_POLICY}"],
        "_files": files,
    }


def collect(run_dir: Path, number: int, date: str, root: Path) -> dict[str, Any]:
    run = load_json(run_dir / "run.json", None)
    if not isinstance(run, dict):
        raise SystemExit(f"{run_dir}/run.json missing or invalid")
    optout = read_optout(root)

    rows: list[dict[str, Any]] = []
    shallow: list[str] = []
    not_v2: list[str] = []
    # Repositories the workflow already skipped never reach the run dir; those
    # that did reach it are dropped here, so the list is honoured either way.
    opted: set[str] = {str(n).lower() for n in (run.get("opted_out") or [])}
    for fp in sorted(run_dir.glob("*__*.json")):
        repo = slug_repo(fp.stem)
        if repo.lower() in optout:
            opted.add(repo.lower())
            continue
        audit = load_json(fp, None)
        if not isinstance(audit, dict):
            not_v2.append(repo)
            continue
        if not is_v2(audit):
            not_v2.append(repo)
            continue
        cov = audit["coverage"]
        if cov.get("shallow"):
            shallow.append(repo)
            continue
        v = stat_view(audit.get("verified") or {})
        p = stat_view(audit.get("probable") or {})
        rows.append({
            "repo": repo,
            "url": f"https://github.com/{repo}",
            "audit_file": f"repos/{fp.name}",
            "reproduce": f"re audit {repo} --json",
            "total_commits": int(audit.get("total_commits") or 0),
            "verified": v,
            "probable": p,
            "by_agent": {k: stat_view(s) for k, s in sorted((audit.get("by_agent") or {}).items())},
            "baseline": baseline_view(audit),
            "coverage": {
                "method": cov.get("method"),
                "blame_flags": list(cov.get("blame_flags") or []),
                "ignore_revs_file": bool(cov.get("ignore_revs_file")),
                "shallow": bool(cov.get("shallow")),
                "sample_floor": int(cov.get("sample_floor") or 0),
                "small_sample": bool(cov.get("small_sample")),
            },
            "aggregated": not cov.get("small_sample") and v["introduced"] > 0,
            "_source": fp,
        })
    opted_out = len(opted)
    every_audit = list(rows)
    rows, duplicates = drop_duplicate_audits(rows, read_repo_list(root))

    # Alphabetical, case-insensitive, and nothing else: never by rate.
    rows.sort(key=lambda r: r["repo"].lower())
    aggregated = [r for r in rows if r["aggregated"]]
    not_aggregated = [r for r in rows if not r["aggregated"]]

    sample_floor = max((r["coverage"]["sample_floor"] for r in rows), default=5)
    blame_flags = next((r["coverage"]["blame_flags"] for r in rows if r["coverage"]["blame_flags"]), [])
    method_version = next((r["coverage"]["method"] for r in rows if r["coverage"]["method"]), run.get("method") or "v2")

    intro = sum(r["verified"]["introduced"] for r in aggregated)
    surv = sum(r["verified"]["surviving"] for r in aggregated)
    ai_commits = sum(r["verified"]["commits"] for r in aggregated)
    total_commits = sum(r["total_commits"] for r in aggregated)
    pairs = [(r["verified"]["introduced"], r["verified"]["surviving"]) for r in aggregated]
    capped = [r["verified"]["capped_survival_rate"] for r in aggregated]

    by_agent: dict[str, dict[str, int]] = {}
    for r in aggregated:
        for agent, s in r["by_agent"].items():
            a = by_agent.setdefault(agent, {"repositories": 0, "commits": 0, "introduced": 0, "surviving": 0})
            a["repositories"] += 1
            a["commits"] += s["commits"]
            a["introduced"] += s["introduced"]
            a["surviving"] += s["surviving"]
    by_agent_out = {
        agent: {**a, "survival_rate": (a["surviving"] / a["introduced"]) if a["introduced"] else None}
        for agent, a in sorted(by_agent.items())
    }

    year = int(date[:4])
    rid = report_id(year, number)
    interval_note = (
        f"95 % percentile interval from {RESAMPLES} bootstrap resamples of the {len(aggregated)} aggregated "
        f"repositories (with replacement, seed {number}). It describes the sampled repositories, "
        "not all AI-assisted code, and not the repositories not in this sample."
    )
    facts = {
        "schema": SCHEMA,
        "title": f"Survival Report #{number}",
        "number": number,
        "id": rid,
        "date": date,
        "generated_at": run.get("generated_at") or dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "url": report_url(rid),
        "license": LICENSE,
        "license_url": LICENSE_URL,
        "publisher": "Crovia Trust",
        "tool": {"name": run.get("tool") or "causari", "version": run.get("tool_version") or "unknown"},
        "method": {
            "version": method_version,
            "url": f"{SITE_URL}/method",
            "command": run.get("command") or "re audit <owner/repo> --json",
            "blame_flags": blame_flags,
            "sample_floor": sample_floor,
            "cap_rule": "a commit weighs at most the 95th percentile of per-commit introduced line counts in its repository, and never more than 10,000 lines",
            "evidence_class": "VERIFIED only; PROBABLE is listed but never summed",
            "aggregation": "repositories with at least sample_floor VERIFIED commits and a full (non-shallow) clone",
            "selection": selection_sentence(root),
        },
        "aggregate": {
            "repositories": len(aggregated),
            "total_commits": total_commits,
            "ai_tagged_commits": ai_commits,
            "introduced": intro,
            "surviving": surv,
            "survival_rate": (surv / intro) if intro else None,
            "survival_rate_interval_95": bootstrap_rate(pairs, seed=number),
            "median_capped_survival_rate": median(capped),
            "median_capped_survival_rate_interval_95": bootstrap_median(capped, seed=number),
            "interval_method": {"kind": "bootstrap over repositories", "resamples": RESAMPLES, "seed": number, "note": interval_note},
            "baseline": aggregate_baseline(aggregated, seed=number, sample_floor=sample_floor),
        },
        "by_agent": by_agent_out,
        "repositories": [{k: v for k, v in r.items() if not k.startswith("_")} for r in aggregated],
        "not_aggregated": [{k: v for k, v in r.items() if not k.startswith("_")} for r in not_aggregated],
        "excluded": {
            "shallow": sorted(shallow, key=str.lower),
            "not_method_v2": sorted(not_v2, key=str.lower),
            "failed": sorted((run.get("failed") or []), key=str.lower),
            "duplicates": duplicates,
            "opted_out": opted_out,
            "optout_file": f"{REPO_URL}/blob/main/.github/survival-optout.txt",
        },
        "reach": collect_reach(run_dir, root),
        "doi": None,
        "concept_doi": None,
        "zenodo": None,
        # every audit file of the run is kept next to the page, dropped
        # duplicates included: the byte-identity a reader may want to check
        # is checkable there.
        "_sources": {r["repo"]: r["_source"] for r in every_audit},
    }
    return facts


# ----------------------------------------------------------------------------- text

def headline(f: dict[str, Any]) -> str:
    a = f["aggregate"]
    if not a["repositories"]:
        return "No repository met the aggregation rule in this run."
    s = (
        f"{fmt_int(a['surviving'])} of {fmt_int(a['introduced'])} lines introduced by "
        f"{fmt_int(a['ai_tagged_commits'])} AI-tagged commits in {a['repositories']} open-source repositories "
        f"are still at HEAD ({fmt_pct(a['survival_rate'])})."
    )
    iv = a["survival_rate_interval_95"]
    if iv:
        s += f" 95 % interval over the sampled repositories: {fmt_pct(iv['low'])} to {fmt_pct(iv['high'])}."
    return s


def revision_of(f: dict[str, Any]) -> int:
    return int(f.get("revision") or 1)


def cite(f: dict[str, Any]) -> str:
    rev = f", revision {revision_of(f)}" if revision_of(f) > 1 else ""
    s = f"Crovia Trust. Survival Report #{f['number']} ({f['date']}{rev}). {f['url']}"
    if f.get("doi"):
        s += f" DOI {f['doi']}"
    return s


def correction_lines(f: dict[str, Any]) -> list[str]:
    """One plain sentence per correction, newest first: what changed, when,
    and where the superseded bytes are. Used by the page, the markdown and
    the feed, so the three say the same thing."""
    out = []
    for c in sorted(f.get("corrections") or [], key=lambda c: -int(c["revision"])):
        prev = c.get("previous") or {}
        agg = prev.get("aggregate") or {}
        was = ""
        if agg.get("introduced") is not None:
            was = (f" Revision {int(c['revision']) - 1} counted {agg.get('repositories')} repositories, "
                   f"{fmt_int(agg.get('surviving'))} of {fmt_int(agg.get('introduced'))} lines ({fmt_pct(agg.get('survival_rate'))});"
                   f" its bytes are kept unchanged at {prev.get('file')}" + (f", DOI {prev['doi']}" if prev.get("doi") else "") + ".")
        out.append(f"Revision {c['revision']} ({c['date']}): {c['note']}{was}")
    return out


POSITIONING = {
    "is": (
        "This report counts lines. For each repository it states how many lines were introduced by commits "
        "that carry machine-readable AI authorship metadata (trailers such as Co-Authored-By naming an agent, "
        "bot author identities, aider markers, and, from method v4, git-ai notes that name a non-empty tool; method v3 counted any git-ai note), and how many of those lines git blame still "
        "attributes to those commits at HEAD, under the method version stated on the page (blame with -w -M -C, "
        "a per-commit weight cap, a sample floor, full clones only; from method v3 the untagged lines of the same "
        "repository, in the matched age windows, stand next to the AI-tagged ones). Every row is reproducible with one command."
    ),
    "is_not": (
        "It is not a quality judgement, a productivity measure, total AI usage, proof of authorship, "
        "a security finding, a correctness finding, or a causal effect of AI. Deleted lines include "
        "removed features and rewritten prototypes; surviving lines include dead code. It is not a "
        "sample of all AI-assisted code: inline completions "
        "leave no trace in git, untagged agent commits are invisible, and {selection} "
        "The intervals describe the sampled repositories only."
    ),
    "context": (
        "Prior measurement work asks related questions with different instruments. GitClear publishes churn "
        "reports built from code-change patterns across the repositories it analyses; arXiv 2601.16809 "
        "(\"Will It Survive?\") follows the modification of agent-authored code in 201 projects with its own "
        "detector and finds that such code is modified less often than human-written code. This report does "
        "not reproduce either method and does not adjudicate between them: it publishes counts from git "
        "metadata alone, with the method version, the tool version and the exact bytes behind every number, "
        "so that the three can be read side by side."
    ),
}


def is_not_text(selection: str) -> str:
    return POSITIONING["is_not"].format(selection=selection)


def report_selection(f: dict[str, Any]) -> str:
    """The selection sentence of one report: stored in report.json from the
    run that discovered the list; earlier reports carry the hand-picked one."""
    return (f.get("method") or {}).get("selection") or SELECTION_BY_PR


def fmt_gap(x: float | None) -> str:
    return "—" if x is None else f"{x * 100:+.1f} pts"


def row_gap(row: dict[str, Any]) -> dict[str, Any] | None:
    return ((row.get("baseline") or {}).get("age_matched")) or None


def row_untagged_rate(row: dict[str, Any]) -> str:
    """The untagged rate re-weighted to the AI-tagged age mix, or the rule that withheld it."""
    m = row_gap(row)
    if m and m.get("untagged_rate") is not None:
        return fmt_pct(m["untagged_rate"])
    if row.get("baseline"):
        return "no shared window"
    return "—"


def baseline_md(f: dict[str, Any]) -> list[str]:
    """The baseline section of report.md; empty for reports built before method v3."""
    b = (f.get("aggregate") or {}).get("baseline")
    if not b or not b.get("repositories"):
        return []
    lines = ["", "## Baseline: the same repositories' untagged lines, in the matched age windows", "",
             b["definition"], ""]
    if b.get("repositories_with_gap"):
        lines.append(f"- Repositories with an age-matched gap: {b['repositories_with_gap']} of {b['repositories']} with a baseline")
        lines.append(f"- Median gap across them: {fmt_gap(b['median_gap'])}")
        iv = b.get("median_gap_interval_95")
        if iv:
            lines.append(f"- 95 % bootstrap interval on that median: {fmt_gap(iv['low'])} to {fmt_gap(iv['high'])}")
        lines.append(f"- Gaps below zero: {b['gaps_negative']} · above zero: {b['gaps_positive']}")
    if b.get("rewritten"):
        lines.append(f"- Cleared or rewritten ({b['rewritten_rule']}): {', '.join(b['rewritten'])}. "
                     "Nothing from before that date survives in them, tagged or not; their rows measure the rewrite as much as the code.")
    if b.get("pooled_by_age"):
        lines += ["", f"Age windows, {b['pooled_by_age_note']}.", "", "| Line age | AI-tagged commits | AI-tagged lines | Still at HEAD | AI-tagged | Untagged commits | Untagged lines | Still at HEAD | Untagged |",
                  "|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
        for w in b["pooled_by_age"]:
            t, u = w["tagged"], w["untagged"]
            if not (t["commits"] or u["commits"]):
                continue
            lines.append(f"| {window_label(w)} | {fmt_int(t['commits'])} | {fmt_int(t['introduced'])} | {fmt_int(t['surviving'])} | {fmt_pct(t['survival_rate'])} | "
                         f"{fmt_int(u['commits'])} | {fmt_int(u['introduced'])} | {fmt_int(u['surviving'])} | {fmt_pct(u['survival_rate'])} |")
    return lines


def reach_destination_label(d: dict[str, Any]) -> str:
    host = d.get("host") or (str(d.get("host_hash") or "")[:16] + "…")
    return f"{host}:{d.get('port')}"


def reach_sentence(r: dict[str, Any]) -> str:
    """One plain sentence on the receipt: what the witness saw, under which policy."""
    sm = r.get("summary") or {}
    pol = r.get("policy") or {}
    where = (f"{sm.get('destinations', 0)} destination(s) over {sm.get('connections', 0)} connection(s): "
             f"{sm.get('allowed', 0)} allowed, {sm.get('blocked', 0)} blocked, {sm.get('failed', 0)} failed")
    if pol.get("kind") == "allowlist":
        under = f"under an allowlist of {pol.get('rules')} rule(s) in {pol.get('mode')} mode"
    else:
        under = "with no policy in force (destinations stated, not judged)"
    logs = r.get("logs") or {}
    uploaded, shards = logs.get("uploaded"), logs.get("shards")
    if isinstance(uploaded, int) and isinstance(shards, int) and shards and uploaded < shards:
        scope = f"{uploaded} of {shards} uploaded shard logs; a shard that uploaded no log is absent from this sheet"
    elif isinstance(uploaded, int) and isinstance(shards, int) and shards:
        scope = f"{uploaded} of {shards} uploaded shard logs"
    else:
        scope = "the shard logs concatenated into this sheet"
    return f"The witness recorded, in {scope}, {where}, {under}."


def reach_verdict_sentence(r: dict[str, Any]) -> str:
    v = r.get("verdict")
    if v == "within-policy":
        return "Every destination the witness saw is one the policy allows."
    if v == "outside-policy":
        return f"A destination outside the policy was reached: {', '.join(r.get('outside') or [])}."
    if v == "unchecked":
        return "The policy could not be checked against the destinations."
    return "No policy was in force; the destinations are stated, not judged."


def reach_md(f: dict[str, Any]) -> list[str]:
    r = f.get("reach")
    if not r:
        return []
    pol = r.get("policy") or {}
    lines = ["", "## Where this measurement connected", "",
             reach_sentence(r) + " " + reach_verdict_sentence(r),
             "",
             "| Destination | Outcome | Connections | Bytes out | Bytes in |", "|---|---|---:|---:|---:|"]
    for d in r.get("destinations") or []:
        lines.append(f"| {reach_destination_label(d)} | {d.get('outcome')} | {fmt_int(d.get('connections'))} | "
                     f"{fmt_int(d.get('bytes_out'))} | {fmt_int(d.get('bytes_in'))} |")
    lines += ["",
              f"Signed run sheet: {f['url']}{r['sheet']} (profile {r['profile']}, witness `{(r.get('witness') or {}).get('id')}`, "
              f"capture `{r.get('capture')}`, disclosure `{r.get('disclosure')}`).  "]
    if pol.get("hash"):
        lines.append(f"Policy: {f['url']}{pol['file']} (source {pol['source']}), bound in the sheet as `{pol['hash']}`.  "
                     if pol.get("file") else f"Policy bound in the sheet as `{pol['hash']}` (source {pol['source']}).  ")
    lines.append("Verify: `" + "` or `".join(r.get("verify") or []) + f"`, or paste the sheet and the policy at {REACH_VERIFIER_URL}.  ")
    lines += ["", f"Covered: {r['covers']}. Not covered: {r['not_covered']}. "
              "The record says where the measurement connected through the witness and how many bytes crossed, nothing about their content."]
    return lines


def report_md(f: dict[str, Any]) -> str:
    a = f["aggregate"]
    m = f["method"]
    rev = f" (revision {revision_of(f)})" if revision_of(f) > 1 else ""
    lines = [
        f"# Survival Report #{f['number']} — {f['date']}{rev}",
        "",
        f"Counts of surviving lines from AI-tagged commits in {a['repositories']} open-source repositories, "
        f"measured with {f['tool']['name']} {f['tool']['version']}, method {m['version']}. "
        "Counts, not grades: no rank, no verdict; rows are alphabetical.",
        "",
        f"Page: {f['url']}  ",
        f"Data: {f['url']}report.json  ",
        f"Feed: {SITE_URL}/{REPORTS_REL}/feed.xml  ",
        f"Licence: {LICENSE}  ",
    ]
    if f.get("doi"):
        lines.append(f"DOI: https://doi.org/{f['doi']}  ")
    if f.get("corrections"):
        lines += ["", "## Corrections", ""] + [f"- {c}" for c in correction_lines(f)]
    lines += ["", "## Aggregate", "", headline(f), ""]
    if a["repositories"]:
        lines += [
            f"- Repositories aggregated: {a['repositories']}",
            f"- Commits in those repositories (no merges): {fmt_int(a['total_commits'])}",
            f"- AI-tagged (VERIFIED) commits: {fmt_int(a['ai_tagged_commits'])}",
            f"- Lines introduced by them: {fmt_int(a['introduced'])}",
            f"- Still attributed to them at HEAD: {fmt_int(a['surviving'])}",
            f"- Line-weighted ratio: {fmt_pct(a['survival_rate'])}",
        ]
        iv = a["survival_rate_interval_95"]
        if iv:
            lines.append(f"- 95 % bootstrap interval over the sampled repositories: {fmt_pct(iv['low'])} to {fmt_pct(iv['high'])}")
        lines.append(f"- Median of per-repository capped ratios: {fmt_pct(a['median_capped_survival_rate'])}")
        iv2 = a["median_capped_survival_rate_interval_95"]
        if iv2:
            lines.append(f"- 95 % bootstrap interval on that median: {fmt_pct(iv2['low'])} to {fmt_pct(iv2['high'])}")
        lines.append("")
        lines.append(a["interval_method"]["note"])
        lines.append("")
    lines += baseline_md(f)
    has_baseline = any(r.get("baseline") for r in f["repositories"])
    extra_head = " Untagged, matched windows | Gap |" if has_baseline else ""
    extra_sep = "---:|---:|" if has_baseline else ""
    lines += ["", "## Repositories (alphabetical)", "",
              f"| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Line-weighted | Capped | Median per commit | Largest commit |{extra_head} Reproduce |",
              f"|---|---:|---:|---:|---:|---:|---:|---:|---:|{extra_sep}---|"]
    for r in f["repositories"]:
        v = r["verified"]
        extra = f" {row_untagged_rate(r)} | {fmt_gap((row_gap(r) or {}).get('gap'))} |" if has_baseline else ""
        mark = " · rewritten" if rewritten(r) else ""
        lines.append(
            f"| {r['repo']}{mark} | {fmt_int(r['total_commits'])} | {fmt_int(v['commits'])} | {fmt_int(v['introduced'])} | "
            f"{fmt_int(v['surviving'])} | {fmt_pct(v['survival_rate'])} | {fmt_pct(v['capped_survival_rate'])} | "
            f"{fmt_pct(v['median_survival'])} | {fmt_share(v['largest_commit_share'])} |{extra} `{r['reproduce']}` |"
        )
    if f["by_agent"]:
        lines += ["", "## By agent, across aggregated repositories (alphabetical)", "",
                  "| Agent | Repositories | Commits | Introduced | Still at HEAD | Line-weighted |", "|---|---:|---:|---:|---:|---:|"]
        for agent, s in f["by_agent"].items():
            lines.append(f"| {agent} | {s['repositories']} | {fmt_int(s['commits'])} | {fmt_int(s['introduced'])} | {fmt_int(s['surviving'])} | {fmt_pct(s['survival_rate'])} |")
    if f["not_aggregated"]:
        lines += ["", f"## Measured but not aggregated (fewer than {m['sample_floor']} AI-tagged commits)", "",
                  "| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Reproduce |", "|---|---:|---:|---:|---:|---|"]
        for r in f["not_aggregated"]:
            v = r["verified"]
            lines.append(f"| {r['repo']} | {fmt_int(r['total_commits'])} | {fmt_int(v['commits'])} | {fmt_int(v['introduced'])} | {fmt_int(v['surviving'])} | `{r['reproduce']}` |")
    ex = f["excluded"]
    lines += ["", "## Excluded from this report", ""]
    lines.append(f"- Shallow clones (history truncated; method {m['version']} refuses them): {', '.join(ex['shallow']) or 'none'}")
    lines.append(f"- Audits that failed in this run: {', '.join(ex['failed']) or 'none'}")
    if ex["not_method_v2"]:
        lines.append(f"- Outputs without a coverage block (method v1): {', '.join(ex['not_method_v2'])}")
    for d in ex.get("duplicates") or []:
        lines.append(f"- {d['dropped']} is the same repository as {d['kept']} ({d['reason']}); counted once, under {d['kept']}")
    lines.append(f"- Opted out by their maintainers ({ex['optout_file']}): {ex['opted_out']}")
    lines += reach_md(f)
    lines += ["", "## Method", "",
              f"Method {m['version']}, {f['tool']['name']} {f['tool']['version']}. Detection from commit metadata only; "
              f"survival from `git blame {' '.join(m['blame_flags'])}` at HEAD. Per-commit cap: {m['cap_rule']}. "
              f"Sample floor: {m['sample_floor']} VERIFIED commits. {m['evidence_class']}. Full clones only. "
              + ("Baseline (method v3): the untagged lines of the same repository, by age; the gap is defined in the Baseline section. "
                 if (a.get('baseline') or {}).get('repositories') else "")
              + f"Details, limits and how to contest a number: {m['url']}.",
              "", "## What this report is, and is not", "", POSITIONING["is"], "", is_not_text(report_selection(f)), "", POSITIONING["context"],
              "", "## Cite", "", cite(f), ""]
    return "\n".join(lines)


# ----------------------------------------------------------------------------- card

def card_svg(f: dict[str, Any]) -> str:
    a = f["aggregate"]
    mono = "'JetBrains Mono','SF Mono','Cascadia Mono','Fira Code',Consolas,monospace"
    r = 14.5 * 1.3
    discs = ((28, 34), (72, 34), (50, 72))
    disc_svg = "\n".join(
        f'  <circle cx="{72 + x * 1.3:.1f}" cy="{72 + y * 1.3:.1f}" r="{r:.1f}" fill="{PAPER}"/>' for x, y in discs
    )
    big = f"{fmt_int(a['surviving'])} of {fmt_int(a['introduced'])} lines" if a["repositories"] else "no repository aggregated"
    line2 = (f"from {fmt_int(a['ai_tagged_commits'])} AI-tagged commits in {a['repositories']} repositories are still at HEAD"
             if a["repositories"] else "in this run")
    iv = a["survival_rate_interval_95"]
    line3 = (f"{fmt_pct(a['survival_rate'])} · 95 % interval over the sampled repositories {fmt_pct(iv['low'])} – {fmt_pct(iv['high'])}"
             if iv else (fmt_pct(a["survival_rate"]) if a["repositories"] else ""))
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 630" width="1200" height="630" role="img" aria-label="Survival Report #{f['number']}">
  <rect width="1200" height="630" fill="{INK}"/>
{disc_svg}
  <text x="252" y="170" font-family="{mono}" font-size="72" font-weight="500" letter-spacing="-1" fill="{PAPER}">causari</text>
  <text x="72" y="270" font-family="{mono}" font-size="26" fill="{MIST}">Survival Report #{f['number']} · {esc(f['date'])} · method {esc(f['method']['version'])}</text>
  <text x="72" y="345" font-family="{mono}" font-size="44" fill="{PAPER}">{esc(big)}</text>
  <text x="72" y="400" font-family="{mono}" font-size="{26 if len(line2) > 62 else 30}" fill="{PAPER}">{esc(line2)}</text>
  <text x="72" y="450" font-family="{mono}" font-size="24" fill="{MIST}">{esc(line3)}</text>
  <text x="72" y="540" font-family="{mono}" font-size="24" fill="{MIST}">counts, not grades  ·  re audit &lt;owner/repo&gt; --json  ·  {LICENSE}</text>
  <text x="72" y="580" font-family="{mono}" font-size="24" fill="{MIST}">causari.dev/{REPORTS_REL}</text>
</svg>
"""


def card_png(f: dict[str, Any], out: Path) -> bool:
    try:
        from PIL import Image, ImageDraw
    except ImportError:
        print("survival_report: Pillow not installed; card.png skipped", file=sys.stderr)
        return False
    sys.path.insert(0, str(HERE))
    import identity  # scripts/identity.py: palette, disc geometry, font lookup

    a = f["aggregate"]
    S = 2
    img = Image.new("RGB", (1200 * S, 630 * S), INK)
    d = ImageDraw.Draw(img)

    def fitted(text: str, size: int, max_width: int = 1056 * S):
        # shrink until the line fits the card's text column
        while size > 14 * S and d.textlength(text, font=identity.font(size)) > max_width:
            size -= 1 * S
        return identity.font(size)

    identity.draw_discs(d, 72 * S, 72 * S, 1.3 * S, PAPER)
    d.text((252 * S, 118 * S), "causari", font=identity.font(72 * S), fill=PAPER)
    d.text((72 * S, 246 * S), f"Survival Report #{f['number']} · {f['date']} · method {f['method']['version']}", font=identity.font(26 * S), fill=MIST)
    big = f"{fmt_int(a['surviving'])} of {fmt_int(a['introduced'])} lines" if a["repositories"] else "no repository aggregated"
    d.text((72 * S, 305 * S), big, font=fitted(big, 44 * S), fill=PAPER)
    line2 = (f"from {fmt_int(a['ai_tagged_commits'])} AI-tagged commits in {a['repositories']} repositories are still at HEAD"
             if a["repositories"] else "in this run")
    d.text((72 * S, 372 * S), line2, font=fitted(line2, 30 * S), fill=PAPER)
    iv = a["survival_rate_interval_95"]
    if iv:
        line3 = f"{fmt_pct(a['survival_rate'])} · 95 % interval over the sampled repositories {fmt_pct(iv['low'])} – {fmt_pct(iv['high'])}"
    else:
        line3 = fmt_pct(a["survival_rate"]) if a["repositories"] else ""
    d.text((72 * S, 428 * S), line3, font=fitted(line3, 24 * S), fill=MIST)
    d.text((72 * S, 518 * S), f"counts, not grades  ·  re audit <owner/repo> --json  ·  {LICENSE}", font=identity.font(24 * S), fill=MIST)
    d.text((72 * S, 558 * S), f"causari.dev/{REPORTS_REL}", font=identity.font(24 * S), fill=MIST)
    img = img.resize((1200, 630), Image.LANCZOS)
    out.parent.mkdir(parents=True, exist_ok=True)
    img.save(out, optimize=True)
    return True


# ----------------------------------------------------------------------------- html

def page_head(title: str, desc: str, url: str, image: str, jsonld: dict[str, Any]) -> str:
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover" />
  <title>{esc(title)} — causari</title>
  <meta name="description" content="{esc(desc)}" />
  <meta name="theme-color" content="#0b0d10" />
  <meta name="color-scheme" content="light dark" />
  <link rel="canonical" href="{esc(url)}" />
  <link rel="alternate" type="application/atom+xml" title="Causari Survival Report" href="/{REPORTS_REL}/feed.xml" />
  <meta property="og:type" content="article" />
  <meta property="og:title" content="{esc(title)}" />
  <meta property="og:description" content="{esc(desc)}" />
  <meta property="og:url" content="{esc(url)}" />
  <meta property="og:image" content="{esc(image)}" />
  <meta property="og:image:width" content="1200" />
  <meta property="og:image:height" content="630" />
  <meta property="og:site_name" content="causari" />
  <meta name="twitter:card" content="summary_large_image" />
  <meta name="twitter:title" content="{esc(title)}" />
  <meta name="twitter:description" content="{esc(desc)}" />
  <meta name="twitter:image" content="{esc(image)}" />
  <link rel="icon" type="image/svg+xml" href="/assets/favicon.svg" />
  <link rel="stylesheet" href="{asset_url('styles.css')}" />
  <script type="application/ld+json">{json.dumps(jsonld, ensure_ascii=False).replace('</', '<\\/')}</script>
</head>
<body>

<a class="skip-link" href="#main">Skip to content</a>

<header class="nav" role="banner">
  <div class="container nav-inner">
    <a href="/" class="brand" aria-label="causari home">
      <img src="/assets/mark.svg" alt="" width="34" height="34" class="mark-light" />
      <img src="/assets/mark-white.svg" alt="" width="34" height="34" class="mark-dark" />
      <span class="brand-name" translate="no">causari</span>
    </a>
    <nav class="nav-links" aria-label="Primary">
      <a href="/{REPORTS_REL}/" aria-current="page">reports</a>
      <a href="/method">method</a>
      <a href="/verify/">verify</a>
      <a href="/faq">faq</a>
      <a href="{REPO_URL}" rel="noopener" class="hide-sm">source</a>
      <button class="theme-toggle" id="theme-toggle" aria-label="Toggle light and dark" title="Toggle light and dark">◐</button>
    </nav>
  </div>
</header>

<main id="main">
"""


def page_foot() -> str:
    return f"""</main>

<footer class="footer">
  <div class="container">
    <div class="foot-bottom">
      <p>© <span id="year">2026</span> <a href="https://croviatrust.com" rel="noopener">Crovia</a> · <em>causari</em> is a trademark of Crovia Trust. Report text and data <a href="{LICENSE_URL}" rel="license noopener">{LICENSE}</a>.</p>
      <p class="muted">Every number reproducible: <code translate="no">re audit &lt;owner/repo&gt; --json</code> · <a href="/method">method</a> · <a href="/faq">faq</a> · <a href="/{REPORTS_REL}/feed.xml">feed</a> · <a href="/">causari.dev</a></p>
    </div>
  </div>
</footer>

<script src="{asset_url('app.js')}" defer></script>
</body>
</html>
"""


def positioning_html(selection: str) -> str:
    """The positioning statement as a proof card: title, the paragraphs, one command."""
    return f"""<div class="proof rp-positioning">
      <h3>What this report is, and is not</h3>
      <p>{esc(POSITIONING['is'])}</p>
      <p>{esc(is_not_text(selection))}</p>
      <p>{esc(POSITIONING['context']).replace('arXiv 2601.16809', '<a href="https://arxiv.org/abs/2601.16809" rel="noopener">arXiv 2601.16809</a>')}</p>
<pre translate="no"><code translate="no">re audit &lt;owner/repo&gt; --json   <span class="dim"># the exact bytes behind any row</span></code></pre>
    </div>"""


def num_cell(value: str, href: str, title: str) -> str:
    return f'<td><a class="rp-num" href="{esc(href)}" title="{esc(title)}">{value}</a></td>'


def repo_rows(rows: list[dict[str, Any]], full: bool, baseline: bool = False) -> str:
    out = []
    for r in rows:
        v = r["verified"]
        p = r["probable"]
        href = r["audit_file"]
        t = r["reproduce"]
        probable = f' <span class="muted">(+{fmt_int(p["commits"])} probable)</span>' if p["commits"] else ""
        mark = (' <span class="muted" title="More than half of the commits predate the oldest line still at HEAD: '
                'nothing from before that date survives, tagged or not">· rewritten</span>' if rewritten(r) else "")
        cells = [
            f'<td><a href="{esc(repo_path(r["repo"]))}" title="{esc(r["repo"])}: page, history and badge">{esc(r["repo"])}</a>{mark}</td>',
            num_cell(fmt_int(r["total_commits"]), href, t),
            num_cell(fmt_int(v["commits"]), href, t).replace("</a></td>", f"</a>{probable}</td>"),
            num_cell(fmt_int(v["introduced"]), href, t),
            num_cell(fmt_int(v["surviving"]), href, t),
        ]
        if full:
            cells += [
                num_cell(fmt_pct(v["survival_rate"]), href, t),
                num_cell(fmt_pct(v["capped_survival_rate"]), href, t),
                num_cell(fmt_pct(v["median_survival"]), href, t),
                num_cell(fmt_share(v["largest_commit_share"]), href, t),
            ]
            if baseline:
                m = row_gap(r)
                if m and m.get("gap") is not None:
                    cells += [num_cell(fmt_pct(m["untagged_rate"]), href, t), num_cell(fmt_gap(m["gap"]), href, t)]
                elif r.get("baseline"):
                    cells += ['<td colspan="2"><span class="lb-none" title="No age window holds at least the sample floor of both AI-tagged and untagged commits">no shared window</span></td>']
                else:
                    cells += ['<td colspan="2"><span class="lb-none" title="Measured before method v3: no baseline in this audit">—</span></td>']
        else:
            floor = r["coverage"]["sample_floor"]
            cells.append(f'<td><span class="lb-none" title="Fewer than {floor} AI-tagged commits: one commit can dominate, so no ratio is aggregated">n &lt; {floor}</span></td>')
        out.append("<tr>" + "".join(cells) + "</tr>")
    return "\n".join(out)


def agent_rows(by_agent: dict[str, Any]) -> str:
    return "\n".join(
        f"<tr><td>{esc(agent)}</td><td>{s['repositories']}</td><td>{fmt_int(s['commits'])}</td>"
        f"<td>{fmt_int(s['introduced'])}</td><td>{fmt_int(s['surviving'])}</td><td>{fmt_pct(s['survival_rate'])}</td></tr>"
        for agent, s in by_agent.items()
    )


def doi_html(f: dict[str, Any]) -> str:
    if f.get("doi"):
        return f'<a class="rp-doi" id="doi" href="https://doi.org/{esc(f["doi"])}" rel="noopener">DOI {esc(f["doi"])}</a>'
    return '<span class="rp-doi" id="doi">DOI: pending deposit</span>'


def reach_html(f: dict[str, Any]) -> str:
    r = f.get("reach")
    if not r:
        return ""
    pol = r.get("policy") or {}
    rows = "".join(
        f"<tr><td><code translate=\"no\">{esc(reach_destination_label(d))}</code></td><td>{esc(d.get('outcome'))}</td>"
        f"<td class=\"num\">{fmt_int(d.get('connections'))}</td><td class=\"num\">{fmt_int(d.get('bytes_out'))}</td>"
        f"<td class=\"num\">{fmt_int(d.get('bytes_in'))}</td></tr>"
        for d in r.get("destinations") or []
    )
    policy_html = ""
    if pol.get("hash"):
        policy_html = (
            f' The policy is <a href="{esc(pol["file"])}"><code translate="no">{esc(pol["file"])}</code></a>'
            if pol.get("file") else " The policy"
        ) + (f' (<a href="{esc(pol["source"])}" rel="noopener">source</a>), bound in the sheet as '
             f'<code translate="no">{esc(pol["hash"])}</code>.')
    verify_cmds = " or ".join(f'<code translate="no">{esc(c)}</code>' for c in r.get("verify") or [])
    warnings = ""
    if r.get("warnings"):
        warnings = "<p class=\"muted\">" + " ".join(esc(w) for w in r["warnings"]) + "</p>"
    return f"""
    <div class="rp-section" id="reach">
    <h3>Where this measurement connected</h3>
    <p class="muted">{esc(reach_sentence(r))} <strong>{esc(reach_verdict_sentence(r))}</strong>{policy_html}</p>
    <div class="tbl-scroll">
      <table class="lb-table">
        <thead><tr><th>Destination</th><th>Outcome</th><th>Connections</th><th>Bytes out</th><th>Bytes in</th></tr></thead>
        <tbody>{rows}</tbody>
      </table>
    </div>
    <p class="muted">Signed run sheet: <a href="{esc(r['sheet'])}"><code translate="no">{esc(r['sheet'])}</code></a> (profile <code translate="no">{esc(r['profile'])}</code>, witness <code translate="no">{esc((r.get('witness') or {}).get('id'))}</code>, capture <code translate="no">{esc(r.get('capture'))}</code>, disclosure <code translate="no">{esc(r.get('disclosure'))}</code>). Verify it offline with {verify_cmds}, or paste the sheet and the policy at <a href="{REACH_VERIFIER_URL}" rel="noopener">croviatrust.com/registry/seal/verify</a>.</p>
    {warnings}
    <p class="muted">Covered: {esc(r['covers'])}. Not covered: {esc(r['not_covered'])}. The record says where the measurement connected through the witness and how many bytes crossed, nothing about their content.</p>
    </div>"""


def render_report(f: dict[str, Any]) -> str:
    a = f["aggregate"]
    m = f["method"]
    ex = f["excluded"]
    url = f["url"]
    img = url + "card.png"
    title = f"Survival Report #{f['number']} · {f['date']}"
    if revision_of(f) > 1:
        title += f" · revision {revision_of(f)}"
    desc = headline(f) + " Counts, not grades; method public; every number reproducible."
    jsonld = {
        "@context": "https://schema.org", "@type": "Report", "name": title, "headline": headline(f), "url": url,
        "datePublished": f["date"], "dateModified": f.get("revised_at") or f["generated_at"], "inLanguage": "en", "image": img,
        "version": str(revision_of(f)),
        "author": {"@type": "Organization", "name": "Crovia Trust", "url": "https://croviatrust.com"},
        "publisher": {"@type": "Organization", "name": "Crovia Trust", "url": "https://croviatrust.com"},
        "license": LICENSE_URL,
        "isBasedOn": [r["url"] for r in f["repositories"]],
        "distribution": {"@type": "DataDownload", "encodingFormat": "application/json", "contentUrl": url + "report.json"},
    }
    if f.get("doi"):
        jsonld["identifier"] = {"@type": "PropertyValue", "propertyID": "DOI", "value": f["doi"]}
    b = a.get("baseline") or {}
    has_baseline = bool(b.get("repositories"))
    base_stat = ""
    if has_baseline and b.get("repositories_with_gap"):
        ivb = b.get("median_gap_interval_95")
        base_stat = f"""
      <a class="rp-stat" href="report.json#baseline"><span class="n">{fmt_gap(b['median_gap'])}</span><span class="l">median age-matched gap to the same repository's untagged lines{' · ' + fmt_gap(ivb['low']) + ' – ' + fmt_gap(ivb['high']) if ivb else ''}</span></a>"""
    baseline_section = ""
    if has_baseline:
        pooled_rows = "\n".join(
            f"<tr><td>{esc(window_label(w))}</td><td>{fmt_int(w['tagged']['commits'])}</td><td>{fmt_int(w['tagged']['introduced'])}</td>"
            f"<td>{fmt_int(w['tagged']['surviving'])}</td><td>{fmt_pct(w['tagged']['survival_rate'])}</td>"
            f"<td>{fmt_int(w['untagged']['commits'])}</td><td>{fmt_int(w['untagged']['introduced'])}</td>"
            f"<td>{fmt_int(w['untagged']['surviving'])}</td><td>{fmt_pct(w['untagged']['survival_rate'])}</td></tr>"
            for w in b.get("pooled_by_age") or [] if w["tagged"]["commits"] or w["untagged"]["commits"]
        )
        pooled_line = f"Age windows below: {b['pooled_by_age_note']}."
        gap_line = ""
        if b.get("repositories_with_gap"):
            ivb = b.get("median_gap_interval_95")
            gap_line = (f"{b['repositories_with_gap']} of the {b['repositories']} repositories with a baseline have an age-matched gap; "
                        f"the median is {fmt_gap(b['median_gap'])}"
                        + (f" (95 % bootstrap interval {fmt_gap(ivb['low'])} to {fmt_gap(ivb['high'])})" if ivb else "")
                        + f"; {b['gaps_negative']} gaps fall below zero and {b['gaps_positive']} above. ")
        rewritten_line = ""
        if b.get("rewritten"):
            rewritten_line = (f"<p class=\"muted\"><strong>Cleared or rewritten</strong> ({esc(b['rewritten_rule'])}): "
                              f"{esc(', '.join(b['rewritten']))}. Nothing from before that date survives in them, tagged or not; "
                              "their rows measure the rewrite as much as the code, and the gap is the figure to read.</p>")
        baseline_section = f"""
    <div class="rp-section" id="baseline">
    <h3>Baseline: the same repositories' untagged lines, in the matched age windows</h3>
    <p class="muted">{esc(b['definition'])}</p>
    <p>{esc(gap_line)}{esc(pooled_line)}</p>
    {rewritten_line}
    <div class="tbl-scroll wide">
      <table class="lb-table" id="by-age">
        <thead><tr><th>Line age</th><th>AI-tagged commits</th><th>Lines introduced</th><th>Still at HEAD</th><th>AI-tagged</th><th>Untagged commits</th><th>Lines introduced</th><th>Still at HEAD</th><th>Untagged</th></tr></thead>
        <tbody>
{pooled_rows}
        </tbody>
      </table>
    </div>
    </div>"""
    iv = a["survival_rate_interval_95"]
    iv2 = a["median_capped_survival_rate_interval_95"]
    strip = ""
    if a["repositories"]:
        strip = f"""
    <div class="rp-strip">
      <a class="rp-stat" href="report.json"><span class="n">{a['repositories']}</span><span class="l">repositories aggregated</span></a>
      <a class="rp-stat" href="report.json"><span class="n">{fmt_int(a['ai_tagged_commits'])}</span><span class="l">AI-tagged commits · of {fmt_int(a['total_commits'])}</span></a>
      <a class="rp-stat" href="report.json"><span class="n">{fmt_int(a['introduced'])}</span><span class="l">lines introduced by them</span></a>
      <a class="rp-stat" href="report.json"><span class="n">{fmt_int(a['surviving'])}</span><span class="l">still attributed to them at HEAD</span></a>
      <a class="rp-stat" href="report.json"><span class="n">{fmt_pct(a['survival_rate'])}</span><span class="l">line-weighted{' · 95 % interval ' + fmt_pct(iv['low']) + ' – ' + fmt_pct(iv['high']) if iv else ''}</span></a>
      <a class="rp-stat" href="report.json"><span class="n">{fmt_pct(a['median_capped_survival_rate'])}</span><span class="l">median of capped per-repository ratios{' · ' + fmt_pct(iv2['low']) + ' – ' + fmt_pct(iv2['high']) if iv2 else ''}</span></a>{base_stat}
    </div>"""
    not_agg = ""
    if f["not_aggregated"]:
        not_agg = f"""
    <div class="rp-section">
    <h3 id="not-aggregated">Measured but not aggregated</h3>
    <p class="muted">Fewer than {m['sample_floor']} AI-tagged commits: the counts are published, the ratio is not, and the repository is left out of the aggregate above.</p>
    <div class="tbl-scroll">
      <table class="lb-table">
        <thead><tr><th>Repository</th><th>Commits</th><th>AI-tagged</th><th>Lines introduced</th><th>Still at HEAD</th><th>Ratio</th></tr></thead>
        <tbody>
{repo_rows(f['not_aggregated'], full=False)}
        </tbody>
      </table>
    </div>
    </div>"""
    agents = ""
    if f["by_agent"]:
        agents = f"""
    <div class="rp-section">
    <h3 id="by-agent">By agent, across the aggregated repositories</h3>
    <p class="muted">Alphabetical. A commit is attributed to the agent its metadata names; one agent per commit.</p>
    <div class="tbl-scroll">
      <table class="lb-table">
        <thead><tr><th>Agent</th><th>Repositories</th><th>Commits</th><th>Lines introduced</th><th>Still at HEAD</th><th>Line-weighted</th></tr></thead>
        <tbody>
{agent_rows(f['by_agent'])}
        </tbody>
      </table>
    </div>
    </div>"""
    excluded_items = [
        f"<li><strong>Shallow clones</strong> (history truncated; method {esc(m['version'])} refuses them): {esc(', '.join(ex['shallow'])) if ex['shallow'] else 'none'}.</li>",
        f"<li><strong>Audits that failed</strong> in this run: {esc(', '.join(ex['failed'])) if ex['failed'] else 'none'}.</li>",
    ]
    if ex["not_method_v2"]:
        excluded_items.append(f"<li><strong>Outputs without a coverage block (method v1):</strong> {esc(', '.join(ex['not_method_v2']))}.</li>")
    for d in ex.get("duplicates") or []:
        excluded_items.append(
            f"<li><strong>One repository, two names:</strong> <code translate=\"no\">{esc(d['dropped'])}</code> is "
            f"<code translate=\"no\">{esc(d['kept'])}</code> ({esc(d['reason'])}); counted once, under the second name.</li>"
        )
    excluded_items.append(
        f'<li><strong>Opted out</strong> by their maintainers: {ex["opted_out"]}. One line in '
        f'<a href="{REPO_URL}/edit/main/.github/survival-optout.txt" rel="noopener"><code translate="no">.github/survival-optout.txt</code></a> '
        "removes a repository from the next report, no questions asked.</li>"
    )
    reach_section = reach_html(f)
    corrections = ""
    if f.get("corrections"):
        items = "".join(f"<li>{esc(c)}</li>" for c in correction_lines(f))
        prev_links = " · ".join(
            f'<a href="{esc(c["previous"]["file"])}">revision {int(c["revision"]) - 1}</a>'
            for c in sorted(f["corrections"], key=lambda c: -int(c["revision"])) if (c.get("previous") or {}).get("file")
        )
        corrections = f"""
    <div class="rp-section rp-corrections" id="corrections">
    <h3>Corrections</h3>
    <p class="muted">This is revision {revision_of(f)} of report #{f['number']}. A report is never edited in place: every superseded revision keeps its bytes and its DOI next to this page ({prev_links}), and what changed is stated here.</p>
    <ul class="rp-list">{items}</ul>
    </div>"""
    eyebrow_rev = f" · revision {revision_of(f)}" if revision_of(f) > 1 else ""
    body = f"""
<section class="section">
  <div class="container">
    <div class="section-head">
      <p class="eyebrow">survival report #{f['number']} · {esc(f['date'])}{eyebrow_rev} · method {esc(m['version'])} · {esc(f['tool']['name'])} {esc(f['tool']['version'])} · unranked</p>
      <h1>Survival Report #{f['number']}</h1>
      <p class="lede">{esc(headline(f))} <strong>These are counts, not grades.</strong> There is no rank, no colour and no verdict on this page; rows are alphabetical. Every number links to the audit bytes behind it and the <a href="/method">method and its limits</a> are public.</p>
      <p class="rp-meta">{doi_html(f)} · <a href="report.json">report.json</a> · <a href="report.md">report.md</a> · <a href="card.png">card</a> · <a href="/{REPORTS_REL}/feed.xml">Atom feed</a> · <a href="/{REPORTS_REL}/">all reports</a></p>
    </div>
    <img class="rp-card" src="card.png" alt="Survival Report #{f['number']} card" width="1200" height="630" loading="lazy" />
{corrections}
{strip}
    <div class="rp-section">
    <h3 id="repositories">Repositories</h3>
    <p class="muted">Alphabetical. VERIFIED commits only; PROBABLE counts are shown but never summed. <em>Capped</em>: no commit weighs more than the cap. <em>Median per commit</em>: the middle commit's own ratio. <em>Largest commit</em>: share of introduced lines from the single largest commit.{' <em>Untagged, matched windows</em>: the survival of the repository&#39;s own untagged lines, re-weighted to the age mix of its AI-tagged lines inside matched age windows; <em>Gap</em>: the AI-tagged ratio minus that, in points (definition in the <a href="#baseline">Baseline</a> section).' if has_baseline else ''} Every number links to the audit bytes of this run; <code translate="no">{esc(m['command'])}</code> reproduces a row. Each repository name links to <a href="/{REPOS_REL}/">its own page</a>: history across reports and a badge.</p>
    <div class="tbl-scroll wide">
      <table class="lb-table" id="repos">
        <thead><tr><th>Repository</th><th>Commits</th><th>AI-tagged</th><th>Lines introduced</th><th>Still at HEAD</th><th>Line-weighted</th><th>Capped</th><th>Median per commit</th><th>Largest commit</th>{'<th>Untagged, matched windows</th><th>Gap</th>' if has_baseline else ''}</tr></thead>
        <tbody>
{repo_rows(f['repositories'], full=True, baseline=has_baseline)}
        </tbody>
      </table>
    </div>
    </div>
{baseline_section}
{not_agg}
{agents}
    <div class="rp-section">
    <h3 id="excluded">Excluded from this report</h3>
    <ul class="rp-list">
      {''.join(excluded_items)}
    </ul>
    </div>
{reach_section}
    <div class="proof rp-section" id="method">
      <h3>Method</h3>
      <ul>
        <li><strong>Method {esc(m['version'])}</strong>, {esc(f['tool']['name'])} {esc(f['tool']['version'])}. Detection from commit metadata only; no model, no guess from the diff. Full text and known artefacts at <a href="/method">causari.dev/method</a>.</li>
        <li><strong>Survival</strong>: <code translate="no">git blame {esc(' '.join(m['blame_flags']))}</code> at HEAD, honouring <code translate="no">.git-blame-ignore-revs</code> where present; a line counts for the commit blame attributes it to, capped at that commit's introduced count.</li>
        <li><strong>Cap rule</strong>: {esc(m['cap_rule'])}. The capped ratio is what one bulk commit cannot dominate.</li>
        <li><strong>Sample floor</strong>: {m['sample_floor']} VERIFIED commits. Below it a repository is measured but not aggregated.</li>
        <li><strong>Intervals</strong>: {esc(a['interval_method']['note'])}</li>
        <li><strong>Full clones only</strong>: method {esc(m['version'])} refuses shallow clones; the workflow clones each repository completely before measuring.</li>{'<li><strong>Baseline</strong> (method v3): every UNKNOWN commit of a repository, human-written, inline-completed or untagged-agent code alike, forms its untagged cohort; the by-age table and the age-matched gap put it next to the AI-tagged one. The untagged cohort includes AI code that carried no tag.</li>' if has_baseline else ''}
        <li><strong>Reproduce or contest</strong>: <code translate="no">{esc(m['command'])}</code> gives the exact bytes behind a row; the bytes of this run are under <code translate="no">repos/</code> next to this page. Open an issue with your JSON if it differs.</li>
      </ul>
    </div>

    <p class="rp-cite">Cite as: <code translate="no">{esc(cite(f))}</code></p>

    {positioning_html(report_selection(f))}
  </div>
</section>
"""
    return page_head(title, desc, url, img, jsonld) + body + page_foot()


def _aggregated_names(report: dict[str, Any]) -> frozenset[str] | None:
    """Lowercased names of the repositories inside the aggregate. None when
    the report does not carry that list: the count alone cannot prove the
    sample is the same one."""
    rows = report.get("repositories")
    if not isinstance(rows, list):
        return None
    names: list[str] = []
    for row in rows:
        if not isinstance(row, dict) or not row.get("repo"):
            return None
        names.append(str(row["repo"]).lower())
    return frozenset(names)


def archive_rate_note(archive: list[dict[str, Any]]) -> str:
    """The line-weighted column of the archive. Empty only when every report
    aggregated the same repositories under the same method: that is the one
    case in which the column repeats one measurement. A different count, a
    different set or a different method is not a series, and the sentence
    names the counts so a reader cannot treat 43 and 61 as one sample."""
    if len(archive) < 2:
        return ""
    parts: list[str] = []
    counts: list[int] = []
    methods: list[str] = []
    sets: list[frozenset[str] | None] = []
    for report in archive:
        n = int(report["aggregate"]["repositories"])
        method = str(report["method"]["version"])
        counts.append(n)
        methods.append(method)
        sets.append(_aggregated_names(report))
        noun = "repository" if n == 1 else "repositories"
        parts.append(f"#{report['number']} aggregated {n} {noun} under method {method}")
    same_method = len(set(methods)) == 1
    same_count = len(set(counts)) == 1
    same_set = all(s is not None for s in sets) and len(set(sets)) == 1 and all(len(s) == counts[0] for s in sets)
    if same_method and same_count and same_set:
        return ""
    if same_method and same_count:
        why = ("The repository counts match, but the repositories are not the same set, "
               "so the rates do not measure one sample over time.")
    elif not same_method and not same_count:
        why = "A different repository count and a different method mean the rates do not measure the same thing over time."
    elif not same_count:
        why = "A different repository count means the rates do not measure the same sample over time."
    else:
        why = "A different method means the rates do not measure the same thing over time."
    return (
        "The line-weighted column is not a series. Each rate is lines still at HEAD divided by lines introduced, "
        f"inside that report's own aggregated repositories. {'; '.join(parts)}. {why} "
        "A repository followed across reports is on its page."
    )


def render_index(archive: list[dict[str, Any]]) -> str:
    url = f"{SITE_URL}/{REPORTS_REL}/"
    title = "Survival Report · weekly"
    latest = archive[0] if archive else None
    desc = ("Weekly, numbered, citable: how many lines from AI-tagged commits are still at HEAD in open-source repositories. "
            "Counts and intervals, no ranks, no verdicts. Every number reproducible with re audit <owner/repo> --json.")
    jsonld = {"@context": "https://schema.org", "@type": "CollectionPage", "name": title, "url": url, "description": desc,
              "publisher": {"@type": "Organization", "name": "Crovia Trust", "url": "https://croviatrust.com"},
              "hasPart": [{"@type": "Report", "name": f"Survival Report #{a['number']}", "url": a["url"], "datePublished": a["date"]} for a in archive[:52]]}
    rows = "\n".join(
        f'<tr><td><a href="/{REPORTS_REL}/{esc(a["id"])}/">Survival Report #{a["number"]}</a>{(" <a class=\"muted\" href=\"/" + REPORTS_REL + "/" + esc(a["id"]) + "/#corrections\">rev. " + str(revision_of(a)) + "</a>") if revision_of(a) > 1 else ""}</td><td>{esc(a["date"])}</td>'
        f'<td>{a["aggregate"]["repositories"]}</td><td>{fmt_int(a["aggregate"]["ai_tagged_commits"])}</td>'
        f'<td>{fmt_int(a["aggregate"]["introduced"])}</td><td>{fmt_int(a["aggregate"]["surviving"])}</td>'
        f'<td>{fmt_pct(a["aggregate"]["survival_rate"])}</td><td class="txt">{esc(a["method"]["version"])}</td>'
        f'<td class="txt">{("<a href=\"https://doi.org/" + esc(a["doi"]) + "\" rel=\"noopener\">" + esc(a["doi"]) + "</a>") if a.get("doi") else "<span class=\"muted\">pending</span>"}</td></tr>'
        for a in archive
    )
    rate_note = archive_rate_note(archive)
    rate_html = f'\n    <p class="muted" id="archive-rates">{esc(rate_note)}</p>' if rate_note else ""
    latest_block = ""
    if latest:
        latest_block = f"""
    <a href="/{REPORTS_REL}/{esc(latest['id'])}/"><img class="rp-card" src="/{REPORTS_REL}/{esc(latest['id'])}/card.png" alt="Survival Report #{latest['number']} card" width="1200" height="630" /></a>
    <p class="rp-latest">Latest: <a href="/{REPORTS_REL}/{esc(latest['id'])}/">Survival Report #{latest['number']}</a> · {esc(latest['date'])} — {esc(headline(latest))}</p>
    <p><a class="btn btn-primary" href="/{REPORTS_REL}/{esc(latest['id'])}/">Read Survival Report #{latest['number']}</a></p>"""
    body = f"""
<section class="section">
  <div class="container">
    <div class="section-head">
      <p class="eyebrow">measured weekly · git metadata only · unranked · atom feed</p>
      <h1>Survival Report</h1>
      <p class="lede">Every week, one numbered report: for each measured open-source repository, how many lines were introduced by commits that carry machine-readable AI authorship metadata, and how many of those lines <code translate="no">git blame</code> still attributes to them at HEAD. AI-tagged means that metadata matched; it does not prove a model wrote the line. <strong>These are counts, not grades.</strong> No rank, no colour, no verdict; rows are alphabetical; intervals describe the sampled repositories only. Every number is reproducible with one command, the <a href="/method">method and its limits</a> are public, and the <a href="https://github.com/croviatrust/causari#readme">README</a> says what the audit, the local ledger and a Seal each do.</p>
      <p class="rp-meta"><a href="/{REPORTS_REL}/feed.xml">Atom feed</a> · <a href="/{REPORTS_REL}/latest.json">latest.json</a> · <a href="{REPO_URL}/blob/main/docs/survival-report.md" rel="noopener">how it is made</a> · <a href="{REPO_URL}/edit/main/.github/survival-optout.txt" rel="noopener">opt out</a> (<code translate="no">.github/survival-optout.txt</code>)</p>
    </div>
{latest_block}
    <div class="rp-section">
    <h3 id="archive">All reports</h3>
    <div class="tbl-scroll wide">
      <table class="lb-table" id="archive-table">
        <thead><tr><th>Report</th><th>Date</th><th>Repositories</th><th>AI-tagged commits</th><th>Lines introduced</th><th>Still at HEAD</th><th>Line-weighted</th><th class="txt">Method</th><th class="txt">DOI</th></tr></thead>
        <tbody>
{rows}
        </tbody>
      </table>
    </div>{rate_html}
    <p class="muted">The report replaced the weekly measurements table in September 2026. Repositories enter <a href="{REPO_URL}/blob/main/.github/survival-repos.txt" rel="noopener"><code translate="no">.github/survival-repos.txt</code></a> by pull request or through the weekly discovery, which lists the most-starred public repositories where GitHub commit search finds at least five commits carrying AI authorship metadata (<a href="/method#selection">how repositories are selected</a>); maintainers opt out with one line in <a href="{REPO_URL}/edit/main/.github/survival-optout.txt" rel="noopener"><code translate="no">.github/survival-optout.txt</code></a>.</p>
    <p><a href="/{REPOS_REL}/">Every repository has a page and a badge</a>: its counts across reports, the exact bytes behind each number, and a README badge that follows the latest report.</p>
    </div>

    {positioning_html(SELECTION_SERIES)}
  </div>
</section>
"""
    image = f"{SITE_URL}/{REPORTS_REL}/{latest['id']}/card.png" if latest else f"{SITE_URL}/assets/og.png"
    return page_head(title, desc, url, image, jsonld) + body + page_foot()


def render_feed(archive: list[dict[str, Any]]) -> str:
    feed_url = f"{SITE_URL}/{REPORTS_REL}/feed.xml"
    updated = max((a.get("revised_at") or a["generated_at"] for a in archive), default=None) if archive else dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    entries = []
    for a in archive[:104]:
        summary = headline(a) + " Counts, not grades. " + LICENSE + "."
        if a.get("doi"):
            summary += f" DOI {a['doi']}."
        if a.get("corrections"):
            summary += " " + " ".join(correction_lines(a))
        entries.append(f"""  <entry>
    <title>Survival Report #{a['number']} · {esc(a['date'])}</title>
    <link rel="alternate" type="text/html" href="{esc(a['url'])}"/>
    <link rel="enclosure" type="image/png" href="{esc(a['url'])}card.png"/>
    <link rel="related" type="application/json" href="{esc(a['url'])}report.json"/>
    <id>{esc(a['url'])}</id>
    <published>{esc(a['date'])}T00:00:00Z</published>
    <updated>{esc(a.get('revised_at') or a['generated_at'])}</updated>
    <summary>{esc(summary)}</summary>
  </entry>""")
    return f"""<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>causari — Survival Report</title>
  <subtitle>Weekly counts of surviving lines from AI-tagged commits in open-source repositories. No ranks, no verdicts. {LICENSE}.</subtitle>
  <link rel="self" type="application/atom+xml" href="{feed_url}"/>
  <link rel="alternate" type="text/html" href="{SITE_URL}/{REPORTS_REL}/"/>
  <id>{SITE_URL}/{REPORTS_REL}/</id>
  <updated>{esc(updated)}</updated>
  <author><name>Crovia Trust</name><uri>https://croviatrust.com</uri></author>
  <rights>{LICENSE} Crovia Trust</rights>
  <icon>{SITE_URL}/assets/favicon.svg</icon>
{chr(10).join(entries)}
</feed>
"""


# ----------------------------------------------------------------------------- repository pages

def measured_repos(archive: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """One entry per repository with a measured row (aggregated or not) in any
    report, alphabetical by lowercase name. `history` runs oldest to newest;
    the name is spelt as the latest report measured it."""
    entries: dict[str, dict[str, Any]] = {}
    for f in sorted(archive, key=lambda a: (a["number"], a["date"])):
        for row in list(f.get("repositories") or []) + list(f.get("not_aggregated") or []):
            key = row["repo"].lower()
            e = entries.setdefault(key, {"key": key, "history": []})
            e["repo"] = row["repo"]
            e["history"].append({"report": f, "row": row})
    out = sorted(entries.values(), key=lambda e: e["key"])
    for e in out:
        e["latest"] = e["history"][-1]
        e["path"] = repo_path(e["repo"])
        e["url"] = SITE_URL + e["path"]
    return out


def bytes_href(f: dict[str, Any], row: dict[str, Any]) -> str:
    return f"/{REPORTS_REL}/{f['id']}/{row['audit_file']}"


def ratio_text(row: dict[str, Any]) -> str:
    """The line-weighted ratio, or the sample-floor marker the report uses
    instead when there are too few AI-tagged commits to publish one."""
    if row["coverage"]["small_sample"]:
        return f"n < {row['coverage']['sample_floor']}"
    return fmt_pct(row["verified"]["survival_rate"])


def repo_sentence(e: dict[str, Any]) -> str:
    f, row = e["latest"]["report"], e["latest"]["row"]
    v = row["verified"]
    s = (f"In Survival Report #{f['number']} ({f['date']}, method {f['method']['version']}): "
         f"{fmt_int(v['surviving'])} of {fmt_int(v['introduced'])} lines introduced by {fmt_int(v['commits'])} "
         f"AI-tagged commits are still at HEAD")
    if row["coverage"]["small_sample"]:
        return s + f". Fewer than {row['coverage']['sample_floor']} AI-tagged commits: the counts are published, no ratio is aggregated."
    return s + f", {fmt_pct(v['survival_rate'])}."


BADGE_FONT = "ui-monospace,'JetBrains Mono','SF Mono','Cascadia Mono',Menlo,Consolas,'Liberation Mono',monospace"
BADGE_CHAR = 6.6   # advance of one monospace glyph at 11 px; textLength pins it in every renderer
BADGE_PAD = 8
BADGE_H = 20


def badge_texts(e: dict[str, Any]) -> tuple[str, str]:
    f, row = e["latest"]["report"], e["latest"]["row"]
    return f"AI code survival  {ratio_text(row)}", f"causari · #{f['number']}"


def badge_width(e: dict[str, Any]) -> tuple[int, int]:
    left, right = badge_texts(e)
    return round(len(left) * BADGE_CHAR + 2 * BADGE_PAD), round(len(right) * BADGE_CHAR + 2 * BADGE_PAD)


def badge_svg(e: dict[str, Any], dark: bool = False) -> str:
    """Text-only badge in the identity: ink on paper (or paper on ink), one
    monospace line, the report number in a small right segment. No third
    colour anywhere, so no red or green can ever mean good or bad."""
    left, right = badge_texts(e)
    lw, rw = badge_width(e)
    w = lw + rw
    fg, bg = (PAPER, INK) if dark else (INK, PAPER)
    title = f"{e['repo']}: {repo_sentence(e)} causari.dev"
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{BADGE_H}" viewBox="0 0 {w} {BADGE_H}" role="img" aria-label="{esc(title)}">
  <title>{esc(title)}</title>
  <rect width="{w}" height="{BADGE_H}" rx="3" fill="{fg}"/>
  <rect x="1" y="1" width="{lw - 1}" height="{BADGE_H - 2}" rx="2" fill="{bg}"/>
  <g font-family="{BADGE_FONT}" font-size="11" text-rendering="geometricPrecision">
    <text x="{BADGE_PAD}" y="14" fill="{fg}" textLength="{lw - 2 * BADGE_PAD}" lengthAdjust="spacingAndGlyphs" xml:space="preserve">{esc(left)}</text>
    <text x="{lw + BADGE_PAD}" y="14" fill="{bg}" textLength="{rw - 2 * BADGE_PAD}" lengthAdjust="spacingAndGlyphs" xml:space="preserve">{esc(right)}</text>
  </g>
</svg>
"""


def badge_markdown(e: dict[str, Any]) -> str:
    return f"[![AI code survival]({e['url']}badge.svg)]({e['url']})"


def sparkline_svg(points: list[tuple[str, float | None]]) -> str:
    """Inline one-colour sparkline of a ratio across reports on a fixed
    0 – 100 % axis. Empty with fewer than two values: one point is not a line."""
    vals = [(label, v) for label, v in points if v is not None]
    if len(vals) < 2:
        return ""
    w, h, pad = 160, 36, 4
    step = (w - 2 * pad) / (len(vals) - 1)
    pts = [(pad + i * step, pad + (1 - min(max(v, 0.0), 1.0)) * (h - 2 * pad)) for i, (_, v) in enumerate(vals)]
    poly = " ".join(f"{x:.1f},{y:.1f}" for x, y in pts)
    dots = "".join(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="2"/>' for x, y in pts)
    title = "Line-weighted ratio across reports, 0 to 100 %: " + "; ".join(f"#{label} {fmt_pct(v)}" for label, v in vals)
    return (f'<svg class="spark" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" aria-label="{esc(title)}">'
            f"<title>{esc(title)}</title>"
            f'<polyline points="{poly}" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round"/>'
            f'<g fill="currentColor">{dots}</g></svg>')


def repo_latest(e: dict[str, Any]) -> dict[str, Any]:
    f, row = e["latest"]["report"], e["latest"]["row"]
    v = row["verified"]
    return {
        "schema": REPO_SCHEMA,
        "repo": e["repo"],
        "repository_url": row["url"],
        "url": e["url"],
        "badge": e["url"] + "badge.svg",
        "report": {"number": f["number"], "id": f["id"], "date": f["date"], "url": f["url"]},
        "method": f["method"]["version"],
        "tool": f["tool"],
        "verified": {
            "commits": v["commits"], "introduced": v["introduced"], "surviving": v["surviving"],
            "survival_rate": v["survival_rate"], "capped_survival_rate": v["capped_survival_rate"],
            "median_survival": v["median_survival"], "largest_commit_share": v["largest_commit_share"],
        },
        "interval_95": v.get("survival_rate_interval_95"),
        "baseline": row.get("baseline"),
        "probable_commits": row["probable"]["commits"],
        "total_commits": row["total_commits"],
        "aggregated": bool(row.get("aggregated")),
        "coverage": row["coverage"],
        "reports": len(e["history"]),
        "reproduce": row["reproduce"],
        "bytes": SITE_URL + bytes_href(f, row),
        "license": LICENSE,
    }


def render_repo_page(e: dict[str, Any]) -> str:
    f, row = e["latest"]["report"], e["latest"]["row"]
    v, p, cov = row["verified"], row["probable"], row["coverage"]
    m = f["method"]
    repo = e["repo"]
    url = e["url"]
    href = bytes_href(f, row)
    sentence = repo_sentence(e)
    title = f"{repo} · AI code survival"
    desc = f"{sentence} Counts, not grades; every number links to the audit bytes and the method is public."
    img = f["url"] + "card.png"
    jsonld = {
        "@context": "https://schema.org", "@type": "Dataset", "name": title, "description": sentence, "url": url,
        "about": {"@type": "SoftwareSourceCode", "name": repo, "codeRepository": row["url"]},
        "isPartOf": {"@type": "Report", "name": f"Survival Report #{f['number']}", "url": f["url"], "datePublished": f["date"]},
        "datePublished": f["date"], "dateModified": f["generated_at"], "inLanguage": "en", "license": LICENSE_URL,
        "creator": {"@type": "Organization", "name": "Crovia Trust", "url": "https://croviatrust.com"},
        "measurementTechnique": f"{row['reproduce']} (method {m['version']}, git metadata only)",
        "distribution": [
            {"@type": "DataDownload", "encodingFormat": "application/json", "contentUrl": url + "latest.json"},
            {"@type": "DataDownload", "encodingFormat": "application/json", "contentUrl": SITE_URL + href},
        ],
    }
    small = cov["small_sample"]
    ratio_label = "line-weighted" if not small else f"fewer than {cov['sample_floor']} AI-tagged commits: no ratio aggregated"
    strip = f"""
    <div class="rp-strip">
      <a class="rp-stat" href="{esc(href)}"><span class="n">{fmt_int(v['commits'])}</span><span class="l">AI-tagged commits · of {fmt_int(row['total_commits'])}</span></a>
      <a class="rp-stat" href="{esc(href)}"><span class="n">{fmt_int(v['introduced'])}</span><span class="l">lines introduced by them</span></a>
      <a class="rp-stat" href="{esc(href)}"><span class="n">{fmt_int(v['surviving'])}</span><span class="l">still attributed to them at HEAD</span></a>
      <a class="rp-stat" href="{esc(href)}"><span class="n">{esc(ratio_text(row))}</span><span class="l">{ratio_label}</span></a>
      <a class="rp-stat" href="{esc(href)}"><span class="n">{'—' if small else fmt_pct(v['median_survival'])}</span><span class="l">median per commit{' · not published below the floor' if small else ''}</span></a>
      <a class="rp-stat" href="{esc(href)}"><span class="n">{fmt_share(v['largest_commit_share'])}</span><span class="l">of introduced lines in the largest commit</span></a>
    </div>"""
    agent_cells = []
    for agent, s in row["by_agent"].items():
        ratio = esc(ratio_text(row)) if small else fmt_pct(s["survival_rate"])
        capped = "—" if small else fmt_pct(s["capped_survival_rate"])
        agent_cells.append(f"<tr><td>{esc(agent)}</td><td>{fmt_int(s['commits'])}</td><td>{fmt_int(s['introduced'])}</td>"
                           f"<td>{fmt_int(s['surviving'])}</td><td>{ratio}</td><td>{capped}</td></tr>")
    agents = ""
    if agent_cells:
        agents = f"""
    <div class="rp-section">
    <h3 id="by-agent">By agent, in Survival Report #{f['number']}</h3>
    <p class="muted">Alphabetical. A commit is attributed to the agent its metadata names; one agent per commit. VERIFIED commits only.</p>
    <div class="tbl-scroll">
      <table class="lb-table" id="agents">
        <thead><tr><th>Agent</th><th>Commits</th><th>Lines introduced</th><th>Still at HEAD</th><th>Line-weighted</th><th>Capped</th></tr></thead>
        <tbody>
{chr(10).join(agent_cells)}
        </tbody>
      </table>
    </div>
    </div>"""
    baseline = ""
    bl = row.get("baseline")
    if bl:
        u = bl["untagged"]
        am = bl.get("age_matched")
        o = bl.get("oldest_surviving")
        if am and am.get("gap") is not None:
            gap_text = (f"Age-matched: AI-tagged lines {fmt_pct(am['tagged_rate'])} against untagged lines in the matched age windows "
                        f"{fmt_pct(am['untagged_rate'])}, a gap of {fmt_gap(am['gap'])}, over {am['buckets_used']} age window"
                        f"{'s' if am['buckets_used'] != 1 else ''} holding {fmt_share(am['tagged_lines_covered'])} of the AI-tagged lines.")
        else:
            gap_text = f"No age window holds at least {cov['sample_floor']} commits of both kinds, so there is no age-matched figure."
        rewrite_text = ""
        if o and o.get("commits_before"):
            rewrite_text = (f" The oldest line still at HEAD dates {esc(o['date'])}; {fmt_int(o['commits_before'])} commits "
                            f"({fmt_int(o['tagged_commits_before'])} AI-tagged, {fmt_int(o['introduced_before'])} lines) are older, "
                            "and nothing from before that date survives, tagged or not"
                            + (": more than half of this repository's commits, so it was cleared or rewritten and every ratio on this page measures that as much as the code." if rewritten(row) else "."))
        age_rows = "\n".join(
            f"<tr><td>{esc(window_label(w))}</td>"
            + (f"<td>{fmt_pct(w['tagged']['survival_rate'])}</td><td>{fmt_int(w['tagged']['commits'])}</td>" if w["tagged"]["commits"] else "<td>—</td><td>0</td>")
            + (f"<td>{fmt_pct(w['untagged']['survival_rate'])}</td><td>{fmt_int(w['untagged']['commits'])}</td>" if w["untagged"]["commits"] else "<td>—</td><td>0</td>")
            + "</tr>"
            for w in bl["by_age"] if w["tagged"]["commits"] or w["untagged"]["commits"]
        )
        baseline = f"""
    <div class="rp-section" id="baseline">
    <h3>Baseline: this repository's untagged lines</h3>
    <p class="muted">Commits with no machine-readable AI signal, human-written, inline-completed or untagged-agent code alike: {fmt_int(u['commits'])} commits, {fmt_int(u['introduced'])} lines introduced, {fmt_int(u['surviving'])} still at HEAD ({fmt_pct(u['survival_rate'])} line-weighted, {fmt_pct(u['median_survival'])} median per commit). Age is the time from a commit to HEAD. The baseline was added in method v3 and is unchanged later; definition at <a href="/method#v3">causari.dev/method</a>.</p>
    <p>{esc(gap_text)}{rewrite_text}</p>
    <div class="tbl-scroll">
      <table class="lb-table" id="by-age">
        <thead><tr><th>Line age</th><th>AI-tagged</th><th>commits</th><th>Untagged</th><th>commits</th></tr></thead>
        <tbody>
{age_rows}
        </tbody>
      </table>
    </div>
    </div>"""
    history_rows = []
    points: list[tuple[str, float | None]] = []
    for h in e["history"]:
        hf, hr = h["report"], h["row"]
        hv = hr["verified"]
        hb = bytes_href(hf, hr)
        points.append((str(hf["number"]), None if hr["coverage"]["small_sample"] else hv["survival_rate"]))
        history_rows.append(
            f'<tr><td><a href="/{REPORTS_REL}/{esc(hf["id"])}/">Survival Report #{hf["number"]}</a></td><td>{esc(hf["date"])}</td>'
            + num_cell(fmt_int(hv["commits"]), hb, hr["reproduce"]) + num_cell(fmt_int(hv["introduced"]), hb, hr["reproduce"])
            + num_cell(fmt_int(hv["surviving"]), hb, hr["reproduce"]) + num_cell(esc(ratio_text(hr)), hb, hr["reproduce"]) + "</tr>"
        )
    spark = sparkline_svg(points)
    spark_note = (" The line is the line-weighted ratio on a fixed 0 – 100 % axis; each point is one report."
                  if spark else " A line appears once the repository has been measured in two reports.")
    probable = (f"{fmt_int(p['commits'])} PROBABLE commits ({fmt_int(p['introduced'])} lines introduced, {fmt_int(p['surviving'])} still at HEAD) "
                "are listed here and never summed into the VERIFIED counts." if p["commits"] else "No PROBABLE commits: every counted commit carries machine-readable AI authorship metadata.")
    coverage_items = [
        f"<li><strong>Method {esc(cov['method'] or m['version'])}</strong>, {esc(f['tool']['name'])} {esc(f['tool']['version'])}. Detection from commit metadata only; survival from <code translate=\"no\">git blame {esc(' '.join(cov['blame_flags'] or m['blame_flags']))}</code> at HEAD.</li>",
        f"<li><strong>Full clone</strong>: {'yes' if not cov['shallow'] else 'no'}. Method {esc(m['version'])} refuses shallow clones; this row comes from a complete history.</li>",
        f"<li><strong>Sample</strong>: {fmt_int(v['commits'])} AI-tagged commits; the sample floor is {cov['sample_floor']}. "
        + (f"Below the floor, so the counts are published and no ratio is aggregated into the report." if cov["small_sample"] else "At or above the floor, so the ratio is aggregated into the report.") + "</li>",
        f"<li><strong>PROBABLE</strong>: {probable}</li>",
        f"<li><strong>Cap</strong>: {fmt_int(v['cap_lines']) if v['cap_lines'] is not None else '—'} lines per commit in this repository; the capped ratio is {fmt_pct(v['capped_survival_rate']) if not cov['small_sample'] else 'not published below the floor'}.</li>",
        f"<li><strong><code translate=\"no\">.git-blame-ignore-revs</code></strong>: {'honoured' if cov['ignore_revs_file'] else 'not present in this repository'}.</li>",
    ]
    body = f"""
<section class="section">
  <div class="container">
    <div class="section-head">
      <p class="eyebrow"><a href="/{REPOS_REL}/">repository</a> · survival report #{f['number']} · {esc(f['date'])} · method {esc(m['version'])} · unranked</p>
      <h1><a href="{esc(row['url'])}" rel="noopener" translate="no">{esc(repo)}</a></h1>
      <p class="lede">{esc(sentence)} <strong>These are counts, not grades.</strong> No rank, no colour, no verdict, and no comparison with any other repository on this page. Every number links to the audit bytes behind it and the <a href="/method">method and its limits</a> are public.</p>
      <p class="rp-meta"><a href="{esc(row['url'])}" rel="noopener">GitHub</a> · <a href="{esc(e['path'])}latest.json">latest.json</a> · <a href="{esc(e['path'])}badge.svg">badge.svg</a> · <a href="/{REPORTS_REL}/{esc(f['id'])}/">Survival Report #{f['number']}</a> · <a href="/{REPOS_REL}/">all repositories</a> · <a href="/{REPORTS_REL}/feed.xml">Atom feed</a></p>
    </div>
{strip}
{baseline}
{agents}
    <div class="rp-section">
    <h3 id="history">History across reports</h3>
    <p class="muted">One row per report this repository was measured in, oldest first; each report measured HEAD as of its own date.{spark_note}</p>
    {spark}
    <div class="tbl-scroll">
      <table class="lb-table" id="history-table">
        <thead><tr><th>Report</th><th>Date</th><th>AI-tagged commits</th><th>Lines introduced</th><th>Still at HEAD</th><th>Line-weighted</th></tr></thead>
        <tbody>
{chr(10).join(history_rows)}
        </tbody>
      </table>
    </div>
    </div>

    <div class="proof rp-section" id="reproduce">
      <h3>Reproduce</h3>
      <p>One command, any machine, no account: the same bytes the report was built from. The exact bytes behind this page are <a href="{esc(href)}"><code translate="no">{esc(row['audit_file'])}</code></a> of Survival Report #{f['number']}. Open an issue with your JSON if it differs.</p>
<pre translate="no"><code translate="no">{esc(row['reproduce'])}   <span class="dim"># method {esc(m['version'])}, full clone</span></code></pre>
    </div>

    <div class="rp-section" id="badge">
    <h3>Badge</h3>
    <p class="muted">Text only, one colour, no red and no green. It follows the latest report and is cached for one hour. <code translate="no">badge-dark.svg</code> is the same badge as paper on ink.</p>
    <p class="rp-badges"><img src="{esc(e['path'])}badge.svg" alt="{esc(badge_texts(e)[0])} · {esc(badge_texts(e)[1])}" width="{sum(badge_width(e))}" height="{BADGE_H}" /> <img src="{esc(e['path'])}badge-dark.svg" alt="{esc(badge_texts(e)[0])} · {esc(badge_texts(e)[1])}, paper on ink" width="{sum(badge_width(e))}" height="{BADGE_H}" /></p>
    <div class="term">
      <div class="term-head"><span>README · Markdown</span><button class="copy" data-copy="badge-md" aria-label="Copy the badge Markdown">copy</button></div>
<pre translate="no" id="badge-md"><code translate="no">{esc(badge_markdown(e))}</code></pre>
    </div>
    </div>

    <div class="rp-section">
    <h3 id="coverage">Coverage</h3>
    <ul class="rp-list">
      {''.join(coverage_items)}
    </ul>
    </div>
  </div>
</section>
"""
    return page_head(title, desc, url, img, jsonld) + body + page_foot()


def render_repo_index(entries: list[dict[str, Any]]) -> str:
    url = f"{SITE_URL}/{REPOS_REL}/"
    title = "Measured repositories · AI code survival"
    desc = ("Every repository measured in the Survival Report has a page and a badge: counts across reports, the exact bytes "
            "behind each number, no rank. Counts, not grades; every number reproducible with re audit <owner/repo> --json.")
    jsonld = {"@context": "https://schema.org", "@type": "CollectionPage", "name": title, "url": url, "description": desc,
              "publisher": {"@type": "Organization", "name": "Crovia Trust", "url": "https://croviatrust.com"},
              "hasPart": [{"@type": "Dataset", "name": f"{e['repo']} · AI code survival", "url": e["url"]} for e in entries]}
    rows = "\n".join(
        f'<tr><td><a href="{esc(e["path"])}" translate="no">{esc(e["repo"])}</a></td>'
        f'<td>{fmt_int(e["latest"]["row"]["verified"]["commits"])}</td>'
        f'<td>{fmt_int(e["latest"]["row"]["verified"]["introduced"])}</td>'
        f'<td>{fmt_int(e["latest"]["row"]["verified"]["surviving"])}</td>'
        f'<td>{esc(ratio_text(e["latest"]["row"]))}</td>'
        f'<td><a class="rp-num" href="/{REPORTS_REL}/{esc(e["latest"]["report"]["id"])}/" title="Survival Report #{e["latest"]["report"]["number"]}">#{e["latest"]["report"]["number"]}</a></td>'
        f'<td>{esc(e["latest"]["report"]["date"])}</td></tr>'
        for e in entries
    )
    empty = "" if entries else '<p class="muted">No repository has been measured yet.</p>'
    body = f"""
<section class="section">
  <div class="container">
    <div class="section-head">
      <p class="eyebrow">{len(entries)} repositories · alphabetical · unranked</p>
      <h1>Measured repositories</h1>
      <p class="lede">Every repository that appears in a <a href="/{REPORTS_REL}/">Survival Report</a> has its own page: the latest counts, the counts by agent, the history across reports, the exact bytes behind each number and a README badge. <strong>These are counts, not grades.</strong> The list is alphabetical; nothing here ranks or compares repositories, and the <a href="/method">method and its limits</a> are public.</p>
      <p class="rp-meta"><a href="/{REPORTS_REL}/">all reports</a> · <a href="/{REPORTS_REL}/feed.xml">Atom feed</a> · <a href="{REPO_URL}/blob/main/.github/survival-repos.txt" rel="noopener">add a repository</a> · <a href="{REPO_URL}/edit/main/.github/survival-optout.txt" rel="noopener">opt out</a></p>
    </div>
    <div class="rp-section">
    <h3 id="repositories">Repositories</h3>
    <p class="muted">Latest counts per repository, from the most recent report it was measured in. <em>n &lt; 5</em>: fewer AI-tagged commits than the sample floor; counts are published, no ratio is aggregated.</p>
    {empty}
    <div class="tbl-scroll wide">
      <table class="lb-table" id="repos">
        <thead><tr><th>Repository</th><th>AI-tagged commits</th><th>Lines introduced</th><th>Still at HEAD</th><th>Line-weighted</th><th>Report</th><th>Date</th></tr></thead>
        <tbody>
{rows}
        </tbody>
      </table>
    </div>
    </div>
    <div class="proof rp-section" id="badge">
      <h3>The badge</h3>
      <p>Each page carries a text-only badge in one colour, <code translate="no">/{REPOS_REL}/&lt;owner&gt;/&lt;repo&gt;/badge.svg</code>, that follows the latest report; <code translate="no">latest.json</code> next to it holds the same counts for machines. Paths are lowercase.</p>
<pre translate="no"><code translate="no">[![AI code survival]({SITE_URL}/{REPOS_REL}/&lt;owner&gt;/&lt;repo&gt;/badge.svg)]({SITE_URL}/{REPOS_REL}/&lt;owner&gt;/&lt;repo&gt;/)</code></pre>
    </div>
  </div>
</section>
"""
    return page_head(title, desc, url, f"{SITE_URL}/assets/og.png", jsonld) + body + page_foot()


REPO_PAGE_FILES = {"index.html", "badge.svg", "badge-dark.svg", "latest.json"}


def repo_aliases(archive: list[dict[str, Any]], entries: list[dict[str, Any]]) -> list[tuple[str, str]]:
    """Names a report dropped as duplicates of another repository, mapped to
    the page of the name kept: (alias path, kept path). A page that was
    published under the dropped name, and any badge embedded from it, keeps
    resolving. Only names no report measures on their own."""
    measured = {e["key"] for e in entries}
    kept_path = {e["key"]: e["path"] for e in entries}
    out: dict[str, str] = {}
    for f in archive:
        for d in (f.get("excluded") or {}).get("duplicates") or []:
            alias, kept = d["dropped"], d["kept"].lower()
            if alias.lower() not in measured and kept in kept_path:
                out[alias.lower()] = kept_path[kept]
                if alias != alias.lower():
                    out[alias] = kept_path[kept]
    return sorted(out.items())


def write_repo_pages(site: Path, archive: list[dict[str, Any]]) -> list[dict[str, Any]]:
    entries = measured_repos(archive)
    base = site / REPOS_REL
    base.mkdir(parents=True, exist_ok=True)
    for e in entries:
        out = site / e["path"].strip("/")
        out.mkdir(parents=True, exist_ok=True)
        (out / "index.html").write_text(render_repo_page(e), encoding="utf-8")
        (out / "badge.svg").write_text(badge_svg(e), encoding="utf-8")
        (out / "badge-dark.svg").write_text(badge_svg(e, dark=True), encoding="utf-8")
        dump_json(out / "latest.json", repo_latest(e))
    (base / "index.html").write_text(render_repo_index(entries), encoding="utf-8")
    # A page no report measures any more (a name that turned out to be a
    # duplicate) is removed, but only if it holds nothing but generated
    # files; the redirect block sends its URLs to the page of the kept name.
    live = {site / e["path"].strip("/") for e in entries}
    for owner in sorted(p for p in base.iterdir() if p.is_dir()):
        for page in sorted(p for p in owner.iterdir() if p.is_dir()):
            if page in live:
                continue
            files = {p.name for p in page.iterdir()}
            if files and files <= REPO_PAGE_FILES:
                shutil.rmtree(page)
        if not any(owner.iterdir()):
            owner.rmdir()
    return entries


# ----------------------------------------------------------------------------- site glue

def replace_block(text: str, begin: str, end: str, body: str, insert_before: str | None) -> str:
    block = f"{begin}\n{body}\n{end}"
    pattern = re.compile(re.escape(begin) + r".*?" + re.escape(end), re.S)
    if pattern.search(text):
        return pattern.sub(lambda _: block, text)
    if insert_before and insert_before in text:
        return text.replace(insert_before, block + "\n" + insert_before, 1)
    return text.rstrip("\n") + "\n\n" + block + "\n"


def update_redirects(site: Path, latest: dict[str, Any] | None, repos: list[dict[str, Any]] = (), aliases: list[tuple[str, str]] = ()) -> None:
    path = site / "_redirects"
    text = path.read_text(encoding="utf-8") if path.exists() else ""
    lines = [
        f"/survival             /{REPORTS_REL}/                 301",
        f"/survival-data.json   /{REPORTS_REL}/latest.json      301",
    ]
    target = f"/{REPORTS_REL}/{latest['id']}/" if latest else f"/{REPORTS_REL}/"
    lines.append(f"/report               {target}   302")
    lines.append(f"/report/latest        {target}   302")
    # Repository pages live at lowercase paths; the spelling GitHub shows
    # reaches the same page. Static rules only: splat rules are capped at 100.
    for e in repos:
        cased = f"/{REPOS_REL}/{e['repo']}/"
        if cased != e["path"]:
            lines.append(f"{cased:<21} {e['path']}   301")
    for alias, kept in aliases:
        for name in ("", "badge.svg", "badge-dark.svg", "latest.json"):
            lines.append(f"/{REPOS_REL}/{alias}/{name}".ljust(21) + f" {kept}{name}   301")
    path.write_text(replace_block(text, REDIRECT_BEGIN, REDIRECT_END, "\n".join(lines), None), encoding="utf-8")


def update_sitemap(site: Path, archive: list[dict[str, Any]], repos: list[dict[str, Any]] = ()) -> None:
    path = site / "sitemap.xml"
    text = path.read_text(encoding="utf-8") if path.exists() else (
        '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n</urlset>\n')
    urls = [f"  <url><loc>{SITE_URL}/{REPORTS_REL}/</loc><changefreq>weekly</changefreq><priority>0.8</priority></url>"]
    for a in archive:
        urls.append(f"  <url><loc>{esc(a['url'])}</loc><lastmod>{esc(a['date'])}</lastmod><changefreq>yearly</changefreq><priority>0.6</priority></url>")
    urls.append(f"  <url><loc>{SITE_URL}/{REPOS_REL}/</loc><changefreq>weekly</changefreq><priority>0.6</priority></url>")
    for e in repos:
        urls.append(f"  <url><loc>{esc(e['url'])}</loc><lastmod>{esc(e['latest']['report']['date'])}</lastmod><changefreq>weekly</changefreq><priority>0.5</priority></url>")
    path.write_text(replace_block(text, "  " + SITEMAP_BEGIN, "  " + SITEMAP_END, "\n".join(urls), "</urlset>"), encoding="utf-8")


def load_archive(site: Path) -> list[dict[str, Any]]:
    out = []
    for fp in glob.glob(str(site / REPORTS_REL / "[0-9][0-9][0-9][0-9]" / "*" / "report.json")):
        a = load_json(Path(fp), None)
        if isinstance(a, dict) and a.get("schema") == SCHEMA:
            out.append(a)
    out.sort(key=lambda a: (a["number"], a["date"]), reverse=True)
    return out


def next_number(site: Path) -> int:
    dirs = [d for d in glob.glob(str(site / REPORTS_REL / "[0-9][0-9][0-9][0-9]" / "*")) if Path(d).is_dir() and Path(d).name.isdigit()]
    return len(dirs) + 1


def rebuild(site: Path) -> list[dict[str, Any]]:
    """Everything derived from the report.json files: each report page (so a
    change to the templates reaches every report), archive, feed, latest.json,
    the repository pages and badges, and the managed redirect and sitemap
    blocks. Deterministic: the same inputs give the same bytes."""
    archive = load_archive(site)
    base = site / REPORTS_REL
    base.mkdir(parents=True, exist_ok=True)
    for a in archive:
        (base / a["id"] / "index.html").write_text(render_report(a), encoding="utf-8")
    (base / "index.html").write_text(render_index(archive), encoding="utf-8")
    (base / "feed.xml").write_text(render_feed(archive), encoding="utf-8")
    if archive:
        dump_json(base / "latest.json", archive[0])
    repos = write_repo_pages(site, archive)
    update_redirects(site, archive[0] if archive else None, repos, repo_aliases(archive, repos))
    update_sitemap(site, archive, repos)
    return archive


def write_report(f: dict[str, Any], site: Path, png: bool = True) -> Path:
    out = site / REPORTS_REL / f["id"]
    if out.exists() and (out / "report.json").exists():
        existing = load_json(out / "report.json", {})
        if existing.get("number") != f["number"]:
            raise SystemExit(f"{out} already holds report #{existing.get('number')}; refusing to overwrite")
        # The same number again is a correction: it goes through `revise`,
        # which freezes the superseded bytes and says what changed.
        if revision_of(f) <= revision_of(existing):
            raise SystemExit(
                f"{out} already holds report #{existing.get('number')} revision {revision_of(existing)}; "
                f"a correction is `revise --number {f['number']} --run <dir> --note '<what changed>'`"
            )
    out.mkdir(parents=True, exist_ok=True)
    sources = f.pop("_sources", {})
    (out / "repos").mkdir(exist_ok=True)
    for repo, src in sources.items():
        shutil.copyfile(src, out / "repos" / f"{repo_slug(repo)}.json")
    if f.get("reach"):
        for name, src in f["reach"].pop("_files", {}).items():
            shutil.copyfile(src, out / name)
    dump_json(out / "report.json", f)
    (out / "report.md").write_text(report_md(f), encoding="utf-8")
    (out / "card.svg").write_text(card_svg(f), encoding="utf-8")
    if png:
        card_png(f, out / "card.png")
    (out / "index.html").write_text(render_report(f), encoding="utf-8")
    return out


def revise(run_dir: Path, number: int, note: str, site: Path, root: Path, today: str | None = None, png: bool = True) -> dict[str, Any]:
    """Publish a corrected revision of an existing report from the same run
    directory (or a corrected one), without editing history: the superseded
    report.json and report.md are frozen as report.r<K>.json / .md next to
    the page, the new report.json carries `revision`, `revised_at` and a
    `corrections` entry naming what changed and where the old bytes are. The
    report date stays the date of the measurement. A DOI belongs to bytes,
    so the new revision starts without one; the deposit mints a new version
    under the same Concept DOI."""
    if not note.strip():
        raise SystemExit("revise: --note must say what changed")
    out = site / REPORTS_REL
    prior = None
    for fp in glob.glob(str(out / "[0-9][0-9][0-9][0-9]" / f"{number:02d}" / "report.json")):
        candidate = load_json(Path(fp), None)
        if isinstance(candidate, dict) and candidate.get("number") == number:
            prior = candidate
    if prior is None:
        raise SystemExit(f"revise: no published report #{number} under {out}")
    prior_rev = revision_of(prior)
    report_dir = out / prior["id"]
    frozen = report_dir / f"report.r{prior_rev}.json"
    prior_bytes = (report_dir / "report.json").read_bytes()
    if frozen.exists() and frozen.read_bytes() != prior_bytes:
        raise SystemExit(f"revise: {frozen} exists with different bytes; refusing to overwrite a frozen revision")
    frozen.write_bytes(prior_bytes)
    if (report_dir / "report.md").exists():
        (report_dir / f"report.r{prior_rev}.md").write_bytes((report_dir / "report.md").read_bytes())

    facts = collect(run_dir, number, prior["date"], root)
    facts["revision"] = prior_rev + 1
    facts["revised_at"] = today or dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    facts["generated_at"] = prior["generated_at"]
    pa = prior.get("aggregate") or {}
    facts["corrections"] = list(prior.get("corrections") or []) + [{
        "revision": prior_rev + 1,
        "date": facts["revised_at"][:10],
        "note": note.strip(),
        "previous": {
            "revision": prior_rev,
            "file": frozen.name,
            "doi": prior.get("doi"),
            "aggregate": {k: pa.get(k) for k in ("repositories", "ai_tagged_commits", "introduced", "surviving", "survival_rate")},
        },
    }]
    facts["concept_doi"] = prior.get("concept_doi")
    write_report(facts, site, png=png)
    rebuild(site)
    return facts


def rerender(report_dir: Path) -> None:
    """Re-render page and markdown from an edited report.json (e.g. after a DOI was written)."""
    f = load_json(report_dir / "report.json", None)
    if not isinstance(f, dict):
        raise SystemExit(f"{report_dir}/report.json missing")
    (report_dir / "index.html").write_text(render_report(f), encoding="utf-8")
    (report_dir / "report.md").write_text(report_md(f), encoding="utf-8")


# ----------------------------------------------------------------------------- converter

def from_existing(src: Path, out: Path) -> int:
    """Turn the retired site/survival-data.json into a run directory, if and
    only if its rows carry the method v2 fields. Method v1 rows (no coverage,
    no capped rate, blame without -w -M -C) cannot be relabelled as v2 and are
    refused: the first report must come from a fresh run instead."""
    data = load_json(src, None)
    if not isinstance(data, dict) or not isinstance(data.get("rows"), list):
        print(f"from-existing: {src} is not a survival-data.json", file=sys.stderr)
        return 2
    rows = data["rows"]
    missing = [r.get("repo") for r in rows if not is_v2(r)]
    if missing:
        print(f"from-existing: {len(missing)} of {len(rows)} rows lack method v2 fields (coverage/small_sample/shallow): "
              f"{', '.join(map(str, missing[:5]))}{'…' if len(missing) > 5 else ''}", file=sys.stderr)
        print("from-existing: refusing to relabel v1 data; generate Report #1 from a fresh run (see docs/survival-report.md)", file=sys.stderr)
        return 3
    out.mkdir(parents=True, exist_ok=True)
    for r in rows:
        repo = r["repo"]
        audit = {k: v for k, v in r.items() if k not in ("repo", "audited_at")}
        dump_json(out / f"{repo_slug(repo)}.json", audit)
    dump_json(out / "run.json", {
        "generated_at": data.get("generated_at"), "tool": data.get("tool") or "causari", "tool_version": data.get("tool_version") or "unknown",
        "method": data.get("method") or "v2", "command": data.get("command") or "re audit <owner/repo> --json",
        "repos": [r["repo"] for r in rows], "failed": [], "opted_out": [],
    })
    print(f"from-existing: wrote {len(rows)} audits to {out}")
    return 0


# ----------------------------------------------------------------------------- shards

def _union(lists: list[list[Any]]) -> list[str]:
    """Case-insensitive union keeping the first spelling, sorted case-insensitively."""
    seen: dict[str, str] = {}
    for lst in lists:
        for item in lst or []:
            s = str(item).strip()
            if s and s.lower() not in seen:
                seen[s.lower()] = s
    return sorted(seen.values(), key=str.lower)


def merge_shards(run_dir: Path, root: Path) -> dict[str, Any]:
    """Merge the `run-shard-<k>.json` fragments the audit matrix uploaded
    into one `run.json` with the same schema. The workflow shards the list
    by index; every fragment carries its own repos, failed and opted_out.
    A repository of the list that no fragment mentions (its shard timed out
    or never uploaded) is recorded as failed, never silently dropped."""
    fragments = []
    for fp in sorted(run_dir.glob("run-shard-*.json")):
        frag = load_json(fp, None)
        if isinstance(frag, dict):
            fragments.append((fp.name, frag))
        else:
            print(f"merge-shards: {fp.name} is not valid JSON; ignored", file=sys.stderr)
    if not fragments:
        raise SystemExit(f"{run_dir}: no run-shard-*.json to merge")

    def first(key: str, default: Any) -> Any:
        return next((f[key] for _, f in fragments if f.get(key)), default)

    versions = sorted({str(f.get("tool_version")) for _, f in fragments if f.get("tool_version")})
    if len(versions) > 1:
        print(f"merge-shards: shards ran different tool versions: {', '.join(versions)}", file=sys.stderr)
    generated = sorted(str(f["generated_at"]) for _, f in fragments if f.get("generated_at"))
    repos = _union([f.get("repos") or [] for _, f in fragments])
    failed = _union([f.get("failed") or [] for _, f in fragments])
    opted = _union([f.get("opted_out") or [] for _, f in fragments])

    optout = read_optout(root)
    known = {r.lower() for r in repos} | {o.lower() for o in opted}
    missing = [r for r in read_repo_list(root) if r.lower() not in known and r.lower() not in optout]
    if missing:
        print(f"merge-shards: {len(missing)} repositories of the list were reported by no shard; recorded as failed: "
              f"{', '.join(missing)}", file=sys.stderr)
    audited = {slug_repo(fp.stem).lower() for fp in run_dir.glob("*__*.json")}
    unreported = [r for r in repos if r.lower() not in audited and r.lower() not in {x.lower() for x in failed}]
    if unreported:
        print(f"merge-shards: {len(unreported)} repositories have neither an audit nor a failure record; recorded as failed: "
              f"{', '.join(unreported)}", file=sys.stderr)

    run = {
        "generated_at": generated[0] if generated else dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "tool": first("tool", "causari"),
        "tool_version": first("tool_version", "unknown"),
        "method": first("method", "v2"),
        "command": first("command", "re audit <owner/repo> --json"),
        "repos": _union([repos, missing]),
        "failed": _union([failed, missing, unreported]),
        "opted_out": opted,
        "shards": [name for name, _ in fragments],
    }
    dump_json(run_dir / "run.json", run)
    print(f"merge-shards: {len(fragments)} shards → run.json · {len(run['repos'])} repositories, {len(audited)} audits, "
          f"{len(run['failed'])} failed, {len(opted)} opted out")
    return run


# ----------------------------------------------------------------------------- main

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--site", default=str(ROOT / "site"), help="site root (default: <repo>/site)")
    ap.add_argument("--root", default=str(ROOT), help="repository root, for .github/survival-optout.txt")
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="build one report from a run directory, then rebuild archive, feed, latest, redirects")
    b.add_argument("--run", required=True, help="directory with run.json and <owner>__<repo>.json")
    b.add_argument("--number", type=int, help="report number (default: existing report dirs + 1)")
    b.add_argument("--date", help="report date YYYY-MM-DD (default: run.json generated_at, else today UTC)")
    b.add_argument("--no-png", action="store_true", help="skip card.png even if Pillow is present")
    rv = sub.add_parser("revise", help="publish a corrected revision of an existing report: the superseded report.json/.md are frozen as report.r<K>.*, the new one says what changed")
    rv.add_argument("--run", required=True, help="run directory (the original, or a corrected one)")
    rv.add_argument("--number", type=int, required=True, help="the report to correct")
    rv.add_argument("--note", required=True, help="one plain sentence: what was wrong, what changed")
    rv.add_argument("--no-png", action="store_true")
    sub.add_parser("rebuild", help="report pages, archive index, feed, latest.json, repository pages and badges (site/r/), redirects, sitemap from existing report.json files")
    rr = sub.add_parser("rerender", help="re-render one report's page and markdown from its report.json")
    rr.add_argument("report_dir")
    fe = sub.add_parser("from-existing", help="convert the retired survival-data.json into a run directory (v2 rows only)")
    fe.add_argument("src")
    fe.add_argument("--out", required=True)
    ms = sub.add_parser("merge-shards", help="merge run-shard-*.json fragments of the audit matrix into run.json")
    ms.add_argument("--run", required=True, help="directory holding the downloaded shard artifacts")
    args = ap.parse_args(argv)

    site = Path(args.site)
    root = Path(args.root)
    if args.cmd == "from-existing":
        return from_existing(Path(args.src), Path(args.out))
    if args.cmd == "merge-shards":
        merge_shards(Path(args.run), root)
        return 0
    if args.cmd == "rerender":
        rerender(Path(args.report_dir))
        rebuild(site)
        return 0
    if args.cmd == "revise":
        facts = revise(Path(args.run), args.number, args.note, site, root, png=not args.no_png)
        a = facts["aggregate"]
        print(f"survival_report: #{args.number} revision {facts['revision']} · {a['repositories']} repositories aggregated · "
              f"{fmt_int(a['surviving'])}/{fmt_int(a['introduced'])} lines ({fmt_pct(a['survival_rate'])}) · "
              f"previous bytes frozen as {facts['corrections'][-1]['previous']['file']}")
        return 0
    if args.cmd == "rebuild":
        archive = rebuild(site)
        print(f"survival_report: rebuilt archive with {len(archive)} report(s), {len(measured_repos(archive))} repository page(s)")
        return 0

    run_dir = Path(args.run)
    number = args.number or next_number(site)
    run = load_json(run_dir / "run.json", {}) or {}
    date = args.date or (run.get("generated_at") or dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))[:10]
    facts = collect(run_dir, number, date, root)
    out = write_report(facts, site, png=not args.no_png)
    archive = rebuild(site)
    a = facts["aggregate"]
    print(f"survival_report: #{number} {date} → {out} · {a['repositories']} repositories aggregated, "
          f"{len(facts['not_aggregated'])} measured only, {len(facts['excluded']['shallow'])} shallow, "
          f"{len(facts['excluded']['failed'])} failed · {fmt_int(a['surviving'])}/{fmt_int(a['introduced'])} lines "
          f"({fmt_pct(a['survival_rate'])}) · archive {len(archive)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
