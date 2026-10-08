#!/usr/bin/env python3
"""Audit Causari's public surfaces against canon/canon.json.

The canon says what every public surface of Causari must and must not say:
one version everywhere, no verdict wording on a measurement, the same agent
matrix in the README and on the site, installers that agree with the release
workflow on the archive name, every internal link on causari.dev pointing at
a file that exists. This tool checks the working tree (offline, runs in CI on
every push) and, with ``--live``, the deployed site.

    python3 scripts/audit_surfaces.py            # report
    python3 scripts/audit_surfaces.py --gate     # exit 1 on critical or high
    python3 scripts/audit_surfaces.py --live     # also fetch causari.dev

Standard library only. Findings carry a severity; ``info`` lines are the
checks that passed, so the report is a complete account of what was looked at.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.error
import urllib.request
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SEVERITIES = ("critical", "high", "medium", "low", "info")


class Report:
    def __init__(self) -> None:
        self.findings: list[dict] = []

    def add(self, area: str, severity: str, where: str, message: str) -> None:
        assert severity in SEVERITIES
        self.findings.append({"area": area, "severity": severity, "where": where, "message": message})

    def count(self, severity: str) -> int:
        return sum(1 for f in self.findings if f["severity"] == severity)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8", errors="replace")


# ---------------------------------------------------------------- versions

def check_versions(canon: dict, r: Report) -> None:
    found: dict[str, list[str]] = {}
    for spec in canon["version_sources"]["files"]:
        text = read(spec["path"])
        m = re.search(spec["pattern"], text, re.M)
        if not m:
            r.add("version", "high", spec["path"], f"pattern {spec['pattern']!r} not found")
            continue
        found.setdefault(m.group(1), []).append(f"{spec['path']} ({spec['pattern'][:30]}…)")
    if len(found) > 1:
        r.add("version", "high", "repo", f"more than one version on public surfaces: {json.dumps(found)}")
    elif found:
        r.add("version", "info", "repo", f"one version everywhere: {next(iter(found))}")


# ---------------------------------------------------------------- wording

def check_forbidden(canon: dict, r: Report) -> None:
    fw = canon["forbidden_words"]
    for path in canon["surfaces"]["text"]:
        if not (ROOT / path).exists():
            r.add("wording", "high", path, "surface listed in canon does not exist")
            continue
        text = read(path)
        allowed = set(fw["allow"].get(path, []))
        hits = []
        for word in fw["words"]:
            if word in allowed:
                continue
            for m in re.finditer(re.escape(word), text):
                # Naming the forbidden word in quotes ("no \"healthy\"") is how the
                # rule is explained; using it bare is the defect.
                before = text[max(0, m.start() - 1):m.start()]
                if before in ('"', "'", "\u201c", "\u2018", "`", "\u00ab"):
                    continue
                line = text.count("\n", 0, m.start()) + 1
                hits.append(f"{word!r} at line {line}")
                break
        if hits:
            r.add("wording", "high", path, "forbidden wording: " + "; ".join(hits))
        else:
            r.add("wording", "info", path, "no forbidden wording")


def check_required(canon: dict, r: Report) -> None:
    for path, phrases in canon["required_phrases"]["by_file"].items():
        if not (ROOT / path).exists():
            r.add("wording", "high", path, "file with required phrases does not exist")
            continue
        text = read(path)
        missing = [p for p in phrases if not any(alt in text for alt in p.split("|"))]
        if missing:
            r.add("wording", "medium", path, f"missing required phrase(s): {missing}")
        else:
            r.add("wording", "info", path, "all required phrases present")


# ---------------------------------------------------------------- files

def check_files(canon: dict, r: Report) -> None:
    for path in canon["surfaces"]["required_files"]:
        if (ROOT / path).exists():
            r.add("files", "info", path, "present")
        else:
            r.add("files", "high", path, "required file missing")
    for a, b in canon["surfaces"]["identical_pairs"]:
        if not (ROOT / a).exists() or not (ROOT / b).exists():
            r.add("files", "high", f"{a} = {b}", "one side missing")
        elif read(a) != read(b):
            r.add("files", "high", f"{a} = {b}", "files differ; the site serves a stale installer")
        else:
            r.add("files", "info", f"{a} = {b}", "identical")


# ---------------------------------------------------------------- html

class LinkCollector(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.links: list[tuple[str, str]] = []
        self.ids: set[str] = set()
        self.external_loads: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        a = dict(attrs)
        if a.get("id"):
            self.ids.add(a["id"])
        if a.get("name") and tag == "a":
            self.ids.add(a["name"])
        for key in ("href", "src"):
            v = a.get(key)
            if not v:
                continue
            rel = (a.get("rel") or "").lower()
            is_load = tag in ("script", "img") or (tag == "link" and rel not in ("canonical", "alternate", "me", "license"))
            if is_load and v.startswith(("http://", "https://")):
                self.external_loads.append(v)
            if tag == "a" or (tag in ("link", "img", "script")):
                self.links.append((tag, v))


def site_path_exists(target: str, redirects: dict[str, str]) -> bool:
    if target in redirects:
        return True
    p = target.split("?")[0]
    candidates = [ROOT / "site" / p.lstrip("/")]
    # a directory URL is a directory even when its last segment has a dot (/r/vercel/next.js/)
    if p.endswith("/") or not Path(p).suffix:
        candidates.append(ROOT / "site" / (p.lstrip("/") + ".html"))
        candidates.append(ROOT / "site" / p.lstrip("/") / "index.html")
    return any(c.is_file() for c in candidates)


def load_redirects() -> dict[str, str]:
    out: dict[str, str] = {}
    for line in read("site/_redirects").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) >= 2:
            out[parts[0]] = parts[1]
    return out


def check_html(canon: dict, r: Report) -> None:
    redirects = load_redirects()
    parsed: dict[str, LinkCollector] = {}
    for path in canon["surfaces"]["html"]:
        lc = LinkCollector()
        lc.feed(read(path))
        parsed[path] = lc
    for path, lc in parsed.items():
        broken = []
        for tag, href in lc.links:
            if href.startswith(("http://", "https://", "mailto:", "data:")):
                continue
            if href.startswith("#"):
                if href[1:] and href[1:] not in lc.ids:
                    broken.append(f"{href} (no such id)")
                continue
            target, _, frag = href.partition("#")
            target = target.split("?", 1)[0]  # /styles.css?v=<hash>: the file is the path
            if not target.startswith("/"):
                target = "/" + target
            if not site_path_exists(target, redirects):
                broken.append(href)
                continue
            if frag:
                # a fragment on another page of ours: check it exists there
                other = next((p for p in parsed if "/" + Path(p).stem == target or ("/" == target and p.endswith("index.html"))), None)
                if other and frag not in parsed[other].ids:
                    broken.append(f"{href} (no id {frag!r} in {other})")
        if broken:
            r.add("links", "high", path, "broken internal link(s): " + ", ".join(broken))
        else:
            r.add("links", "info", path, f"{len(lc.links)} links, all internal targets resolve")
        if lc.external_loads:
            r.add("privacy", "high", path, f"loads third-party resources: {lc.external_loads}")
        else:
            r.add("privacy", "info", path, "no third-party resource loads")
        text = read(path)
        if 'data-theme="dark"' in text.split("<body")[0]:
            r.add("identity", "medium", path, "theme forced on <html>; the palette follows the system unless the visitor chooses")
        if "assets/favicon.svg" not in text:
            r.add("identity", "medium", path, "favicon is not the ∵ mark")


# ---------------------------------------------------------------- matrix

def agent_rows(text: str, start_marker: str, end_marker: str | None = None) -> list[str]:
    rows = []
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("|") and not s.startswith("|--") and not s.startswith("| Agent"):
            cell = s.split("|")[1].strip().strip("*")
            rows.append(cell)
        if s.startswith("<tr><td>"):
            m = re.match(r"<tr><td>([^<]+)</td>", s)
            if m:
                rows.append(m.group(1).strip())
    return rows


def check_matrix(canon: dict, r: Report) -> None:
    want = canon["integration_matrix"]["agents"]
    readme = read("README.md")
    html = read("site/index.html")
    got_md = [row for row in agent_rows(readme, "") if any(row.startswith(a) for a in want)]
    got_html = [row for row in agent_rows(html, "") if any(row.startswith(a) for a in want)]
    norm = lambda rows: [next(a for a in want if row.startswith(a)) for row in rows]
    if norm(got_md) != want:
        r.add("matrix", "medium", "README.md", f"integration matrix rows {norm(got_md)} != canon {want}")
    elif norm(got_html) != want:
        r.add("matrix", "medium", "site/index.html", f"integration matrix rows {norm(got_html)} != canon {want}")
    else:
        r.add("matrix", "info", "README.md = site/index.html", f"same agent rows in the same order: {want}")


# ---------------------------------------------------------------- release

def check_assets(canon: dict, r: Report) -> None:
    """Every page references /styles.css and /app.js at their current content
    version (scripts/site_version.py); a stale page would render with a
    browser's day-old cached stylesheet."""
    sys.path.insert(0, str(ROOT / "scripts"))
    import site_version

    stale = [p for p in site_version.pages() if site_version.render(p.read_text(encoding="utf-8")) != p.read_text(encoding="utf-8")]
    if stale:
        for p in stale:
            r.add("assets", "high", str(p.relative_to(ROOT)), "stale /styles.css or /app.js version: run scripts/site_version.py")
    else:
        r.add("assets", "info", "site/**/*.html", f"asset versions current ({len(site_version.pages())} pages)")
    for name in site_version.ASSETS:
        block = re.search(r"^/" + re.escape(name) + r"\n((?:  .*\n?)+)", read("site/_headers"), re.M)
        if not block or "immutable" not in block.group(1):
            r.add("assets", "high", "site/_headers", f"/{name} must be cached immutable: its URL carries the content version")


