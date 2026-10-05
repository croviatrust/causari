#!/usr/bin/env python3
"""Publication gate for the one-shot Survival Report #4 recovery.

Two invariants are kept apart:

- The measurement is frozen: Actions run 37308519983, source commit
  8c7c431, the original shard artifacts, and one deterministic report hash.
- The publication parent is the workflow commit that is executing. That
  commit must still be origin/main at the moment of the fast-forward push.
  A later reviewed fix becomes the parent by being the commit that runs,
  not by editing a hardcoded main SHA.

No network except `deposit-plan`, which only GETs Zenodo.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

MEASUREMENT_SHA = "8c7c431f3fd934a0d9c5005d376f131ab89bd0fa"
SOURCE_RUN = "37308519983"
REPORT_ID = "2026/04"
CANON_SHA = "2bbc49cbb54140937b04b0eb2ca9c50cfd521ba40a86d0d9587050219efc588e"
GENERATED_AT = "2026-10-05T12:17:49Z"
REACH_RUN = "survival-report-gh-37308519983"
EXPECTED_LOGIN = "croviatrust"
REPORT_SUBJECT = (
    "report: Survival Report #4 (2026-10-05, method v3) "
    "assembled from Actions run 37308519983"
)
DOI_SUBJECT = re.compile(
    r"^report: DOI 10\.5281/zenodo\.\d+ for Survival Report #4 "
    r"assembled from Actions run 37308519983$"
)
DOI_VALUE = re.compile(r"^10\.5281/zenodo\.\d+$")
SHA = re.compile(r"^[0-9a-f]{40}$")
PRODUCTION_SANDBOX = {"", "0", "false", "no"}
ALLOWED_DIRS = ("site/reports/survival/", "site/r/")
ALLOWED_FILES = {"site/_redirects", "site/sitemap.xml"}
ZENODO_API = "https://zenodo.org/api"


def die(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def canonical_sha(report: dict) -> str:
    """Hash used for the frozen report. Deposit fields stay in the document;
    witness close time and the witness key do not."""
    body = json.loads(json.dumps(report))
    reach = body.get("reach") or {}
    reach.pop("closed_at", None)
    reach.pop("witness", None)
    body["reach"] = reach
    blob = (json.dumps(body, indent=1, ensure_ascii=False) + "\n").encode()
    return hashlib.sha256(blob).hexdigest()


def neutralized(report: dict) -> dict:
    """The same report with the DOI write-back removed."""
    body = json.loads(json.dumps(report))
    body["doi"] = None
    body["concept_doi"] = None
    body["zenodo"] = None
    return body


def assert_measurement(report: dict, provenance: dict) -> str:
    """Return 'ready' or 'deposited'. Anything else is a hard stop."""
    if report.get("id") != REPORT_ID or report.get("number") != 4:
        die(f"report id is {report.get('id')}")
    if report.get("generated_at") != GENERATED_AT:
        die(f"generated_at is {report.get('generated_at')}")
    if (report.get("method") or {}).get("version") != "v3":
        die("method is not v3")
    if (report.get("tool") or {}).get("name") != "causari" or (report.get("tool") or {}).get("version") != "0.3.0":
        die("tool is not causari 0.3.0")
    if (report.get("reach") or {}).get("run_id") != REACH_RUN:
        die("reach.run_id is not the measurement run")
    if provenance.get("source_sha") != MEASUREMENT_SHA:
        die("provenance source commit is not the measurement commit")
    if provenance.get("source_run_id") != SOURCE_RUN:
        die("provenance source run is not 37308519983")
    if provenance.get("measurement_generated_at") != GENERATED_AT:
        die("provenance measurement time moved")
    if provenance.get("measurement_generated_at") == provenance.get("recovery_assembled_at"):
        die("provenance does not keep measurement time apart from recovery time")
    if provenance.get("reach_run_id") != REACH_RUN or provenance.get("report_id") != REPORT_ID:
        die("provenance report binding moved")
    if provenance.get("deterministic_report_sha256") != CANON_SHA:
        die("provenance does not name the frozen report hash")
    if canonical_sha(neutralized(report)) != CANON_SHA:
        die(f"deterministic sha256 {canonical_sha(neutralized(report))}")
    if report.get("doi"):
        if canonical_sha(report) == CANON_SHA:
            die("doi is set but the canonical hash ignored it")
        return "deposited"
    if canonical_sha(report) != CANON_SHA:
        die(f"deterministic sha256 {canonical_sha(report)}")
    return "ready"


def assert_ref(ref: str) -> None:
    if ref != "refs/heads/main":
        die(f"publication ref is {ref}")


def assert_control_plane(ref: str, workflow_sha: str, origin_main: str) -> None:
    """Main must be the commit whose workflow is executing.

    That commit is not the measurement commit. Replacing the measurement
    SHA with another constant would go stale at the next reviewed fix;
    the executing SHA does not.
    """
    assert_ref(ref)
    if not SHA.fullmatch(workflow_sha or "") or not SHA.fullmatch(origin_main or ""):
        die("control-plane sha is not a commit")
    if origin_main != workflow_sha:
        die("main is not the reviewed recovery commit that is executing")


def assert_pusher(login: str, permission: str, enforce_admins: str) -> None:
    if login != EXPECTED_LOGIN or permission != "admin" or enforce_admins != "false":
        die(
            "push identity refused: "
            f"login={login or 'missing'} permission={permission or 'missing'} "
            f"enforce_admins={enforce_admins or 'missing'}"
        )


def assert_production_sandbox(value: str) -> None:
    if value not in PRODUCTION_SANDBOX:
        die(f"ZENODO_SANDBOX={value!r} is not a production value")


def path_allowed(path: str) -> bool:
    if path.startswith("/") or ".." in path.split("/"):
        return False
    return path in ALLOWED_FILES or path.startswith(ALLOWED_DIRS)


def classify_history(rows: list[tuple[str, str, list[str]]]) -> str:
    """Commits after the executing workflow commit. Only the report commit
    and, after it, the DOI write-back may exist."""
    if not rows:
        return "at-tip"
    if len(rows) > 2:
        die(f"main has {len(rows)} commits after the reviewed recovery commit")

    def check(subject: str, files: list[str], pattern: re.Pattern[str] | None, exact: str | None) -> None:
        if exact is not None and subject != exact:
            die("a commit after the reviewed recovery commit is not the report publication")
        if pattern is not None and not pattern.match(subject):
            die("a commit after the reviewed recovery commit is not the DOI write-back")
        if not files or any(not path_allowed(item) for item in files):
            die("a commit after the reviewed recovery commit touches other paths")

    sha, subject, files = rows[0]
    if not SHA.fullmatch(sha):
        die("history contains a non-commit")
    check(subject, files, None, REPORT_SUBJECT)
    if len(rows) == 1:
        return "report"
    sha, subject, files = rows[1]
    if not SHA.fullmatch(sha):
        die("history contains a non-commit")
    check(subject, files, DOI_SUBJECT, None)
    return "doi"


def publish_decision(history: str, main_kind: str) -> str:
    if history == "at-tip" and main_kind == "absent":
        return "commit"
    if history == "report" and main_kind == "ready":
        return "already"
    if history == "doi" and main_kind == "deposited":
        return "already"
    if history == "at-tip" and main_kind == "ready":
        return "already"
    die(f"publication state refused: history={history} report={main_kind}")


def deposit_phase(history: str, main_kind: str) -> str:
    if history == "report" and main_kind == "ready":
        return "proceed"
    if history == "doi" and main_kind == "deposited":
        return "skip"
    die(f"deposit state refused: history={history} report={main_kind}")


# Fields the existing deposit already sends. source_sha is not among them:
# provenance.json is not uploaded. The canonical report hash is not a Zenodo
# metadata field; it is implied by report.json, whose MD5 Zenodo stores.
REPORT_URL = "https://causari.dev/reports/survival/2026/04/"
ZENODO_VERSION = "#4"
CONCEPT_DOI = "10.5281/zenodo.22863965"


class DepositIdentity:
    def __init__(self, title: str, report_url: str, version: str, concept_doi: str, report_md5: str) -> None:
        self.title = title
        self.report_url = report_url
        self.version = version
        self.concept_doi = concept_doi
        self.report_md5 = report_md5


def _title_of(record: dict) -> str:
    meta = record.get("metadata") or {}
    return meta.get("title") or record.get("title") or ""


def _published(record: dict) -> bool:
    meta = record.get("metadata") or {}
    doi = record.get("doi") or meta.get("doi")
    state = record.get("state") or ""
    return bool(doi) and (bool(record.get("submitted")) or state in ("done", "published"))


def _links_report(record: dict, report_url: str) -> bool:
    rels = (record.get("metadata") or {}).get("related_identifiers") or []
    return any(
        isinstance(item, dict) and item.get("relation") == "isIdenticalTo" and item.get("identifier") == report_url
        for item in rels
    )


def _report_md5(record: dict) -> str | None:
    for item in record.get("files") or []:
        name = item.get("filename") or item.get("key") or ""
        if name != "report.json":
            continue
        raw = str(item.get("checksum") or "")
        hexdigest = raw.split(":", 1)[1] if raw.startswith("md5:") else raw
        return hexdigest.lower() or None
    return None


def same_deposit(record: dict, identity: DepositIdentity) -> bool:
    """A published record is this report only when every identity Zenodo
    already stores agrees. Title alone is not enough."""
    meta = record.get("metadata") or {}
    conceptdoi = record.get("conceptdoi") or meta.get("conceptdoi")
    return (
        _title_of(record) == identity.title
        and meta.get("version") == identity.version
        and _links_report(record, identity.report_url)
        and conceptdoi == identity.concept_doi
        and _report_md5(record) == identity.report_md5.lower()
    )


def _mentions_report(record: dict, identity: DepositIdentity) -> bool:
    return _title_of(record) == identity.title or _links_report(record, identity.report_url)


def select_record(records: list[dict], identity: DepositIdentity) -> tuple[str, dict | None]:
    mentioned = [item for item in records if _mentions_report(item, identity)]
    published = [item for item in mentioned if _published(item)]
    drafts = [item for item in mentioned if not _published(item)]
    matched = [item for item in published if same_deposit(item, identity)]
    if len(matched) == 1 and len(published) == 1 and not drafts:
        return "adopt", matched[0]
    if matched or published or drafts:
        die("Zenodo has a record for this report that is not the single published deposit")
    return "create", None


def deposit_action(report: dict, state: dict, records: list[dict] | None, identity: DepositIdentity | None = None) -> str:
    stored = ((state.get("production") or {}).get("reports") or {}).get(REPORT_ID) or {}
    report_doi = report.get("doi") or None
    state_doi = stored.get("doi") or None
    if report_doi and state_doi:
        if report_doi != state_doi:
            die("report DOI and zenodo.json disagree")
        return "skip"
    if report_doi or state_doi:
        die("the report and zenodo.json disagree about whether a DOI exists")
    if records is None:
        die("Zenodo was not listed")
    if identity is None:
        die("deposit identity is missing")
    action, _record = select_record(records, identity)
    return action


def identity_for(report: dict, state: dict, report_bytes: bytes) -> DepositIdentity:
    if report.get("url") != REPORT_URL:
        die("report url is not 2026/04")
    if report.get("revision") not in (None, 1):
        die("report revision is not the first deposit")
    concept = (state.get("production") or {}).get("concept_doi")
    if concept != CONCEPT_DOI:
        die("concept lineage is not the Survival Report series")
    return DepositIdentity(
        expected_title(report),
        REPORT_URL,
        ZENODO_VERSION,
        CONCEPT_DOI,
        hashlib.md5(report_bytes).hexdigest(),
    )


def doi_subject(doi: str) -> str:
    if not DOI_VALUE.fullmatch(doi or ""):
        die("DOI is not a Zenodo DOI")
    return (
        f"report: DOI {doi} for Survival Report #4 "
        "assembled from Actions run 37308519983"
    )


def _git(repo: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=repo,
        text=True,
        capture_output=True,
        check=check,
    )


def history_rows(repo: Path, base: str) -> list[tuple[str, str, list[str]]]:
    if not SHA.fullmatch(base):
        die("workflow sha is not a commit")
    origin = _git(repo, "rev-parse", "origin/main").stdout.strip()
    if origin == base:
        return []
    if _git(repo, "merge-base", "--is-ancestor", base, "origin/main", check=False).returncode != 0:
        die("reviewed recovery commit is not an ancestor of main")
    shas = [line for line in _git(repo, "rev-list", "--reverse", f"{base}..origin/main").stdout.splitlines() if line]
    rows = []
    for sha in shas:
        subject = _git(repo, "log", "-1", "--format=%s", sha).stdout.rstrip("\n")
        files = [line for line in _git(repo, "diff-tree", "--no-commit-id", "--name-only", "-r", sha).stdout.splitlines() if line]
        rows.append((sha, subject, files))
    return rows


def show_optional(repo: Path, rev: str, path: str) -> str | None:
    result = _git(repo, "show", f"{rev}:{path}", check=False)
    if result.returncode != 0:
        return None
    return result.stdout


def classify_main(repo: Path, rev: str = "origin/main") -> str:
    report_text = show_optional(repo, rev, "site/reports/survival/2026/04/report.json")
    if report_text is None:
        return "absent"
    provenance_text = show_optional(repo, rev, "site/reports/survival/2026/04/provenance.json")
    latest_text = show_optional(repo, rev, "site/reports/survival/latest.json")
    if provenance_text is None or latest_text is None:
        die("report 2026/04 is on main without provenance or latest.json")
    kind = assert_measurement(json.loads(report_text), json.loads(provenance_text))
    latest = json.loads(latest_text)
    if latest.get("id") != REPORT_ID:
        die(f"latest.json is {latest.get('id')}")
    return kind


def verify_survival_tree(base: Path, *, allow_deposited: bool) -> str:
    report_path = base / "2026" / "04" / "report.json"
    provenance_path = base / "2026" / "04" / "provenance.json"
    latest_path = base / "latest.json"
    if not report_path.is_file() or not provenance_path.is_file() or not latest_path.is_file():
        die(f"survival tree {base} is missing the measured report")
    kind = assert_measurement(
        json.loads(report_path.read_text(encoding="utf-8")),
        json.loads(provenance_path.read_text(encoding="utf-8")),
    )
    latest = json.loads(latest_path.read_text(encoding="utf-8"))
    if latest.get("id") != REPORT_ID:
        die(f"latest.json is {latest.get('id')}")
    if kind == "deposited" and not allow_deposited:
        die("artifact already carries a DOI")
    return kind


def _next_link(header: str | None) -> str | None:
    if not header:
        return None
    for part in header.split(","):
        if 'rel="next"' not in part:
            continue
        match = re.search(r"<([^>]+)>", part)
        if not match:
            continue
        url = match.group(1)
        parsed = urllib.parse.urlparse(url)
        if parsed.scheme != "https" or parsed.netloc != "zenodo.org":
            die("zenodo list failed: unexpected page URL")
        if "access_token" in parsed.query:
            die("zenodo list failed: token in URL")
        return url
    return None


def list_depositions(token: str) -> list[dict]:
    """GET the authenticated user's depositions. Never POST. Never print the token."""
    if not token:
        die("ZENODO_TOKEN is not set. Zenodo was not called.")
    records: list[dict] = []
    url: str | None = f"{ZENODO_API}/deposit/depositions?size=50"
    for _page in range(6):
        if url is None:
            return records
        request = urllib.request.Request(
            url,
            headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
            method="GET",
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                status = response.status
                link = response.headers.get("Link")
                payload = json.loads(response.read().decode("utf-8") or "null")
        except urllib.error.HTTPError as exc:
            die(f"zenodo list failed: HTTP {exc.code}")
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError):
            die("zenodo list failed: HTTP error")
        if status != 200 or not isinstance(payload, list):
            die(f"zenodo list failed: HTTP {status}")
        records.extend(payload)
        url = _next_link(link)
    if url:
        die("zenodo list failed: too many pages")
    return records


