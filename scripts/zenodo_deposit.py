#!/usr/bin/env python3
"""Deposit one Survival Report on Zenodo and write the DOI back into the report.

Zenodo (CERN/OpenAIRE) issues DOIs without an endorsement gate and is indexed
by Google Scholar, OpenAIRE and DataCite. Every report becomes a permanent,
citable copy; all reports roll up to one Concept DOI, the citation for the
series. Mirrors ops/phase0/zenodo_deposit_weekly.py of the Crovia Silence
Report: same metadata shape, same creators, same licence, same idempotency.

Deposited files:  report.json, report.md, card.png, repos/*.json, MANIFEST.json

State: site/reports/survival/zenodo.json keeps the Concept DOI and one record
per report (production and sandbox kept apart), so re-running for the same
report finds the record and either skips (same bytes) or publishes a new
version of it (changed bytes) instead of creating a duplicate.

    ZENODO_TOKEN=…            python3 scripts/zenodo_deposit.py site/reports/survival/2026/01
    ZENODO_SANDBOX=1 …        same against sandbox.zenodo.org (separate state)
    python3 scripts/zenodo_deposit.py --dry-run <report dir>   no token needed; prints the payload

Standard library only.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
SITE_URL = "https://causari.dev"
REPO_URL = "https://github.com/croviatrust/causari"

SANDBOX = os.environ.get("ZENODO_SANDBOX", "") not in ("", "0", "false", "no")
ZENODO_API = os.environ.get("ZENODO_API") or ("https://sandbox.zenodo.org/api" if SANDBOX else "https://zenodo.org/api")
TOKEN = os.environ.get("ZENODO_TOKEN", "")

KEYWORDS = [
    "Causari", "AI code survival", "AI-assisted programming", "code provenance", "git blame",
    "Co-Authored-By", "coding agents", "Claude Code", "GitHub Copilot", "aider", "Cursor",
    "open-source repositories", "reproducible measurement", "software evolution", "Crovia",
]


# ----------------------------------------------------------------------------- http

def http(method: str, url: str, headers: dict | None = None, data: bytes | None = None, timeout: int = 180) -> tuple[int, Any]:
    req = urllib.request.Request(url, method=method, data=data, headers=headers or {})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            body = r.read()
            return r.status, (json.loads(body) if body else {})
    except urllib.error.HTTPError as e:
        return e.code, {"error": e.read().decode("utf-8", "replace")[:1000]}


def auth() -> dict[str, str]:
    return {"Content-Type": "application/json", "Authorization": f"Bearer {TOKEN}"}


def load_json(p: Path, default: Any) -> Any:
    try:
        return json.loads(p.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return default


def dump_json(p: Path, data: Any) -> None:
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(data, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")


# ----------------------------------------------------------------------------- payload

def gather(report_dir: Path) -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    for name in ("report.json", "report.md", "card.png", "card.svg", "reach.sheet.json", "egress-policy.json"):
        if (report_dir / name).exists():
            files[name] = (report_dir / name).read_bytes()
    for fp in sorted((report_dir / "repos").glob("*.json")) if (report_dir / "repos").is_dir() else []:
        files[f"repos/{fp.name}"] = fp.read_bytes()
    return files


def manifest(f: dict[str, Any], files: dict[str, bytes]) -> dict[str, Any]:
    return {
        "report": f["id"], "number": f["number"], "date": f["date"],
        "generated_at": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "files": [{"path": k, "bytes": len(v), "sha256": hashlib.sha256(v).hexdigest()} for k, v in sorted(files.items())],
    }


def content_hash(files: dict[str, bytes]) -> str:
    """One digest over the deposited bytes (MANIFEST excluded): same digest,
    same record, nothing to publish."""
    h = hashlib.sha256()
    for k in sorted(files):
        if k == "MANIFEST.json":
            continue
        h.update(k.encode())
        h.update(b"\0")
        h.update(hashlib.sha256(files[k]).digest())
    return h.hexdigest()


def pct(x: float | None) -> str:
    return "—" if x is None else f"{x * 100:.1f} %"


def description(f: dict[str, Any], concept_doi: str | None) -> str:
    a = f["aggregate"]
    m = f["method"]
    iv = a.get("survival_rate_interval_95")
    interval = f" 95 % bootstrap interval over the sampled repositories: {pct(iv['low'])} to {pct(iv['high'])}." if iv else ""
    rows = "".join(
        f"<li>{r['repo']}: {r['verified']['surviving']:,} of {r['verified']['introduced']:,} lines from "
        f"{r['verified']['commits']:,} AI-tagged commits still at HEAD ({pct(r['verified']['survival_rate'])}; capped {pct(r['verified']['capped_survival_rate'])})</li>"
        for r in f["repositories"]
    )
    series = f" Cite the series by the Concept DOI {concept_doi}." if concept_doi else ""
    corrections = ""
    if f.get("corrections"):
        sys.path.insert(0, str(HERE))
        import survival_report

        items = "".join(f"<li>{c}</li>" for c in survival_report.correction_lines(f))
        corrections = (f"<p>This is revision {f.get('revision')} of report #{f['number']}. Superseded revisions keep their bytes and "
                       f"their DOI; what changed:</p><ul>{items}</ul>")
    return (corrections +
        f"<p>Survival Report #{f['number']} ({f['date']}): counts of surviving lines from AI-tagged commits in "
        f"{a['repositories']} open-source repositories, measured with {f['tool']['name']} {f['tool']['version']}, method {m['version']}. "
        f"{a['surviving']:,} of {a['introduced']:,} lines introduced by {a['ai_tagged_commits']:,} commits carrying machine-readable AI "
        f"authorship metadata are still attributed to those commits at HEAD ({pct(a['survival_rate'])}).{interval}</p>"
        f"<p>Repositories, alphabetical:</p><ul>{rows}</ul>"
        f"<p>Method: detection from commit metadata only; survival from <code>git blame {' '.join(m['blame_flags'])}</code> at HEAD; "
        f"per-commit cap ({m['cap_rule']}); sample floor {m['sample_floor']} VERIFIED commits; full clones only. "
        f"These are counts, not grades: no rank, no verdict; the intervals describe the sampled repositories, not all AI-assisted code. "
        f"Every number is reproducible with <code>{m['command']}</code>; the exact audit bytes are deposited under <code>repos/</code>. "
        f"Method and limits: <a href=\"{m['url']}\">{m['url']}</a>. Report page: <a href=\"{f['url']}\">{f['url']}</a>.</p>"
        f"<p>Licence CC-BY-4.0.{series}</p>"
    )


def metadata(f: dict[str, Any], version: str, concept_doi: str | None) -> dict[str, Any]:
    a = f["aggregate"]
    return {"metadata": {
        "title": f"Causari — Survival Report #{f['number']} ({f['date']}): AI code survival in {a['repositories']} open-source repositories",
        "upload_type": "publication",
        "publication_type": "report",
        "publication_date": f["date"],
        "description": description(f, concept_doi),
        "creators": [{"name": "Crovia Trust", "affiliation": "Crovia Trust"}],
        "keywords": KEYWORDS,
        "license": "cc-by-4.0",
        "access_right": "open",
        "version": version,
        "language": "eng",
        "related_identifiers": [
            {"identifier": f["url"], "relation": "isIdenticalTo", "resource_type": "publication-report"},
            {"identifier": f["url"] + "report.json", "relation": "isSupplementedBy", "resource_type": "dataset"},
            {"identifier": f["method"]["url"], "relation": "isDescribedBy", "resource_type": "publication-other"},
            {"identifier": REPO_URL, "relation": "isSupplementTo", "resource_type": "software"},
            {"identifier": "https://arxiv.org/abs/2601.16809", "relation": "references", "resource_type": "publication-preprint"},
        ],
    }}


# ----------------------------------------------------------------------------- deposit

def deposit(files: dict[str, bytes], meta: dict[str, Any], parent_id: int | None) -> tuple[dict | None, dict | None]:
    if parent_id:
        st, body = http("POST", f"{ZENODO_API}/deposit/depositions/{parent_id}/actions/newversion", auth())
        if st != 201:
            return None, {"step": "newversion", "status": st, "body": body}
        st, dep = http("GET", body["links"]["latest_draft"], auth())
    else:
        st, dep = http("POST", f"{ZENODO_API}/deposit/depositions", auth(), data=b"{}")
    if st not in (200, 201):
        return None, {"step": "create", "status": st, "body": dep}
    dep_id, bucket = dep["id"], dep["links"]["bucket"]
    # a new version inherits the previous files; remove them so the record holds exactly this report
    for fobj in dep.get("files", []):
        http("DELETE", f"{ZENODO_API}/deposit/depositions/{dep_id}/files/{fobj['id']}", auth())
    for name, blob in files.items():
        st, r = http("PUT", f"{bucket}/{name.replace('/', '__')}",
                     {"Content-Type": "application/octet-stream", "Authorization": f"Bearer {TOKEN}"}, data=blob)
        if st not in (200, 201):
            return None, {"step": f"upload {name}", "status": st, "body": r}
    st, body = http("PUT", f"{ZENODO_API}/deposit/depositions/{dep_id}", auth(), data=json.dumps(meta).encode())
    if st != 200:
        return None, {"step": "metadata", "status": st, "body": body}
    st, body = http("POST", f"{ZENODO_API}/deposit/depositions/{dep_id}/actions/publish", auth())
    if st not in (200, 202):
        return None, {"step": "publish", "status": st, "body": body}
    return body, None


def write_back(report_dir: Path, f: dict[str, Any], rec: dict[str, Any], site: Path) -> None:
    """DOI into report.json, then re-render the page/markdown and the archive
    through the generator so every surface says the same thing."""
    f["doi"] = rec["doi"]
    f["concept_doi"] = rec.get("concept_doi")
    f["zenodo"] = {"record_url": rec.get("html"), "record_id": rec.get("id"), "version": rec.get("version"),
                   "sandbox": rec.get("sandbox", False), "published_at": rec.get("published_at")}
    dump_json(report_dir / "report.json", f)
    sys.path.insert(0, str(HERE))
    import survival_report

    survival_report.rerender(report_dir)
    survival_report.rebuild(site)


# ----------------------------------------------------------------------------- main

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("report_dir", help="site/reports/survival/<YYYY>/<NN>")
    ap.add_argument("--site", default=str(ROOT / "site"))
    ap.add_argument("--state", help="state file (default: <site>/reports/survival/zenodo.json)")
    ap.add_argument("--dry-run", action="store_true", help="print the payload and stop; no token needed")
    ap.add_argument("--force", action="store_true", help="publish a new version even if the bytes are unchanged")
    args = ap.parse_args(argv)

    report_dir = Path(args.report_dir)
    site = Path(args.site)
    state_path = Path(args.state) if args.state else site / "reports" / "survival" / "zenodo.json"
    f = load_json(report_dir / "report.json", None)
    if not isinstance(f, dict) or f.get("schema") != "causari.survival_report.v1":
        print(json.dumps({"error": f"{report_dir}/report.json missing or not a survival report"}))
        return 2
    if not TOKEN and not args.dry_run:
        print(json.dumps({"error": "ZENODO_TOKEN not set", "hint": "add the secret or run with --dry-run"}))
        return 2

    state = load_json(state_path, {"schema": "causari.zenodo_state.v1", "production": {"concept_doi": None, "concept_id": None, "reports": {}},
                                   "sandbox": {"concept_doi": None, "concept_id": None, "reports": {}}})
    env = state["sandbox" if SANDBOX else "production"]
    prior = env["reports"].get(f["id"])

    files = gather(report_dir)
    if "report.json" not in files or "report.md" not in files:
        print(json.dumps({"error": "report.json/report.md missing; build the report first"}))
        return 3
    digest = content_hash(files)
    if prior and prior.get("content_sha256") == digest and not args.force:
        print(json.dumps({"skipped": True, "reason": "same bytes already deposited", "report": f["id"], "doi": prior.get("doi")}, indent=1))
        return 0
    files["MANIFEST.json"] = (json.dumps(manifest(f, files), indent=1) + "\n").encode()
    # The Zenodo version follows the report's own revision when it has one
    # (a correction published with `survival_report.py revise`); otherwise
    # one more than the last deposit of this report.
    revision = max((prior or {}).get("revision", 0) + 1, int(f.get("revision") or 1))
    version = f"#{f['number']}" if revision == 1 else f"#{f['number']}-r{revision}"
    meta = metadata(f, version, env.get("concept_doi"))

    if args.dry_run:
        print(json.dumps({
            "dry_run": True, "api": ZENODO_API, "sandbox": SANDBOX, "report": f["id"], "version": version,
            "parent_record": env.get("concept_id"), "concept_doi": env.get("concept_doi"),
            "files": [{"path": k, "bytes": len(v), "sha256": hashlib.sha256(v).hexdigest()} for k, v in sorted(files.items())],
            "content_sha256": digest, "metadata": meta["metadata"],
        }, indent=1, ensure_ascii=False))
        return 0

    pub, err = deposit(files, meta, env.get("concept_id"))
    if err:
        print(json.dumps({"error": err}, indent=1))
        return 4
    rec = {
        "doi": pub.get("doi") or pub["metadata"].get("doi"),
        "concept_doi": pub.get("conceptdoi") or env.get("concept_doi"),
        "html": pub["links"].get("html") or pub["links"].get("record_html"),
        "id": pub["id"], "version": version, "revision": revision, "sandbox": SANDBOX,
        "content_sha256": digest,
        "published_at": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    env["reports"][f["id"]] = rec
    env["concept_id"] = env.get("concept_id") or rec["id"]
    env["concept_doi"] = env.get("concept_doi") or rec["concept_doi"]
    dump_json(state_path, state)
    write_back(report_dir, f, rec, site)
    print(json.dumps({"published": True, "report": f["id"], **rec}, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