def check_release(canon: dict, r: Report) -> None:
    for path, needles in canon["release"]["must_mention"].items():
        if not (ROOT / path).exists():
            r.add("release", "high", path, "missing")
            continue
        text = read(path)
        missing = [n for n in needles if n not in text]
        if missing:
            r.add("release", "high", path, f"does not mention {missing}; installers and release disagree on asset names")
        else:
            r.add("release", "info", path, "asset name and checksum file agree with the canon")


# ---------------------------------------------------- evidence invariants
#
# These equivalences are false. A public surface or a machine-readable
# description that states one of them is a high finding: an agent will
# repeat it.
#
#   correlated == declared == cryptographically verified
#   signed == true
#   absence of observed egress == proof of all job egress
#   AI-tagged == AI-written
#   unknown == human

_HUMAN_COLLAPSE = re.compile(r"count(?:s|ed)? as human", re.I)

# Exact slogans that state a stronger proposition than the evidence. A denial
# ("is not", "does not") does not contain these strings.
_STRENGTHEN = (
    "Verified AI survival",
    "Verified AI-tagged",
    "verified AI",
    "verified only",
    "no verified AI",
    "proof of cause",
    "PNX proves",
    "proves what an agent",
    "proves what it did not",
    "Recall proven experience",
    "Verified experience",
    "verified experience",
    "reliable priors",
    "proven experience",
    "authorship proven",
    "proves AI authorship",
    "proves the attribution",
    "proves who typed",
)