def expected_title(report: dict) -> str:
    import zenodo_deposit

    return zenodo_deposit.metadata(report, "#4", None)["metadata"]["title"]


def load_state(site: Path) -> dict:
    path = site / "reports" / "survival" / "zenodo.json"
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        die("zenodo.json is missing")
        raise


def adopt_record(report_dir: Path, site: Path, record: dict) -> None:
    """Write an already-published Zenodo DOI back. Does not create a deposit."""
    import zenodo_deposit

    report = json.loads((report_dir / "report.json").read_text(encoding="utf-8"))
    meta = record.get("metadata") or {}
    doi = record.get("doi") or meta.get("doi")
    if not DOI_VALUE.fullmatch(doi or ""):
        die("adopt: record has no Zenodo DOI")
    digest = zenodo_deposit.content_hash(zenodo_deposit.gather(report_dir))
    state_path = site / "reports" / "survival" / "zenodo.json"
    state = json.loads(state_path.read_text(encoding="utf-8"))
    env = state.setdefault("production", {})
    env.setdefault("reports", {})
    links = record.get("links") or {}
    rec = {
        "doi": doi,
        "concept_doi": record.get("conceptdoi") or env.get("concept_doi"),
        "html": links.get("record_html") or links.get("html"),
        "id": record["id"],
        "version": "#4",
        "revision": 1,
        "sandbox": False,
        "content_sha256": digest,
        "published_at": record.get("created") or meta.get("publication_date"),
    }
    env["reports"][report["id"]] = rec
    env["concept_doi"] = env.get("concept_doi") or rec["concept_doi"]
    env["concept_id"] = env.get("concept_id") or record.get("conceptrecid") or rec["id"]
    zenodo_deposit.dump_json(state_path, state)
    zenodo_deposit.write_back(report_dir, report, rec, site)