# Same-sentence collapses. A "not" inside the match is the correction.
_COLLAPSE = (
    (re.compile(r"(?i)\bunknown\b[^.?\n]{0,48}\b(?:is|means|counts as)\s+human\b"), "unknown stated as human"),
    (re.compile(r"(?i)\bdeclared\b[^.?\n]{0,48}\b(?:is|means)\s+true\b"), "declared stated as true"),
    (re.compile(r"(?i)\bsigned\b[^.?\n]{0,60}\bproves?\s+(?:the\s+)?attribution\b"), "signed bytes stated as true attribution"),
    (re.compile(r"(?i)\bproven\b[^.?\n]{0,48}\bcryptographically\b"), "skill proven stated as cryptographic proof"),
    (re.compile(r"(?i)\bmetadata matched\b[^.?\n]{0,80}\b(?:proves|proven)\b"), "metadata matched stated as authorship proved"),
)


def _collapse_hit(text: str, pattern: re.Pattern[str]) -> str | None:
    for match in pattern.finditer(text):
        if re.search(r"\bnot\b", match.group(0), re.I):
            continue
        return match.group(0)
    return None


def check_evidence_invariants(_canon: dict, r: Report) -> None:
    surfaces = list(_canon["surfaces"]["text"]) + [
        "server.json",
        "plugin/skills/causari/SKILL.md",
        "src/commands/report.rs",
        "src/commands/mcp.rs",
        "src/commands/audit.rs",
        "src/commands/brief.rs",
        "src/commands/seal.rs",
        "src/commands/hook.rs",
        "src/commands/hook/cursor.rs",
        "src/cli.rs",
        "src/skill.rs",
        "src/commands/find.rs",
        "site/verify/verify.js",
        "docs/pnx.md",
        "scripts/survival_report.py",
        "drafts/launch/reddit-r-programming.md",
        "drafts/launch/show-hn.md",
        "drafts/launch/x-thread.md",
    ]
    for path in surfaces:
        if not (ROOT / path).exists():
            r.add("evidence", "high", path, "surface missing")
            continue
        text = read(path)
        if _HUMAN_COLLAPSE.search(text):
            r.add(
                "evidence",
                "high",
                path,
                "unknown collapsed into human ('count as human' or the same words)",
            )
        for slogan in _STRENGTHEN:
            if slogan in text:
                r.add("evidence", "high", path, f"claim stronger than the evidence: {slogan!r}")
        for pattern, why in _COLLAPSE:
            hit = _collapse_hit(text, pattern)
            if hit:
                r.add("evidence", "high", path, f"{why}: {hit!r}")
        if path == "action.yml" and "AI-written code is still alive" in text:
            r.add("evidence", "high", path, "AI-tagged collapsed into AI-written")
        if path == "src/commands/report.rs" and "AI-written lines" in text:
            r.add("evidence", "high", path, "recorded lines described as AI-written")
    llms = read("site/llms.txt")
    for phrase in (
        "UNKNOWN",
        "is not human",
        "declared",
        "correlated",
        "observed",
        "signed is not",
        "does not prove the numbers",
        "`proven` is not awarded",
        "not an execution",
        "not a cryptographic proof",
    ):
        if phrase not in llms:
            r.add("evidence", "high", "site/llms.txt", f"missing evidence-class phrase {phrase!r}")
    readme = read("README.md")
    for phrase in (
        "metadata matched",
        "does not prove a model wrote the line",
        "UNKNOWN",
        "is not human",
        "local ledger",
        "re mcp",
        "does not authenticate",
        "causari.dev/method",
        "llms.txt",
        "curl -fsSL https://causari.dev/install.sh | sh",
        "re audit",
        "no other connection",
    ):
        if phrase not in readme:
            r.add("evidence", "high", "README.md", f"opening omits {phrase!r}")
    # The first section, before the console sample, is what an agent quotes.
    opening = readme.split("```console", 1)[0]
    for phrase in (
        "metadata matched",
        "does not prove a model wrote the line",
        "survival",
        "local ledger",
        "re mcp",
        "does not authenticate",
        "causari.dev/method",
        "llms.txt",
        "curl -fsSL https://causari.dev/install.sh | sh",
        "\nre audit\n",
    ):
        if phrase not in opening:
            r.add("evidence", "high", "README.md", f"first section omits {phrase!r}")
    verify_js = read("site/verify/verify.js")
    if "a.verified" not in verify_js:
        r.add("evidence", "high", "site/verify/verify.js", "stopped reading the JSON field verified")
    if "AI-tagged (metadata matched)" not in verify_js:
        r.add("evidence", "high", "site/verify/verify.js", "display label is not metadata matched")
    for path in ("src/commands/audit.rs", "src/commands/seal.rs", "site/index.html"):
        if "AI-tagged (metadata matched)" not in read(path):
            r.add("evidence", "high", path, "human label is not metadata matched")
    if '"verified"' not in read("src/commands/audit.rs") and "`verified`" not in read("src/commands/audit.rs"):
        r.add("evidence", "high", "src/commands/audit.rs", "stopped naming the JSON field verified")
    desc = json.loads(read("server.json"))["description"]
    if "does not prove" not in desc or "declared" not in desc:
        r.add(
            "evidence",
            "high",
            "server.json",
            "MCP description does not keep declared distinct from proof",
        )
    why = read("src/commands/mcp.rs")
    if "does not prove who typed the line" not in why:
        r.add("evidence", "high", "src/commands/mcp.rs", "causari_why description omits the limit")
    if "`proven` is not awarded" not in why or "not an execution" not in why:
        r.add("evidence", "high", "src/commands/mcp.rs", "causari_recall still treats search as a use")
    if "does not run the command" not in why or "supplied by the recorder" not in why:
        r.add("evidence", "high", "src/commands/mcp.rs", "exit_code is not described as caller-supplied")
    brief = read("src/commands/brief.rs")
    if "`proven` is not awarded" not in brief or "None of this proves the approach was correct" not in brief:
        r.add("evidence", "high", "src/commands/brief.rs", "briefing promotes the recall ladder")
    if "not an execution" not in brief:
        r.add("evidence", "high", "src/commands/brief.rs", "legacy recall count is described as an execution")
    skill = read("plugin/skills/causari/SKILL.md")
    if "`proven` is not awarded" not in skill or "not a cryptographic proof" not in skill:
        r.add("evidence", "high", "plugin/skills/causari/SKILL.md", "skill text promotes proven to a cryptographic proof")
    pnx = read("docs/pnx.md")
    if "no other connection" not in pnx or "witness saw" not in pnx:
        r.add("evidence", "high", "docs/pnx.md", "PNX line does not limit the witness to bodies it saw")
    lede = "AI-tagged means that metadata matched; it does not prove a model wrote the line."
    readme_link = "https://github.com/croviatrust/causari#readme"
    for path in ("scripts/survival_report.py", "site/reports/survival/index.html"):
        page = read(path)
        if lede not in page or readme_link not in page:
            r.add("evidence", "high", path, "survival index does not lead to the README with the metadata limit")
    if not any(f["severity"] == "high" and f["area"] == "evidence" for f in r.findings):
        r.add("evidence", "info", "surfaces", "evidence classes are not collapsed on the scanned surfaces")


# ---------------------------------------------------------------- live

def fetch(url: str, follow: bool = True, plain: bool = False) -> tuple[int, dict, bytes]:
    """`plain=True` sends what any script sends: urllib's own User-Agent
    (`Python-urllib/3.x`), no cookie, no browser hint. That is the client the
    reproducibility claim invites, and the one a Cloudflare Browser Integrity
    Check turns away with a 403."""
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):  # noqa: N802
            return None

    opener = urllib.request.build_opener() if follow else urllib.request.build_opener(NoRedirect)
    headers = {} if plain else {"User-Agent": "causari-canon-audit/1.0"}
    req = urllib.request.Request(url, headers=headers)
    try:
        with opener.open(req, timeout=30) as resp:
            return resp.status, {k.lower(): v for k, v in resp.headers.items()}, resp.read(200_000)
    except urllib.error.HTTPError as e:
        return e.code, {k.lower(): v for k, v in e.headers.items()}, e.read(200_000) if e.fp else b""
    except Exception as e:  # noqa: BLE001
        return 0, {"error": str(e)}, b""


def live_word_present(word: str, text: str) -> bool:
    # Words are matched on word boundaries so a class name such as
    # "screen-more proof-grid" does not read as the retired command "re proof".
    # Non-word tokens (emoji, file names, quoted attributes) are matched raw.
    if re.fullmatch(r"[\w ]+", word):
        return re.search(rf"\b{re.escape(word)}\b", text) is not None
    return word in text