def _repo(args: argparse.Namespace) -> Path:
    return Path(args.repo).resolve()


def _decision(args: argparse.Namespace) -> tuple[str, str]:
    repo = _repo(args)
    history = classify_history(history_rows(repo, args.base))
    kind = classify_main(repo)
    return history, kind


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=".")
    sub = parser.add_subparsers(dest="cmd", required=True)

    control = sub.add_parser("assert-control-plane")
    control.add_argument("--ref", required=True)
    control.add_argument("--workflow-sha", required=True)
    control.add_argument("--origin-main", required=True)

    ref = sub.add_parser("assert-ref")
    ref.add_argument("--ref", required=True)

    pusher = sub.add_parser("assert-pusher")
    pusher.add_argument("--login", required=True)
    pusher.add_argument("--permission", required=True)
    pusher.add_argument("--enforce-admins", required=True)

    sandbox = sub.add_parser("assert-production-sandbox")
    sandbox.add_argument("--value", default="")

    artifact = sub.add_parser("verify-artifact")
    artifact.add_argument("--root", required=True)

    worktree = sub.add_parser("verify-worktree")
    worktree.add_argument("--allow-deposited", action="store_true")

    sub.add_parser("report-subject")
    subject = sub.add_parser("doi-subject")
    subject.add_argument("--json", required=True)

    publish = sub.add_parser("publish-decision")
    publish.add_argument("--base", required=True)

    deposit = sub.add_parser("deposit-phase")
    deposit.add_argument("--base", required=True)

    plan = sub.add_parser("deposit-plan")
    plan.add_argument("--report-dir", required=True)
    plan.add_argument("--record-out", required=True)

    adopt = sub.add_parser("adopt")
    adopt.add_argument("--report-dir", required=True)
    adopt.add_argument("--site", required=True)
    adopt.add_argument("--record", required=True)

    args = parser.parse_args(argv)
    if args.cmd == "assert-control-plane":
        assert_control_plane(args.ref, args.workflow_sha, args.origin_main)
    elif args.cmd == "assert-ref":
        assert_ref(args.ref)
    elif args.cmd == "assert-pusher":
        assert_pusher(args.login, args.permission, args.enforce_admins)
    elif args.cmd == "assert-production-sandbox":
        assert_production_sandbox(args.value)
    elif args.cmd == "verify-artifact":
        root = Path(args.root)
        verify_survival_tree(root / "reports" / "survival", allow_deposited=False)
    elif args.cmd == "verify-worktree":
        verify_survival_tree(_repo(args) / "site" / "reports" / "survival", allow_deposited=args.allow_deposited)
    elif args.cmd == "report-subject":
        print(REPORT_SUBJECT)
    elif args.cmd == "doi-subject":
        payload = json.loads(Path(args.json).read_text(encoding="utf-8"))
        doi = payload.get("doi") or (payload.get("metadata") or {}).get("doi")
        print(doi_subject(doi))
    elif args.cmd == "publish-decision":
        history, kind = _decision(args)
        print(publish_decision(history, kind))
    elif args.cmd == "deposit-phase":
        history, kind = _decision(args)
        print(deposit_phase(history, kind))
    elif args.cmd == "deposit-plan":
        report_dir = Path(args.report_dir)
        if report_dir.parts[-4:] != ("reports", "survival", "2026", "04"):
            die("report dir is not 2026/04")
        site = report_dir.parents[3]
        report_bytes = (report_dir / "report.json").read_bytes()
        report = json.loads(report_bytes.decode("utf-8"))
        state = load_state(site)
        stored = ((state.get("production") or {}).get("reports") or {}).get(REPORT_ID) or {}
        if report.get("doi") or stored.get("doi"):
            print(deposit_action(report, state, None, None))
            return 0
        assert_production_sandbox(os.environ.get("ZENODO_SANDBOX", ""))
        records = list_depositions(os.environ.get("ZENODO_TOKEN", ""))
        identity = identity_for(report, state, report_bytes)
        action, record = select_record(records, identity)
        if action == "adopt" and record is not None:
            Path(args.record_out).write_text(json.dumps(record), encoding="utf-8")
        print(action)
    elif args.cmd == "adopt":
        adopt_record(Path(args.report_dir), Path(args.site), json.loads(Path(args.record).read_text(encoding="utf-8")))
    return 0


if __name__ == "__main__":
    sys.exit(main())