def local_path_for(url_path: str) -> Path:
    p = url_path.lstrip("/")
    if p == "" or p.endswith("/"):
        p += "index.html"
    elif "." not in Path(p).name:
        p += ".html"
    return ROOT / "site" / p


def check_live_words_offline(canon: dict, r: Report) -> None:
    # The same words the weekly live audit refuses, checked against the files
    # that will be deployed, so the pre-push gate fails before the site does.
    live = canon["live"]
    for url_path, marker in live["pages"].items():
        f = local_path_for(url_path)
        if not f.exists():
            continue
        text = f.read_text(encoding="utf-8", errors="replace")
        if marker not in text:
            r.add("wording", "high", str(f.relative_to(ROOT)), f"live marker {marker!r} absent")
            continue
        bad = [w for w in live["forbidden_live_words"] if live_word_present(w, text)]
        if bad:
            r.add("wording", "high", str(f.relative_to(ROOT)), f"forbidden live wording: {bad}")
        else:
            r.add("wording", "info", str(f.relative_to(ROOT)), "live marker present, no forbidden live wording")


def check_live(canon: dict, r: Report) -> None:
    live = canon["live"]
    base = live["base"].rstrip("/")
    for path, marker in live["pages"].items():
        status, headers, body = fetch(base + path)
        text = body.decode("utf-8", errors="replace")
        if status != 200:
            r.add("live", "critical", path, f"HTTP {status} {headers.get('error', '')}")
            continue
        if marker not in text:
            r.add("live", "high", path, f"200 but marker {marker!r} absent; stale deploy?")
        else:
            bad = [w for w in live["forbidden_live_words"] if live_word_present(w, text)]
            if bad:
                r.add("live", "high", path, f"forbidden wording live: {bad}")
            else:
                r.add("live", "info", path, f"200, marker present, {len(body)} bytes")
        for h, want in live["headers"].get(path, {}).items():
            got = headers.get(h, "")
            if want not in got:
                r.add("live", "medium", path, f"header {h}: want {want!r} in {got!r}")
    for src, dst in live["redirects"].items():
        status, headers, _ = fetch(base + src, follow=False)
        loc = headers.get("location", "")
        if status not in (301, 302, 308) or dst not in loc:
            r.add("live", "medium", src, f"expected redirect to …{dst}, got HTTP {status} {loc!r}")
        else:
            r.add("live", "info", src, f"{status} → {loc}")
    # Data paths with a plain script client. The pages above pass with a
    # named User-Agent; a 403 here and 200 there is a bot rule in front of
    # the data, and the "reproducible by anyone" claim is false for scripts.
    for path, marker in (live.get("data") or {}).get("paths", {}).items():
        status, headers, body = fetch(base + path, plain=True)
        text = body.decode("utf-8", errors="replace")
        if status == 403:
            r.add("live", "critical", path,
                  "HTTP 403 for a plain script client (Python-urllib): a bot rule blocks the data; "
                  "exempt this path from the Cloudflare Browser Integrity Check")
        elif status != 200:
            r.add("live", "critical", path, f"HTTP {status} for a plain script client {headers.get('error', '')}")
        elif marker not in text:
            r.add("live", "high", path, f"200 but marker {marker!r} absent")
        else:
            r.add("live", "info", path, f"200 for a plain script client, {len(body)} bytes")


# ---------------------------------------------------------------- main

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--gate", action="store_true", help="exit 1 on any critical or high finding")
    ap.add_argument("--live", action="store_true", help="also audit the deployed site")
    ap.add_argument("--json", action="store_true", help="print findings as JSON")
    ap.add_argument("--quiet", action="store_true", help="hide info lines")
    args = ap.parse_args()

    canon = json.loads(read("canon/canon.json"))
    r = Report()
    for check in (check_versions, check_forbidden, check_live_words_offline, check_required, check_files, check_html, check_assets, check_matrix, check_release, check_evidence_invariants):
        try:
            check(canon, r)
        except Exception as e:  # noqa: BLE001
            r.add("tool", "critical", check.__name__, f"check crashed: {e!r}")
    if args.live:
        check_live(canon, r)

    if args.json:
        print(json.dumps(r.findings, indent=1, ensure_ascii=False))
    else:
        width = max(len(f["where"]) for f in r.findings) if r.findings else 10
        for f in sorted(r.findings, key=lambda f: (SEVERITIES.index(f["severity"]), f["area"], f["where"])):
            if args.quiet and f["severity"] == "info":
                continue
            print(f"{f['severity']:<8} {f['area']:<9} {f['where']:<{width}}  {f['message']}")
        print()
        print("  ".join(f"{s}={r.count(s)}" for s in SEVERITIES))
    if args.gate and (r.count("critical") or r.count("high")):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
