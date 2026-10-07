#!/usr/bin/env python3
"""Tests for scripts/survival_report.py and scripts/zenodo_deposit.py.

Standard library only (unittest); pytest runs them too:

    python3 -m unittest scripts/tests/test_survival_report.py
    python3 -m pytest scripts/tests -q

Every test builds a report from a synthetic run directory into a scratch
site, so nothing under site/ is touched.
"""

from __future__ import annotations

import json
import re
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from html.parser import HTMLParser
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPTS = HERE.parent
ROOT = SCRIPTS.parent
sys.path.insert(0, str(SCRIPTS))

import survival_report as sr  # noqa: E402
import zenodo_deposit as zd  # noqa: E402

CANON = json.loads((ROOT / "canon" / "canon.json").read_text(encoding="utf-8"))
FORBIDDEN = CANON["forbidden_words"]["words"]


def stat(commits: int, introduced: int, surviving: int) -> dict:
    rate = surviving / introduced if introduced else None
    return {
        "commits": commits, "introduced": introduced, "surviving": surviving, "survival_rate": rate,
        "median_survival": rate, "capped_survival_rate": rate, "cap_lines": 100 if introduced else None,
        "largest_commit_share": 0.3 if introduced else None,
    }


def audit(commits: int, introduced: int, surviving: int, agent: str = "claude-code", shallow: bool = False) -> dict:
    return {
        "method": "v2", "total_commits": commits * 10,
        "verified": stat(commits, introduced, surviving), "probable": stat(0, 0, 0),
        "by_agent": {agent: stat(commits, introduced, surviving)},
        "coverage": {"method": "v2", "blame_flags": ["-w", "-M", "-C"], "ignore_revs_file": False,
                     "shallow": shallow, "sample_floor": 5, "small_sample": commits < 5},
    }


def window(from_days: int, to_days: int | None, tagged: tuple[int, int, int], untagged: tuple[int, int, int]) -> dict:
    def cohort(c: int, i: int, s: int) -> dict:
        return {"commits": c, "introduced": i, "surviving": s, "survival_rate": (s / i) if i else None}
    return {"from_days": from_days, "to_days": to_days, "tagged": cohort(*tagged), "untagged": cohort(*untagged)}


def audit_v3(commits: int, introduced: int, surviving: int, *, agent: str = "claude-code",
             by_age: list[dict], gap: dict | None, oldest: dict | None, total_commits: int | None = None) -> dict:
    """A method v3 audit: the v2 figures plus a `baseline` block as `re audit` writes it."""
    a = audit(commits, introduced, surviving, agent=agent)
    a["method"] = "v3"
    a["coverage"]["method"] = "v3"
    if total_commits is not None:
        a["total_commits"] = total_commits
    u_intro = sum(w["untagged"]["introduced"] for w in by_age)
    u_surv = sum(w["untagged"]["surviving"] for w in by_age)
    u_commits = sum(w["untagged"]["commits"] for w in by_age)
    a["baseline"] = {
        "untagged": stat(u_commits, u_intro, u_surv),
        "by_age": by_age,
        "age_matched": gap,
        "oldest_surviving": oldest,
    }
    return a


# Two comparable windows: tagged 90 % and 40 %, untagged 80 % and 60 %, equal
# tagged line weights, so the age-matched untagged rate is 70 % and the gap
# is 65 - 70 = -5 points; `re audit` computed these from the same inputs.
V3_BY_AGE = [
    window(0, 30, (5, 500, 450), (5, 500, 400)),
    window(30, 90, (0, 0, 0), (0, 0, 0)),
    window(90, 180, (5, 500, 200), (5, 500, 300)),
    window(180, 365, (0, 0, 0), (0, 0, 0)),
    window(365, 730, (1, 1000, 0), (0, 0, 0)),
    window(730, None, (0, 0, 0), (0, 0, 0)),
]
V3_GAP = {"tagged_rate": 0.65, "untagged_rate": 0.70, "gap": -0.05, "buckets_used": 2, "tagged_lines_covered": 0.5}
V3_OLDEST = {"date": "2026-05-01", "age_days": 120, "commits_before": 1, "introduced_before": 1000,
             "tagged_commits_before": 1, "tagged_introduced_before": 1000}
# A cleared repository: 12 of its 22 commits predate the oldest surviving line.
CLEARED_BY_AGE = [
    window(0, 30, (0, 0, 0), (0, 0, 0)),
    window(30, 90, (5, 100, 90), (5, 400, 300)),
    window(90, 180, (0, 0, 0), (0, 0, 0)),
    window(180, 365, (6, 600, 0), (6, 600, 0)),
    window(365, 730, (0, 0, 0), (0, 0, 0)),
    window(730, None, (0, 0, 0), (0, 0, 0)),
]
CLEARED_GAP = {"tagged_rate": 90 / 700, "untagged_rate": 75 / 700, "gap": 15 / 700, "buckets_used": 2, "tagged_lines_covered": 1.0}
CLEARED_OLDEST = {"date": "2026-07-27", "age_days": 60, "commits_before": 12, "introduced_before": 1200,
                  "tagged_commits_before": 6, "tagged_introduced_before": 600}

V3_REPOS = {
    "zeta/last": audit(20, 1000, 600),
    "base/line": audit_v3(11, 2000, 650, by_age=V3_BY_AGE, gap=V3_GAP, oldest=V3_OLDEST, total_commits=22),
    "base/cleared": audit_v3(11, 700, 90, agent="openhands", by_age=CLEARED_BY_AGE, gap=CLEARED_GAP,
                             oldest=CLEARED_OLDEST, total_commits=22),
    "base/lonely": audit_v3(5, 300, 200, by_age=[window(0, 30, (5, 300, 200), (2, 50, 50))], gap=None,
                            oldest={"date": "2026-09-01", "age_days": 3, "commits_before": 0, "introduced_before": 0,
                                    "tagged_commits_before": 0, "tagged_introduced_before": 0}, total_commits=7),
}


REPOS = {
    "zeta/last": audit(20, 1000, 600),
    "Alpha/first": audit(10, 500, 400, agent="cursor"),
    "mid/one": audit(12, 800, 200, agent="aider"),
    "mid/small": audit(3, 50, 10, agent="aider"),
    "mid/shallow": audit(30, 900, 100, shallow=True),
    "opt/out": audit(9, 100, 50, agent="devin"),
}


class Scratch:
    """A run directory, an empty site and a repo root with an opt-out file."""

    def __init__(self, repos: dict | None = None) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.run = base / "run"
        self.site = base / "site"
        self.root = base / "root"
        self.run.mkdir()
        self.site.mkdir()
        (self.root / ".github").mkdir(parents=True)
        for repo, a in (repos or REPOS).items():
            (self.run / f"{sr.repo_slug(repo)}.json").write_text(json.dumps(a), encoding="utf-8")
        (self.run / "run.json").write_text(json.dumps({
            "generated_at": "2026-09-21T05:17:00Z", "tool": "causari", "tool_version": "0.1.5", "method": "v2",
            "command": "re audit <owner/repo> --json", "repos": list(REPOS), "failed": ["gone/repo"], "opted_out": ["skipped/early"],
        }), encoding="utf-8")
        (self.root / ".github" / "survival-optout.txt").write_text("# comment\n\nOPT/out\n", encoding="utf-8")
        # the static parts of the real site the generator amends
        (self.site / "_redirects").write_text("/github https://github.com/croviatrust/causari 302\n", encoding="utf-8")
        (self.site / "sitemap.xml").write_text(
            '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
            "  <url><loc>https://causari.dev/</loc></url>\n</urlset>\n", encoding="utf-8")

    def build(self, number: int = 1, date: str = "2026-09-21") -> dict:
        rc = sr.main(["--site", str(self.site), "--root", str(self.root), "build", "--run", str(self.run),
                      "--number", str(number), "--date", date, "--no-png"])
        assert rc == 0
        return json.loads((self.site / "reports" / "survival" / f"{date[:4]}" / f"{number:02d}" / "report.json").read_text(encoding="utf-8"))

    def close(self) -> None:
        self.tmp.cleanup()


class TextOnly(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.parts: list[str] = []

    def handle_data(self, data: str) -> None:
        self.parts.append(data)


def visible_text(html: str) -> str:
    p = TextOnly()
    p.feed(html)
    return "".join(p.parts)


class BuildTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.s = Scratch()
        cls.f = cls.s.build()
        cls.dir = cls.s.site / "reports" / "survival" / "2026" / "01"
        cls.page = (cls.dir / "index.html").read_text(encoding="utf-8")
        cls.archive = (cls.s.site / "reports" / "survival" / "index.html").read_text(encoding="utf-8")

    @classmethod
    def tearDownClass(cls) -> None:
        cls.s.close()

    def test_alphabetical_order_never_by_rate(self) -> None:
        repos = [r["repo"] for r in self.f["repositories"]]
        self.assertEqual(repos, ["Alpha/first", "mid/one", "zeta/last"])
        self.assertEqual(repos, sorted(repos, key=str.lower))
        rates = [r["verified"]["survival_rate"] for r in self.f["repositories"]]
        self.assertNotEqual(rates, sorted(rates))
        self.assertNotEqual(rates, sorted(rates, reverse=True))
        # and the page shows them in the same order
        positions = [self.page.index(f">{r}</a>") for r in repos]
        self.assertEqual(positions, sorted(positions))

    def test_small_sample_measured_but_not_aggregated(self) -> None:
        self.assertEqual([r["repo"] for r in self.f["not_aggregated"]], ["mid/small"])
        self.assertNotIn("mid/small", [r["repo"] for r in self.f["repositories"]])
        self.assertEqual(self.f["aggregate"]["repositories"], 3)
        self.assertEqual(self.f["aggregate"]["introduced"], 1000 + 500 + 800)
        self.assertEqual(self.f["aggregate"]["surviving"], 600 + 400 + 200)
        self.assertIn("Measured but not aggregated", self.page)

    def test_shallow_excluded_with_note(self) -> None:
        self.assertEqual(self.f["excluded"]["shallow"], ["mid/shallow"])
        for group in ("repositories", "not_aggregated"):
            self.assertNotIn("mid/shallow", [r["repo"] for r in self.f[group]])
        self.assertIn("mid/shallow", self.page)
        self.assertIn("Shallow clones", self.page)

    def test_optout_honoured(self) -> None:
        for group in ("repositories", "not_aggregated"):
            self.assertNotIn("opt/out", [r["repo"] for r in self.f[group]])
        self.assertNotIn("opt/out", self.page)
        self.assertEqual(self.f["excluded"]["opted_out"], 2)  # one from the run dir, one the workflow skipped
        self.assertFalse((self.dir / "repos" / "opt__out.json").exists())

    def test_failed_listed(self) -> None:
        self.assertEqual(self.f["excluded"]["failed"], ["gone/repo"])
        self.assertIn("gone/repo", self.page)

    def test_no_rank_column_no_colour(self) -> None:
        header = re.search(r'<table class="lb-table" id="repos">.*?</thead>', self.page, re.S).group(0)
        self.assertNotIn("Rank", header)
        self.assertNotIn("#</th>", header)
        for token in ("color:", "background:", "🟢", "🟡", "🔴", "class=\"good\"", "class=\"bad\""):
            self.assertNotIn(token, self.page)

    def test_forbidden_words_absent_from_generated_html(self) -> None:
        for html in (self.page, self.archive):
            text = visible_text(html)
            for word in FORBIDDEN:
                for m in re.finditer(re.escape(word), text):
                    before = text[max(0, m.start() - 1):m.start()]
                    self.assertIn(before, ('"', "'", "\u201c", "\u2018", "`", "\u00ab"),
                                  f"forbidden word {word!r} used bare in generated HTML")

    def test_every_number_links_to_reproduction(self) -> None:
        for r in self.f["repositories"] + self.f["not_aggregated"]:
            self.assertEqual(r["reproduce"], f"re audit {r['repo']} --json")
            self.assertTrue((self.dir / r["audit_file"]).exists())
            self.assertIn(f'href="{r["audit_file"]}"', self.page)
            self.assertIn(f"re audit {r['repo']} --json", self.page)

    def test_method_section(self) -> None:
        self.assertIn('href="/method"', self.page)
        self.assertIn("method v2", self.page)
        self.assertIn("causari 0.1.5", self.page)
        self.assertIn("git blame -w -M -C", self.page)
        self.assertIn("95th percentile", self.page)
        self.assertIn("10,000 lines", self.page)
        self.assertIn("counts, not grades", self.page)
        self.assertIn("survival-optout.txt", self.page)

    def test_interval_labelled_as_sample_not_population(self) -> None:
        note = self.f["aggregate"]["interval_method"]["note"]
        self.assertIn("sampled repositories", note)
        self.assertIn("not all AI-assisted code", note)
        self.assertIn("seed 1", note)
        self.assertEqual(self.f["aggregate"]["interval_method"]["resamples"], 2000)

    def test_by_agent_alphabetical_and_summed(self) -> None:
        self.assertEqual(list(self.f["by_agent"]), ["aider", "claude-code", "cursor"])
        self.assertEqual(self.f["by_agent"]["aider"]["introduced"], 800)  # mid/small is not aggregated

    def test_outputs_exist(self) -> None:
        for name in ("index.html", "report.json", "report.md", "card.svg"):
            self.assertTrue((self.dir / name).exists(), name)
        base = self.s.site / "reports" / "survival"
        for name in ("index.html", "feed.xml", "latest.json"):
            self.assertTrue((base / name).exists(), name)
        latest = json.loads((base / "latest.json").read_text(encoding="utf-8"))
        self.assertEqual(latest["id"], "2026/01")
        ET.fromstring((self.dir / "card.svg").read_text(encoding="utf-8"))

    def test_atom_feed_valid(self) -> None:
        ns = {"a": "http://www.w3.org/2005/Atom"}
        root = ET.parse(self.s.site / "reports" / "survival" / "feed.xml").getroot()
        self.assertEqual(root.tag, "{http://www.w3.org/2005/Atom}feed")
        for tag in ("title", "id", "updated"):
            self.assertIsNotNone(root.find(f"a:{tag}", ns), tag)
        self.assertEqual(root.find("a:link[@rel='self']", ns).get("href"), "https://causari.dev/reports/survival/feed.xml")
        entries = root.findall("a:entry", ns)
        self.assertEqual(len(entries), 1)
        e = entries[0]
        self.assertEqual(e.find("a:id", ns).text, "https://causari.dev/reports/survival/2026/01/")
        self.assertTrue(e.find("a:link[@rel='alternate']", ns).get("href").startswith("https://causari.dev/"))
        self.assertIn("Survival Report #1", e.find("a:title", ns).text)

    def test_redirects_and_sitemap(self) -> None:
        redirects = (self.s.site / "_redirects").read_text(encoding="utf-8")
        for pattern in (r"^/survival\s+/reports/survival/\s+301$",
                        r"^/report\s+/reports/survival/2026/01/\s+302$",
                        r"^/survival-data\.json\s+/reports/survival/latest\.json\s+301$"):
            self.assertIsNotNone(re.search(pattern, redirects, re.M), pattern)
        self.assertIn("/github https://github.com/croviatrust/causari 302", redirects)  # untouched
        sitemap = ET.parse(self.s.site / "sitemap.xml").getroot()
        locs = [u.find("{http://www.sitemaps.org/schemas/sitemap/0.9}loc").text for u in sitemap]
        self.assertIn("https://causari.dev/reports/survival/", locs)
        self.assertIn("https://causari.dev/reports/survival/2026/01/", locs)
        self.assertIn("https://causari.dev/", locs)

    def test_rebuild_is_idempotent(self) -> None:
        before = {p.name: p.read_bytes() for p in (self.s.site / "reports" / "survival").iterdir() if p.is_file()}
        sr.rebuild(self.s.site)
        after = {p.name: p.read_bytes() for p in (self.s.site / "reports" / "survival").iterdir() if p.is_file()}
        self.assertEqual(before, after)
        redirects = (self.s.site / "_redirects").read_text(encoding="utf-8")
        self.assertEqual(redirects.count(sr.REDIRECT_BEGIN), 1)


class BaselineTests(unittest.TestCase):
    """Method v3 rows carry a baseline; v2 rows in the same report do not."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.s = Scratch(V3_REPOS)
        cls.f = cls.s.build()
        cls.dir = cls.s.site / "reports" / "survival" / "2026" / "01"
        cls.page = (cls.dir / "index.html").read_text(encoding="utf-8")
        cls.md = (cls.dir / "report.md").read_text(encoding="utf-8")
        cls.repo_page = (cls.s.site / "r" / "base" / "line" / "index.html").read_text(encoding="utf-8")
        cls.v2_page = (cls.s.site / "r" / "zeta" / "last" / "index.html").read_text(encoding="utf-8")

    @classmethod
    def tearDownClass(cls) -> None:
        cls.s.close()

    def row(self, repo: str) -> dict:
        return next(r for r in self.f["repositories"] + self.f["not_aggregated"] if r["repo"] == repo)

    def test_rows_carry_the_baseline_or_none(self) -> None:
        b = self.row("base/line")["baseline"]
        self.assertEqual(b["untagged"]["commits"], 10)
        self.assertEqual(b["untagged"]["introduced"], 1000)
        self.assertEqual(len(b["by_age"]), 6)
        self.assertAlmostEqual(b["age_matched"]["gap"], -0.05)
        self.assertEqual(b["oldest_surviving"]["commits_before"], 1)
        self.assertAlmostEqual(b["oldest_surviving"]["commits_before_share"], 1 / 22)
        self.assertIsNone(self.row("zeta/last")["baseline"])
        self.assertIsNone(self.row("base/lonely")["baseline"]["age_matched"])

    def test_aggregate_baseline(self) -> None:
        b = self.f["aggregate"]["baseline"]
        self.assertEqual(b["repositories"], 3)
        self.assertEqual(b["repositories_with_gap"], 2)
        self.assertAlmostEqual(b["median_gap"], (-0.05 + 15 / 700) / 2)
        self.assertEqual((b["gaps_negative"], b["gaps_positive"]), (1, 1))
        self.assertEqual(b["rewritten"], ["base/cleared"])
        self.assertIn("more than 50%", b["rewritten_rule"])
        # pooled 0-30 d window: base/line (5, 500, 450) + base/lonely (5, 300, 200) tagged;
        # untagged (5, 500, 400) + (2, 50, 50)
        w0 = b["pooled_by_age"][0]
        self.assertEqual((w0["from_days"], w0["to_days"]), (0, 30))
        self.assertEqual(w0["tagged"], {"commits": 10, "introduced": 800, "surviving": 650, "survival_rate": 650 / 800})
        self.assertEqual(w0["untagged"]["commits"], 7)
        self.assertEqual(w0["untagged"]["introduced"], 550)
        self.assertNotIn("pooled_age_matched", b)
        self.assertIn("no gap is computed from these rows", b["pooled_by_age_note"])
        self.assertIn("negative gap", b["definition"])
        iv = b["median_gap_interval_95"]
        self.assertAlmostEqual(iv["low"], -0.05)
        self.assertAlmostEqual(iv["high"], 15 / 700)

    def test_report_page_shows_gap_column_and_baseline_section(self) -> None:
        header = re.search(r'<table class="lb-table" id="repos">.*?</thead>', self.page, re.S).group(0)
        self.assertIn("<th>Untagged, matched windows</th><th>Gap</th>", header)
        self.assertIn(">-5.0 pts<", self.page)
        self.assertIn(">70.0 %<", self.page)
        self.assertIn("no shared window", self.page)  # base/lonely
        self.assertIn("Measured before method v3", self.page)  # zeta/last
        self.assertIn("· rewritten", self.page)
        self.assertIn('id="baseline"', self.page)
        self.assertIn("Cleared or rewritten", self.page)
        self.assertIn("base/cleared", self.page)
        self.assertIn("median age-matched gap", self.page)
        self.assertIn("-1.4 pts", self.page)
        self.assertIn('id="by-age"', self.page)
        self.assertIn("0–30 d", self.page)
        # still no rank, no colour, no verdict
        for token in ("color:", "background:", "🟢", "🟡", "🔴"):
            self.assertNotIn(token, self.page)
        text = visible_text(self.page)
        for word in FORBIDDEN:
            for m in re.finditer(re.escape(word), text):
                before = text[max(0, m.start() - 1):m.start()]
                self.assertIn(before, ('"', "'", "\u201c", "\u2018", "`", "\u00ab"), f"forbidden word {word!r} bare")

    def test_report_md_has_baseline(self) -> None:
        self.assertIn("## Baseline: the same repositories' untagged lines, in the matched age windows", self.md)
        self.assertIn("- Median gap across them: -1.4 pts", self.md)
        self.assertIn("- Gaps below zero: 1 · above zero: 1", self.md)
        self.assertIn("Cleared or rewritten", self.md)
        self.assertIn("| base/cleared · rewritten |", self.md)
        self.assertIn("| Untagged, matched windows | Gap |", self.md.replace("|  Untagged", "| Untagged"))
        self.assertIn("| 70.0 % | -5.0 pts |", self.md)
        self.assertIn("| — | — |", self.md)  # zeta/last, no baseline

    def test_repo_page_and_latest_json(self) -> None:
        self.assertIn("Baseline: this repository", self.repo_page)
        self.assertIn("a gap of -5.0 pts", self.repo_page)
        self.assertIn("The oldest line still at HEAD dates 2026-05-01; 1 commits", self.repo_page)
        self.assertIn('href="/method#v3"', self.repo_page)
        self.assertNotIn("Baseline: this repository", self.v2_page)
        latest = json.loads((self.s.site / "r" / "base" / "line" / "latest.json").read_text(encoding="utf-8"))
        self.assertAlmostEqual(latest["baseline"]["age_matched"]["gap"], -0.05)
        v2 = json.loads((self.s.site / "r" / "zeta" / "last" / "latest.json").read_text(encoding="utf-8"))
        self.assertIsNone(v2["baseline"])
        cleared = (self.s.site / "r" / "base" / "cleared" / "index.html").read_text(encoding="utf-8")
        self.assertIn("cleared or rewritten", cleared)

    def test_rebuild_from_report_json_keeps_the_baseline(self) -> None:
        sr.rebuild(self.s.site)
        page = (self.dir / "index.html").read_text(encoding="utf-8")
        self.assertEqual(page, self.page)


class RepoPageTests(unittest.TestCase):
    """site/r/<owner>/<repo>/: page, badge, latest.json; site/r/index.html."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.s = Scratch()
        cls.f = cls.s.build()
        cls.r = cls.s.site / "r"
        cls.page = (cls.r / "alpha" / "first" / "index.html").read_text(encoding="utf-8")
        cls.index = (cls.r / "index.html").read_text(encoding="utf-8")
        cls.report_page = (cls.s.site / "reports" / "survival" / "2026" / "01" / "index.html").read_text(encoding="utf-8")

    @classmethod
    def tearDownClass(cls) -> None:
        cls.s.close()

    def test_tree_for_every_measured_repository_and_no_other(self) -> None:
        measured = [r["repo"] for r in self.f["repositories"] + self.f["not_aggregated"]]
        self.assertEqual(sorted(measured, key=str.lower), ["Alpha/first", "mid/one", "mid/small", "zeta/last"])
        for repo in measured:
            d = self.r / repo.lower()
            for name in ("index.html", "badge.svg", "badge-dark.svg", "latest.json"):
                self.assertTrue((d / name).exists(), f"{repo}: {name}")
        for absent in ("mid/shallow", "opt/out", "gone/repo"):
            self.assertFalse((self.r / absent).exists(), absent)
        self.assertFalse((self.r / "Alpha").exists(), "paths are lowercase only")

    def test_lowercase_paths_and_cased_redirect(self) -> None:
        self.assertEqual(sr.repo_path("Alpha/first"), "/r/alpha/first/")
        self.assertEqual(sr.repo_path("mid/one"), "/r/mid/one/")
        redirects = (self.s.site / "_redirects").read_text(encoding="utf-8")
        self.assertIsNotNone(re.search(r"^/r/Alpha/first/\s+/r/alpha/first/\s+301$", redirects, re.M))
        self.assertNotIn("/r/mid/one/ ", redirects)  # already lowercase: no redirect needed
        for href in re.findall(r'href="(/r/[^"]+)"', self.report_page + self.index + self.page):
            self.assertEqual(href, href.lower(), href)
        # the report page's repository names link to the repository pages; the numbers still link to the bytes
        self.assertIn('href="/r/alpha/first/"', self.report_page)
        self.assertIn('href="repos/Alpha__first.json"', self.report_page)

    def test_page_with_one_report(self) -> None:
        self.assertIn('<h1><a href="https://github.com/Alpha/first" rel="noopener" translate="no">Alpha/first</a></h1>', self.page)
        self.assertIn("In Survival Report #1 (2026-09-21, method v2): 400 of 500 lines introduced by 10 AI-tagged commits are still at HEAD, 80.0 %.", self.page)
        self.assertIn("re audit Alpha/first --json", self.page)
        self.assertIn('href="/reports/survival/2026/01/repos/Alpha__first.json"', self.page)
        self.assertIn("[![AI code survival](https://causari.dev/r/alpha/first/badge.svg)](https://causari.dev/r/alpha/first/)", self.page)
        self.assertIn('data-copy="badge-md"', self.page)
        self.assertIn("cursor", self.page)  # by agent
        self.assertNotIn('class="spark"', self.page)  # one point is not a line
        self.assertEqual(self.page.count("<tr><td><a href=\"/reports/survival/"), 1)  # one history row
        self.assertIn("A line appears once the repository has been measured in two reports.", self.page)
        self.assertIn('"@type": "Dataset"', self.page)
        self.assertIn('"codeRepository": "https://github.com/Alpha/first"', self.page)
        for m in re.finditer(r"<code\b[^>]*>", self.page):
            self.assertIn('translate="no"', m.group(0), m.group(0))
        for token in ("color:", "background:", "🟢", "🟡", "🔴", "Rank", "rank "):
            self.assertNotIn(token, self.page)
        for other in ("mid/one", "zeta/last", "mid/small"):
            self.assertNotIn(other, self.page)  # no comparison with other repositories
        text = visible_text(self.page)
        for word in FORBIDDEN:
            self.assertNotIn(word, text)

    def test_small_sample_page_publishes_counts_not_ratio(self) -> None:
        page = (self.r / "mid" / "small" / "index.html").read_text(encoding="utf-8")
        self.assertIn("10 of 50 lines introduced by 3 AI-tagged commits are still at HEAD. Fewer than 5 AI-tagged commits", page)
        self.assertNotIn("20.0 %", page)
        self.assertIn('<span class="n">n &lt; 5</span>', page)
        self.assertNotIn("n < 5", page)
        badge = (self.r / "mid" / "small" / "badge.svg").read_text(encoding="utf-8")
        self.assertIn("AI code survival  n &lt; 5", badge)
        self.assertNotIn("20.0", badge)

    def test_badge_text_width_and_colours(self) -> None:
        light = (self.r / "alpha" / "first" / "badge.svg").read_text(encoding="utf-8")
        dark = (self.r / "alpha" / "first" / "badge-dark.svg").read_text(encoding="utf-8")
        root = ET.fromstring(light)
        ns = "{http://www.w3.org/2000/svg}"
        texts = [t.text for t in root.iter(f"{ns}text")]
        self.assertEqual(texts, ["AI code survival  80.0 %", "causari · #1"])
        left = round(len(texts[0]) * sr.BADGE_CHAR + 2 * sr.BADGE_PAD)
        right = round(len(texts[1]) * sr.BADGE_CHAR + 2 * sr.BADGE_PAD)
        self.assertEqual(int(root.get("width")), left + right)
        self.assertEqual(int(root.get("height")), 20)
        title = root.find(f"{ns}title").text
        self.assertIn("Alpha/first: In Survival Report #1 (2026-09-21, method v2): 400 of 500 lines", title)
        colours = set(re.findall(r'fill="(#[0-9a-f]{6})"', light + dark))
        self.assertEqual(colours, {sr.INK, sr.PAPER})
        for word in ("red", "green", "#4c1", "#e05d44", "stroke="):
            self.assertNotIn(word, light + dark)
        # dark is the same badge with the two values swapped
        self.assertEqual(dark.replace(sr.INK, "X").replace(sr.PAPER, sr.INK).replace("X", sr.PAPER), light)
        self.assertIn("monospace", root.find(f"{ns}g").get("font-family"))
        ET.fromstring(dark)

    def test_latest_json_schema(self) -> None:
        latest = json.loads((self.r / "alpha" / "first" / "latest.json").read_text(encoding="utf-8"))
        self.assertEqual(latest["schema"], sr.REPO_SCHEMA)
        self.assertEqual(latest["repo"], "Alpha/first")
        self.assertEqual(latest["url"], "https://causari.dev/r/alpha/first/")
        self.assertEqual(latest["badge"], "https://causari.dev/r/alpha/first/badge.svg")
        self.assertEqual(latest["report"], {"number": 1, "id": "2026/01", "date": "2026-09-21", "url": "https://causari.dev/reports/survival/2026/01/"})
        self.assertEqual(latest["method"], "v2")
        self.assertEqual(latest["verified"]["introduced"], 500)
        self.assertEqual(latest["verified"]["surviving"], 400)
        self.assertAlmostEqual(latest["verified"]["survival_rate"], 0.8)
        self.assertIn("interval_95", latest)
        self.assertTrue(latest["aggregated"])
        self.assertEqual(latest["reports"], 1)
        self.assertEqual(latest["bytes"], "https://causari.dev/reports/survival/2026/01/repos/Alpha__first.json")
        self.assertEqual(latest["reproduce"], "re audit Alpha/first --json")
        self.assertFalse(json.loads((self.r / "mid" / "small" / "latest.json").read_text(encoding="utf-8"))["aggregated"])

    def test_index_lists_every_repository_alphabetically(self) -> None:
        repos = ["Alpha/first", "mid/one", "mid/small", "zeta/last"]
        for repo in repos:
            self.assertIn(f'href="{sr.repo_path(repo)}"', self.index)
            self.assertIn(f">{repo}</a>", self.index)
        positions = [self.index.index(f">{r}</a>") for r in repos]
        self.assertEqual(positions, sorted(positions))
        self.assertIn("counts, not grades", self.index)
        self.assertIn("n &lt; 5", self.index)
        self.assertIn("80.0 %", self.index)
        self.assertNotIn("Rank", self.index)
        text = visible_text(self.index)
        for word in FORBIDDEN:
            self.assertNotIn(word, text)
        # linked from the archive and listed in the sitemap
        archive = (self.s.site / "reports" / "survival" / "index.html").read_text(encoding="utf-8")
        self.assertIn('<a href="/r/">Every repository has a page and a badge</a>', archive)
        sitemap = ET.parse(self.s.site / "sitemap.xml").getroot()
        locs = [u.find("{http://www.sitemaps.org/schemas/sitemap/0.9}loc").text for u in sitemap]
        self.assertIn("https://causari.dev/r/", locs)
        self.assertIn("https://causari.dev/r/alpha/first/", locs)

    def test_second_report_adds_history_and_sparkline(self) -> None:
        s = Scratch()
        try:
            s.build(number=1, date="2026-09-21")
            # the second run measures a different HEAD: fewer surviving lines
            (s.run / "Alpha__first.json").write_text(json.dumps(audit(12, 600, 300, agent="cursor")), encoding="utf-8")
            s.build(number=2, date="2026-09-28")
            page = (s.site / "r" / "alpha" / "first" / "index.html").read_text(encoding="utf-8")
            self.assertIn("In Survival Report #2 (2026-09-28, method v2): 300 of 600 lines introduced by 12 AI-tagged commits are still at HEAD, 50.0 %.", page)
            self.assertIn('class="spark"', page)
            self.assertIn("#1 80.0 %; #2 50.0 %", page)
            rows = re.findall(r'<tr><td><a href="/reports/survival/(\d{4}/\d{2})/">', page)
            self.assertEqual(rows, ["2026/01", "2026/02"])  # oldest first
            badge = (s.site / "r" / "alpha" / "first" / "badge.svg").read_text(encoding="utf-8")
            self.assertIn("AI code survival  50.0 %", badge)
            self.assertIn("causari · #2", badge)
            latest = json.loads((s.site / "r" / "alpha" / "first" / "latest.json").read_text(encoding="utf-8"))
            self.assertEqual(latest["report"]["number"], 2)
            self.assertEqual(latest["reports"], 2)
            # a repository measured once keeps one row and no line
            other = (s.site / "r" / "mid" / "one" / "index.html").read_text(encoding="utf-8")
            self.assertIn('class="spark"', other)  # measured in both reports
            self.assertEqual(sr.sparkline_svg([("1", 0.5)]), "")
            self.assertEqual(sr.sparkline_svg([("1", 0.5), ("2", None)]), "")
            self.assertIn("<polyline", sr.sparkline_svg([("1", 0.5), ("2", 0.6)]))
        finally:
            s.close()

    def test_rebuild_is_idempotent_for_repo_tree(self) -> None:
        snap = lambda: {str(p.relative_to(self.r)): p.read_bytes() for p in self.r.rglob("*") if p.is_file()}
        before = snap()
        sr.rebuild(self.s.site)
        self.assertEqual(before, snap())
        self.assertEqual(self.report_page, (self.s.site / "reports" / "survival" / "2026" / "01" / "index.html").read_text(encoding="utf-8"))


class IntervalTests(unittest.TestCase):
    def test_reproducible_with_seed(self) -> None:
        pairs = [(1000 + 37 * k, 600 - 41 * k + 13 * (k % 3)) for k in range(12)]
        a = sr.bootstrap_rate(pairs, seed=7)
        b = sr.bootstrap_rate(pairs, seed=7)
        self.assertEqual(a, b)
        c = sr.bootstrap_rate(pairs, seed=8)
        self.assertNotEqual(a, c)
        self.assertLessEqual(a["low"], sum(s for _, s in pairs) / sum(i for i, _ in pairs))
        self.assertGreaterEqual(a["high"], sum(s for _, s in pairs) / sum(i for i, _ in pairs))

    def test_same_report_number_same_bytes(self) -> None:
        s1, s2 = Scratch(), Scratch()
        try:
            f1, f2 = s1.build(number=3), s2.build(number=3)
            self.assertEqual(f1["aggregate"], f2["aggregate"])
            self.assertEqual(f1["aggregate"]["interval_method"]["seed"], 3)
            j1 = (s1.site / "reports" / "survival" / "2026" / "03" / "report.json").read_bytes()
            j2 = (s2.site / "reports" / "survival" / "2026" / "03" / "report.json").read_bytes()
            self.assertEqual(j1, j2)
        finally:
            s1.close()
            s2.close()

    def test_no_interval_for_one_repository(self) -> None:
        self.assertIsNone(sr.bootstrap_rate([(100, 50)], seed=1))
        self.assertIsNone(sr.bootstrap_median([0.5], seed=1))


class GuardTests(unittest.TestCase):
    def test_refuses_to_overwrite_a_different_report(self) -> None:
        s = Scratch()
        try:
            s.build(number=1)
            # the directory of #7 already holds report #1: refuse, never overwrite
            (s.site / "reports" / "survival" / "2026" / "01").rename(s.site / "reports" / "survival" / "2026" / "07")
            with self.assertRaises(SystemExit):
                sr.main(["--site", str(s.site), "--root", str(s.root), "build", "--run", str(s.run),
                         "--number", "7", "--date", "2026-09-21", "--no-png"])
            self.assertEqual(json.loads((s.site / "reports" / "survival" / "2026" / "07" / "report.json").read_text())["number"], 1)
        finally:
            s.close()

    def test_next_number_counts_directories(self) -> None:
        s = Scratch()
        try:
            self.assertEqual(sr.next_number(s.site), 1)
            s.build(number=1)
            self.assertEqual(sr.next_number(s.site), 2)
        finally:
            s.close()

    def test_from_existing_refuses_v1_data(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "survival-data.json"
            src.write_text(json.dumps({"generated_at": "2026-08-25T18:00:49Z", "rows": [
                {"repo": "a/b", "total_commits": 10, "verified": stat(6, 100, 50), "probable": stat(0, 0, 0), "by_agent": {}}]}))
            self.assertEqual(sr.from_existing(src, Path(tmp) / "out"), 3)
            self.assertFalse((Path(tmp) / "out" / "run.json").exists())

    def test_from_existing_accepts_v2_rows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "survival-data.json"
            src.write_text(json.dumps({"generated_at": "2026-09-01T00:00:00Z", "tool_version": "0.1.5",
                                       "rows": [{"repo": "a/b", "audited_at": "x", **audit(6, 100, 50)}]}))
            self.assertEqual(sr.from_existing(src, Path(tmp) / "out"), 0)
            self.assertTrue((Path(tmp) / "out" / "a__b.json").exists())
            self.assertEqual(json.loads((Path(tmp) / "out" / "run.json").read_text())["repos"], ["a/b"])


class DuplicateTests(unittest.TestCase):
    """One repository under two names is one measurement. GitHub keeps an old
    name working after a rename, so a list holding both names yields two
    identical audits; the report must count them once and say so."""

    def _build_with(self, extra: dict[str, str], listed: str) -> tuple[dict, str, Path]:
        s = Scratch()
        self.addCleanup(s.close)
        for name, source in extra.items():
            data = (s.run / f"{sr.repo_slug(source)}.json").read_bytes()
            (s.run / f"{sr.repo_slug(name)}.json").write_bytes(data)
        (s.root / ".github" / "survival-repos.txt").write_text(listed, encoding="utf-8")
        f = s.build()
        page = (s.site / "reports" / "survival" / "2026" / "01" / "index.html").read_text(encoding="utf-8")
        return f, page, s.site / "reports" / "survival" / "2026" / "01"

    def test_byte_identical_audits_count_once_under_the_listed_name(self) -> None:
        f, page, out = self._build_with({"old-org/last": "zeta/last"}, "# list\nzeta/last\nAlpha/first\n")
        repos = [r["repo"] for r in f["repositories"]]
        self.assertEqual(repos, ["Alpha/first", "mid/one", "zeta/last"])
        self.assertEqual(f["aggregate"]["repositories"], 3)
        self.assertEqual(f["aggregate"]["introduced"], 1000 + 500 + 800)
        self.assertEqual(f["excluded"]["duplicates"], [
            {"dropped": "old-org/last", "kept": "zeta/last", "reason": "byte-identical audit output",
             "audit_file": "repos/old-org__last.json", "kept_audit_file": "repos/zeta__last.json"}])
        self.assertIn("One repository, two names", page)
        self.assertIn("old-org/last", page)
        # both files stay next to the page, so the byte-identity is checkable
        self.assertEqual((out / "repos" / "old-org__last.json").read_bytes(), (out / "repos" / "zeta__last.json").read_bytes())
        self.assertNotIn("old-org/last", [r["repo"] for r in f["repositories"] + f["not_aggregated"]])

    def test_listed_name_wins_even_when_alphabetically_later(self) -> None:
        f, _, _ = self._build_with({"aaa/last": "zeta/last"}, "zeta/last\n")
        self.assertIn("zeta/last", [r["repo"] for r in f["repositories"]])
        self.assertEqual(f["excluded"]["duplicates"][0]["dropped"], "aaa/last")

    def test_same_head_counts_once_even_if_bytes_differ(self) -> None:
        s = Scratch()
        self.addCleanup(s.close)
        a = {**audit(20, 1000, 600), "repository": {"head": "a" * 40, "origin": "https://github.com/zeta/last"}}
        b = {**audit(20, 1000, 600), "repository": {"head": "a" * 40, "origin": "https://github.com/new/last"}}
        (s.run / "zeta__last.json").write_text(json.dumps(a), encoding="utf-8")
        (s.run / "new__last.json").write_text(json.dumps(b), encoding="utf-8")
        f = s.build()
        self.assertEqual(f["aggregate"]["repositories"], 3)
        self.assertEqual(f["excluded"]["duplicates"], [
            {"dropped": "zeta/last", "kept": "new/last", "reason": "same commit at HEAD",
             "audit_file": "repos/zeta__last.json", "kept_audit_file": "repos/new__last.json"}])

    def test_distinct_repositories_are_never_merged(self) -> None:
        s = Scratch()
        self.addCleanup(s.close)
        f = s.build()
        self.assertEqual(f["excluded"]["duplicates"], [])
        self.assertNotIn("One repository, two names",
                         (s.site / "reports" / "survival" / "2026" / "01" / "index.html").read_text(encoding="utf-8"))


class RevisionTests(unittest.TestCase):
    """A correction is a new revision of the same report: the old bytes stay,
    the page says what changed, the archive and feed carry the revision, and
    a plain rebuild of the same number is refused."""

    def setUp(self) -> None:
        self.s = Scratch()
        self.addCleanup(self.s.close)
        # r1 holds a row that should not have been there
        (self.s.run / "extra__one.json").write_text(json.dumps(audit(8, 200, 100, agent="devin")), encoding="utf-8")
        self.first = self.s.build(number=1)
        self.dir = self.s.site / "reports" / "survival" / "2026" / "01"
        self.assertEqual(self.first["aggregate"]["repositories"], 4)
        # the corrected run: that row removed
        (self.s.run / "extra__one.json").unlink()

    def revise(self, note: str = "extra/one was measured from a mirror of mid/one; it is removed.") -> dict:
        rc = sr.main(["--site", str(self.s.site), "--root", str(self.s.root), "revise", "--run", str(self.s.run),
                      "--number", "1", "--note", note, "--no-png"])
        self.assertEqual(rc, 0)
        return json.loads((self.dir / "report.json").read_text(encoding="utf-8"))

    def test_first_build_has_no_revision_field(self) -> None:
        self.assertNotIn("revision", self.first)
        self.assertNotIn("corrections", self.first)

    def test_same_number_again_is_refused_and_points_to_revise(self) -> None:
        with self.assertRaises(SystemExit) as cm:
            sr.main(["--site", str(self.s.site), "--root", str(self.s.root), "build", "--run", str(self.s.run),
                     "--number", "1", "--date", "2026-09-21", "--no-png"])
        self.assertIn("revise", str(cm.exception))

    def test_revision_freezes_the_old_bytes_and_states_the_change(self) -> None:
        before = (self.dir / "report.json").read_bytes()
        before_md = (self.dir / "report.md").read_bytes()
        f = self.revise()
        self.assertEqual((self.dir / "report.r1.json").read_bytes(), before)
        self.assertEqual((self.dir / "report.r1.md").read_bytes(), before_md)
        self.assertEqual(f["revision"], 2)
        self.assertEqual(f["date"], "2026-09-21")  # the measurement date does not move
        self.assertEqual(f["generated_at"], self.first["generated_at"])
        self.assertTrue(f["revised_at"] > f["generated_at"])
        self.assertIsNone(f["doi"])  # a DOI belongs to bytes; the deposit mints one for this revision
        c = f["corrections"]
        self.assertEqual(len(c), 1)
        self.assertEqual(c[0]["revision"], 2)
        self.assertEqual(c[0]["previous"]["file"], "report.r1.json")
        self.assertEqual(c[0]["previous"]["aggregate"]["repositories"], 4)
        self.assertEqual(f["aggregate"]["repositories"], 3)
        self.assertNotIn("extra/one", [r["repo"] for r in f["repositories"]])
        page = (self.dir / "index.html").read_text(encoding="utf-8")
        self.assertIn("revision 2", page)
        self.assertIn('href="report.r1.json"', page)
        self.assertIn("Revision 2 (", page)
        self.assertIn("Revision 1 counted 4 repositories", page)
        md = (self.dir / "report.md").read_text(encoding="utf-8")
        self.assertIn("## Corrections", md)
        self.assertIn("revision 2", sr.cite(f))
        archive = (self.s.site / "reports" / "survival" / "index.html").read_text(encoding="utf-8")
        self.assertIn("rev. 2", archive)
        feed = (self.s.site / "reports" / "survival" / "feed.xml").read_text(encoding="utf-8")
        self.assertIn("Revision 2 (", feed)
        self.assertIn(f"<updated>{f['revised_at']}</updated>", feed)
        latest = json.loads((self.s.site / "reports" / "survival" / "latest.json").read_text(encoding="utf-8"))
        self.assertEqual(latest["revision"], 2)

    def test_second_revision_keeps_both_predecessors(self) -> None:
        self.revise()
        r2 = (self.dir / "report.json").read_bytes()
        f = self.revise("second correction, for the test.")
        self.assertEqual(f["revision"], 3)
        self.assertEqual((self.dir / "report.r2.json").read_bytes(), r2)
        self.assertTrue((self.dir / "report.r1.json").exists())
        self.assertEqual([c["revision"] for c in f["corrections"]], [2, 3])

    def test_revise_refuses_an_empty_note_and_an_unknown_report(self) -> None:
        with self.assertRaises(SystemExit):
            sr.revise(self.s.run, 1, "   ", self.s.site, self.s.root, png=False)
        with self.assertRaises(SystemExit):
            sr.revise(self.s.run, 9, "note", self.s.site, self.s.root, png=False)
        self.assertFalse((self.dir / "report.r1.json").exists())

    def test_rebuild_keeps_the_revision(self) -> None:
        f = self.revise()
        sr.rebuild(self.s.site)
        self.assertEqual(json.loads((self.dir / "report.json").read_text(encoding="utf-8")), f)
        self.assertIn("revision 2", (self.dir / "index.html").read_text(encoding="utf-8"))


class ScaleTests(unittest.TestCase):
    """One hundred repositories, as the discovered list yields: every surface
    carries all of them, the copy says "100 repositories", nothing assumes a
    handful of rows."""

    N = 100

    @classmethod
    def setUpClass(cls) -> None:
        cls.s = Scratch()
        for p in cls.s.run.glob("*__*.json"):
            p.unlink()
        cls.repos = []
        for k in range(cls.N):
            owner = f"org{k % 7}" if k % 3 else f"Org{k % 5}"  # mixed case, several owners
            repo = f"{owner}/repo-{k:03d}"
            cls.repos.append(repo)
            intro = 1_000 + 137 * k
            a = audit(6 + k % 40, intro, intro - (intro * (k % 10)) // 10 - (3 if k % 10 else 0), agent=["claude-code", "aider", "cursor", "openai-codex"][k % 4])
            (cls.s.run / f"{sr.repo_slug(repo)}.json").write_text(json.dumps(a), encoding="utf-8")
        (cls.s.run / "run.json").write_text(json.dumps({
            "generated_at": "2026-10-05T05:17:00Z", "tool": "causari", "tool_version": "0.2.1", "method": "v2",
            "command": "re audit <owner/repo> --json", "repos": cls.repos, "failed": [f"gone/repo-{k}" for k in range(12)], "opted_out": [],
        }), encoding="utf-8")
        (cls.s.root / ".github" / "survival-discovery.json").write_text(json.dumps({
            "schema": "causari.survival_discovery.v1", "discovered_at": "2026-10-01T04:23:00Z",
            "selection": {"floor": 5, "limit": 100},
            "repositories": [{"repo": r, "seed": k < 30} for k, r in enumerate(cls.repos)],
        }), encoding="utf-8")
        cls.f = cls.s.build(number=2, date="2026-10-05")
        cls.dir = cls.s.site / "reports" / "survival" / "2026" / "02"
        cls.page = (cls.dir / "index.html").read_text(encoding="utf-8")
        cls.md = (cls.dir / "report.md").read_text(encoding="utf-8")

    @classmethod
    def tearDownClass(cls) -> None:
        cls.s.close()

    def test_all_hundred_aggregated_alphabetically(self) -> None:
        repos = [r["repo"] for r in self.f["repositories"]]
        self.assertEqual(len(repos), self.N)
        self.assertEqual(repos, sorted(repos, key=str.lower))
        self.assertEqual(self.f["aggregate"]["repositories"], self.N)
        self.assertEqual(self.f["aggregate"]["introduced"], sum(1_000 + 137 * k for k in range(self.N)))
        self.assertEqual(self.page.count('<tr><td><a href="/r/'), self.N)
        self.assertEqual(sum(1 for l in self.md.splitlines() if l.startswith("| ") and "re audit " in l), self.N)
        self.assertEqual(len(self.f["excluded"]["failed"]), 12)

    def test_copy_counts_one_hundred(self) -> None:
        self.assertIn("in 100 open-source repositories", sr.headline(self.f))
        self.assertIn("in 100 repositories are still at HEAD", (self.dir / "card.svg").read_text(encoding="utf-8"))
        self.assertIn("of the 100 aggregated repositories", self.f["aggregate"]["interval_method"]["note"])
        self.assertIn(">100</span><span class=\"l\">repositories aggregated", self.page)
        iv = self.f["aggregate"]["survival_rate_interval_95"]
        self.assertLess(iv["low"], self.f["aggregate"]["survival_rate"])
        self.assertGreater(iv["high"], self.f["aggregate"]["survival_rate"])
        ET.fromstring((self.dir / "card.svg").read_text(encoding="utf-8"))

    def test_selection_sentence_from_discovery(self) -> None:
        sel = self.f["method"]["selection"]
        self.assertIn("30 hand-picked and 70 found by GitHub commit search", sel)
        self.assertIn("at least 5 commits", sel)
        self.assertIn("discovered 2026-10-01", sel)
        self.assertIn(sel, self.page)
        self.assertIn(sel, self.md)
        self.assertNotIn("added by pull request", self.page)
        for word in FORBIDDEN:
            self.assertNotIn(word, sel)

    def test_sizes_stay_reasonable(self) -> None:
        self.assertLess((self.dir / "report.json").stat().st_size, 400_000)
        self.assertLess((self.dir / "index.html").stat().st_size, 400_000)
        self.assertEqual(len(list((self.dir / "repos").glob("*.json"))), self.N)
        self.assertEqual(len(self.f["by_agent"]), 4)


class ShardTests(unittest.TestCase):
    def frag(self, k: int, repos: list[str], failed: list[str] = (), opted: list[str] = (), version: str = "0.2.1", at: str = "T05:20:00Z") -> dict:
        return {"generated_at": f"2026-10-05{at}", "tool": "causari", "tool_version": version, "method": "v2",
                "command": "re audit <owner/repo> --json", "repos": repos, "failed": list(failed), "opted_out": list(opted), "shard": k}

    def test_merge_unions_fragments_and_records_unreported_repositories(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "root"
            run = Path(tmp) / "run"
            (root / ".github").mkdir(parents=True)
            run.mkdir()
            (root / ".github" / "survival-repos.txt").write_text(
                "# list\na/one\nb/two\nc/three\nd/four\nOpt/Out\ne/five\n", encoding="utf-8")
            (root / ".github" / "survival-optout.txt").write_text("opt/out\n", encoding="utf-8")
            # shard 0: a/one audited, d/four failed; shard 1: b/two audited, c/three has no audit and no failure
            # record (killed mid-run); shard 2 (e/five, Opt/Out) never uploaded at all
            (run / "a__one.json").write_text(json.dumps(audit(6, 100, 50)), encoding="utf-8")
            (run / "b__two.json").write_text(json.dumps(audit(7, 120, 60)), encoding="utf-8")  # distinct bytes: a different repository
            (run / "run-shard-0.json").write_text(json.dumps(self.frag(0, ["a/one", "d/four"], failed=["d/four"], at="T05:17:00Z")), encoding="utf-8")
            (run / "run-shard-1.json").write_text(json.dumps(self.frag(1, ["b/two", "c/three"], opted=["Opt/Out"])), encoding="utf-8")
            import contextlib
            import io
            err = io.StringIO()
            with contextlib.redirect_stderr(err), contextlib.redirect_stdout(io.StringIO()):
                merged = sr.merge_shards(run, root)
            on_disk = json.loads((run / "run.json").read_text(encoding="utf-8"))
            self.assertEqual(on_disk, merged)
            self.assertEqual(set(merged) >= {"generated_at", "tool", "tool_version", "method", "command", "repos", "failed", "opted_out"}, True)
            self.assertEqual(merged["generated_at"], "2026-10-05T05:17:00Z")  # earliest shard
            self.assertEqual(merged["tool_version"], "0.2.1")
            self.assertEqual(merged["repos"], ["a/one", "b/two", "c/three", "d/four", "e/five"])
            self.assertEqual(merged["failed"], ["c/three", "d/four", "e/five"])
            self.assertEqual(merged["opted_out"], ["Opt/Out"])
            self.assertIn("e/five", err.getvalue())
            self.assertIn("c/three", err.getvalue())
            # and the generator builds from the merged run: two rows, three failures, one opt-out
            site = Path(tmp) / "site"
            site.mkdir()
            rc = sr.main(["--site", str(site), "--root", str(root), "build", "--run", str(run), "--number", "1", "--date", "2026-10-05", "--no-png"])
            self.assertEqual(rc, 0)
            f = json.loads((site / "reports" / "survival" / "2026" / "01" / "report.json").read_text(encoding="utf-8"))
            self.assertEqual([r["repo"] for r in f["repositories"]], ["a/one", "b/two"])
            self.assertEqual(f["excluded"]["failed"], ["c/three", "d/four", "e/five"])
            self.assertEqual(f["excluded"]["opted_out"], 1)

    def test_merge_refuses_without_fragments(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(SystemExit):
                sr.merge_shards(Path(tmp), Path(tmp))

    def test_shard_split_is_index_mod_n(self) -> None:
        # the same rule the workflow applies in bash: shard k takes indices i with i % N == k
        repos = [f"o/r{i}" for i in range(23)]
        shards = [[r for i, r in enumerate(repos) if i % 10 == k] for k in range(10)]
        self.assertEqual(sorted(sum(shards, [])), sorted(repos))
        self.assertEqual(shards[0], ["o/r0", "o/r10", "o/r20"])
        self.assertEqual(shards[3], ["o/r3", "o/r13"])


class ZenodoTests(unittest.TestCase):
    def test_dry_run_payload_without_token(self) -> None:
        s = Scratch()
        try:
            s.build()
            d = s.site / "reports" / "survival" / "2026" / "01"
            import contextlib
            import io
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                rc = zd.main(["--site", str(s.site), "--dry-run", str(d)])
            self.assertEqual(rc, 0)
            payload = json.loads(out.getvalue())
            self.assertTrue(payload["dry_run"])
            meta = payload["metadata"]
            self.assertEqual(meta["creators"], [{"name": "Crovia Trust", "affiliation": "Crovia Trust"}])
            self.assertEqual(meta["license"], "cc-by-4.0")
            self.assertEqual(meta["upload_type"], "publication")
            self.assertIn("Survival Report #1", meta["title"])
            self.assertEqual(payload["version"], "#1")
            paths = {f["path"] for f in payload["files"]}
            self.assertTrue({"report.json", "report.md", "MANIFEST.json"} <= paths)
            self.assertIn("repos/Alpha__first.json", paths)
            # no token, not a dry run: refused, nothing written
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(zd.main(["--site", str(s.site), str(d)]), 2)
        finally:
            s.close()

    def test_write_back_puts_doi_on_every_surface(self) -> None:
        s = Scratch()
        try:
            f = s.build()
            d = s.site / "reports" / "survival" / "2026" / "01"
            rec = {"doi": "10.5281/zenodo.99999", "concept_doi": "10.5281/zenodo.99998", "html": "https://zenodo.org/records/99999",
                   "id": 99999, "version": "#1", "sandbox": False, "published_at": "2026-09-21T06:00:00Z"}
            zd.write_back(d, f, rec, s.site)
            self.assertEqual(json.loads((d / "report.json").read_text())["doi"], "10.5281/zenodo.99999")
            self.assertIn("https://doi.org/10.5281/zenodo.99999", (d / "index.html").read_text())
            self.assertIn("10.5281/zenodo.99999", (d / "report.md").read_text())
            self.assertIn("10.5281/zenodo.99999", (s.site / "reports" / "survival" / "index.html").read_text())
            self.assertEqual(json.loads((s.site / "reports" / "survival" / "latest.json").read_text())["doi"], "10.5281/zenodo.99999")
            self.assertIn("10.5281/zenodo.99999", (s.site / "reports" / "survival" / "feed.xml").read_text())
        finally:
            s.close()

    def test_content_hash_ignores_manifest(self) -> None:
        files = {"report.json": b"{}", "MANIFEST.json": b"a"}
        self.assertEqual(zd.content_hash(files), zd.content_hash({"report.json": b"{}", "MANIFEST.json": b"b"}))
        self.assertNotEqual(zd.content_hash(files), zd.content_hash({"report.json": b"{ }"}))


PNX_005 = json.loads((ROOT / "tests" / "vectors" / "pnx" / "conformance" / "pnx_005_reach.json").read_text(encoding="utf-8"))


def verify_json(sheet: dict, verdict: str = "within-policy", ok: bool = True, outside: list | None = None) -> dict:
    """What `tacet-pnx verify SHEET --policy POLICY --json` writes for a sheet alone."""
    return {"ok": ok, "verdict": "sheet-only" if ok else "?", "assets": {}, "errors": [] if ok else ["witness signature invalid"],
            "warnings": [], "sealed": False, "sheet_only": True,
            "reach": {"verdict": verdict if ok else "?", "outside": outside or [], "reached": {}}}


def _archive_facts(number: int, names: list[str], method: str, rate: float = 0.5) -> dict:
    """The fields render_index reads. Names are the aggregated repositories."""
    n = len(names)
    return {
        "number": number,
        "id": f"2026/{number:02d}",
        "date": "2026-09-28",
        "url": f"https://causari.dev/reports/survival/2026/{number:02d}/",
        "generated_at": "2026-09-28T05:17:00Z",
        "aggregate": {
            "repositories": n,
            "ai_tagged_commits": 10,
            "introduced": 100,
            "surviving": 50,
            "survival_rate": rate,
            "survival_rate_interval_95": None,
        },
        "method": {"version": method},
        "repositories": [{"repo": name} for name in names],
    }


class ArchiveRateTests(unittest.TestCase):
    """The archive table puts one line-weighted rate per report in one column.
    That column is a series only when the aggregated repositories and the
    method are the same. Report #4 will not have report #3's 43 repositories."""

    def test_a_changed_sample_is_named_and_is_not_a_series(self) -> None:
        older = ["o/r" + str(i) for i in range(43)]
        newer = older + ["o/extra" + str(i) for i in range(18)]
        html = sr.render_index([
            _archive_facts(4, newer, "v3", 0.40),
            _archive_facts(3, older, "v3", 0.614),
        ])
        text = visible_text(html)
        self.assertIn("The line-weighted column is not a series.", text)
        self.assertIn("#4 aggregated 61 repositories under method v3", text)
        self.assertIn("#3 aggregated 43 repositories under method v3", text)
        self.assertIn("A different repository count means the rates do not measure the same sample over time.", text)
        self.assertIn("A repository followed across reports is on its page.", text)
        self.assertIn('id="archive-rates"', html)
        self.assertNotIn("up from", text.lower())
        self.assertNotIn("down from", text.lower())

    def test_a_method_change_is_not_a_series_even_at_the_same_count(self) -> None:
        names = ["o/r" + str(i) for i in range(43)]
        text = visible_text(sr.render_index([
            _archive_facts(3, names, "v3"),
            _archive_facts(2, names, "v2"),
        ]))
        self.assertIn("not a series", text)
        self.assertIn("A different method means the rates do not measure the same thing over time.", text)

    def test_the_same_repositories_under_the_same_method_need_no_warning(self) -> None:
        names = ["o/r" + str(i) for i in range(43)]
        text = visible_text(sr.render_index([
            _archive_facts(4, names, "v3", 0.55),
            _archive_facts(3, names, "v3", 0.614),
        ]))
        self.assertNotIn("not a series", text)
        self.assertNotIn('id="archive-rates"', sr.render_index([
            _archive_facts(4, names, "v3"),
            _archive_facts(3, names, "v3"),
        ]))

    def test_the_lede_names_metadata_matched_and_points_at_the_readme(self) -> None:
        html = sr.render_index([_archive_facts(1, ["o/only"], "v3")])
        for text in (html, (ROOT / "site/reports/survival/index.html").read_text(encoding="utf-8")):
            self.assertIn("AI-tagged means that metadata matched", text)
            self.assertIn("does not prove a model wrote the line", text)
            self.assertIn("https://github.com/croviatrust/causari#readme", text)
            self.assertNotIn("Verified AI", text)

    def test_one_report_has_nothing_to_line_up(self) -> None:
        html = sr.render_index([_archive_facts(1, ["o/only"], "v2")])
        self.assertNotIn("not a series", visible_text(html))

    def test_equal_counts_of_different_repositories_are_not_the_same_sample(self) -> None:
        text = visible_text(sr.render_index([
            _archive_facts(2, ["o/a", "o/b"], "v3"),
            _archive_facts(1, ["o/a", "o/c"], "v3"),
        ]))
        self.assertIn("not the same set", text)


class ReachTests(unittest.TestCase):
    """The run's reach receipt: the signed PNX run sheet of the egress witness
    the shards ran behind, published next to the report once verified."""

    def scratch(self, sheet: dict | None, verify: dict | None, policy: bool = True) -> Scratch:
        s = Scratch()
        if sheet is not None:
            (s.run / "reach.sheet.json").write_text(json.dumps(sheet), encoding="utf-8")
        if verify is not None:
            (s.run / "reach.verify.json").write_text(json.dumps(verify), encoding="utf-8")
        if policy:
            (s.root / ".github" / "egress-policy.json").write_text(json.dumps(PNX_005["policy"]["document"]), encoding="utf-8")
        return s

    def test_a_run_without_a_sheet_has_no_receipt(self) -> None:
        s = Scratch()
        try:
            f = s.build()
            self.assertIsNone(f["reach"])
            d = s.site / "reports" / "survival" / "2026" / "01"
            self.assertNotIn("Where this measurement connected", (d / "index.html").read_text(encoding="utf-8"))
            self.assertNotIn("Where this measurement connected", (d / "report.md").read_text(encoding="utf-8"))
            self.assertFalse((d / "reach.sheet.json").exists())
        finally:
            s.close()

    def test_a_verified_sheet_is_published_with_its_destinations_and_policy(self) -> None:
        sheet = PNX_005["valid"]["enforce"]["sheet"]
        s = self.scratch(sheet, verify_json(sheet))
        try:
            f = s.build()
            r = f["reach"]
            self.assertEqual(r["profile"], "crovia.pnx.v1")
            self.assertEqual(r["record"], "crovia.pnx.reach.v1")
            self.assertEqual(r["run_id"], sheet["run_id"])
            self.assertEqual(r["witness"], {"id": sheet["witness"]["id"], "key_hex": sheet["witness"]["pubkey"]["key_hex"]})
            self.assertEqual(r["verdict"], "within-policy")
            self.assertEqual(r["summary"], sheet["reach"]["summary"])
            self.assertEqual([d["host"] for d in r["destinations"]], [d["host"] for d in sheet["reach"]["destinations"]])
            self.assertEqual(r["policy"]["hash"], sheet["reach"]["policy"]["hash"])
            self.assertEqual(r["policy"]["file"], "egress-policy.json")
            self.assertEqual(r["policy"]["mode"], "enforce")
            self.assertNotIn("_files", r)
            self.assertIn("tacet-pnx verify reach.sheet.json --policy egress-policy.json", r["verify"])
            d = s.site / "reports" / "survival" / "2026" / "01"
            # the sheet and the policy travel with the report, byte for byte
            self.assertEqual(json.loads((d / "reach.sheet.json").read_text(encoding="utf-8")), sheet)
            self.assertEqual(json.loads((d / "egress-policy.json").read_text(encoding="utf-8")), PNX_005["policy"]["document"])
            html = (d / "index.html").read_text(encoding="utf-8")
            text = visible_text(html)
            self.assertIn("Where this measurement connected", text)
            self.assertIn("Every destination the witness saw is one the policy allows.", text)
            self.assertIn("pastebin.com:443", text)
            self.assertIn("blocked", text)
            self.assertIn('href="reach.sheet.json"', html)
            self.assertIn('href="egress-policy.json"', html)
            self.assertIn("re pnx verify reach.sheet.json --policy egress-policy.json", text)
            self.assertIn("Not covered:", text)
            for word in FORBIDDEN:
                self.assertNotIn(word, text)
            md = (d / "report.md").read_text(encoding="utf-8")
            self.assertIn("## Where this measurement connected", md)
            self.assertIn("| pastebin.com:443 | blocked | 1 | 0 | 0 |", md)
            self.assertIn(sheet["reach"]["policy"]["hash"], md)
            # the receipt is part of the deposit
            self.assertTrue({"reach.sheet.json", "egress-policy.json"} <= set(zd.gather(d)))
        finally:
            s.close()

    def test_a_sheet_without_a_verification_is_not_published(self) -> None:
        sheet = PNX_005["valid"]["enforce"]["sheet"]
        s = self.scratch(sheet, None)
        try:
            with self.assertRaises(SystemExit) as cm:
                s.build()
            self.assertIn("reach.verify.json missing", str(cm.exception))
            self.assertFalse((s.site / "reports" / "survival" / "2026" / "01").exists())
        finally:
            s.close()

    def test_a_sheet_that_failed_verification_is_not_published(self) -> None:
        sheet = PNX_005["valid"]["enforce"]["sheet"]
        s = self.scratch(sheet, verify_json(sheet, ok=False))
        try:
            with self.assertRaises(SystemExit) as cm:
                s.build()
            self.assertIn("did not verify", str(cm.exception))
            self.assertIn("witness signature invalid", str(cm.exception))
        finally:
            s.close()

    def test_an_object_that_is_not_a_sheet_is_refused(self) -> None:
        s = self.scratch({"profile": "crovia.pnx.v1", "hello": 1}, verify_json({}))
        try:
            with self.assertRaises(SystemExit) as cm:
                s.build()
            self.assertIn("not a crovia.pnx.v1 run sheet with a reach record", str(cm.exception))
        finally:
            s.close()

    def test_outside_policy_is_published_and_said_plainly(self) -> None:
        # observe mode: the witness relayed everything; the verifier found pastebin.com outside the policy.
        sheet = PNX_005["valid"]["observe"]["sheet"]
        s = self.scratch(sheet, verify_json(sheet, verdict="outside-policy", outside=["pastebin.com:443"]))
        try:
            f = s.build()
            self.assertEqual(f["reach"]["verdict"], "outside-policy")
            self.assertEqual(f["reach"]["outside"], ["pastebin.com:443"])
            d = s.site / "reports" / "survival" / "2026" / "01"
            text = visible_text((d / "index.html").read_text(encoding="utf-8"))
            self.assertIn("A destination outside the policy was reached: pastebin.com:443.", text)
            self.assertIn("observe mode", text)
        finally:
            s.close()

    def test_without_a_policy_the_destinations_are_stated_not_judged(self) -> None:
        sheet = PNX_005["valid"]["none"]["sheet"]
        s = self.scratch(sheet, verify_json(sheet, verdict="unpoliced"), policy=False)
        try:
            f = s.build()
            self.assertEqual(f["reach"]["verdict"], "unpoliced")
            self.assertIsNone(f["reach"]["policy"]["file"])
            self.assertIsNone(f["reach"]["policy"]["hash"])
            d = s.site / "reports" / "survival" / "2026" / "01"
            self.assertFalse((d / "egress-policy.json").exists())
            text = visible_text((d / "index.html").read_text(encoding="utf-8"))
            self.assertIn("with no policy in force (destinations stated, not judged)", text)
            self.assertIn("No policy was in force; the destinations are stated, not judged.", text)
        finally:
            s.close()

    def test_a_missing_shard_log_is_said_and_the_sheet_does_not_cover_the_job(self) -> None:
        sheet = PNX_005["valid"]["enforce"]["sheet"]
        s = self.scratch(sheet, verify_json(sheet))
        try:
            run = json.loads((s.run / "run.json").read_text(encoding="utf-8"))
            run["shards"] = ["run-shard-0.json", "run-shard-1.json", "run-shard-2.json"]
            (s.run / "run.json").write_text(json.dumps(run), encoding="utf-8")
            (s.run / "reach-shard-0.jsonl").write_text("{}\n", encoding="utf-8")
            (s.run / "reach-shard-1.jsonl").write_text("{}\n", encoding="utf-8")
            f = s.build()
            self.assertEqual(f["reach"]["logs"], {"uploaded": 2, "shards": 3})
            self.assertNotIn("every audit shard", f["reach"]["covers"])
            self.assertIn("uploaded and concatenated", f["reach"]["covers"])
            self.assertIn("uploaded no log", f["reach"]["not_covered"])
            text = visible_text((s.site / "reports" / "survival" / "2026" / "01" / "index.html").read_text(encoding="utf-8"))
            self.assertIn("2 of 3 uploaded shard logs", text)
            self.assertIn("a shard that uploaded no log is absent from this sheet", text)
            self.assertIn("Every destination the witness saw is one the policy allows.", text)
            policy = json.loads((ROOT / ".github" / "egress-policy.json").read_text(encoding="utf-8"))
            self.assertEqual(set(policy), {"version", "allow"})
            self.assertEqual(policy["allow"], ["github.com:443"])
        finally:
            s.close()

    def test_rerender_keeps_the_receipt(self) -> None:
        sheet = PNX_005["valid"]["enforce"]["sheet"]
        s = self.scratch(sheet, verify_json(sheet))
        try:
            s.build()
            d = s.site / "reports" / "survival" / "2026" / "01"
            (d / "index.html").unlink()
            self.assertEqual(sr.main(["--site", str(s.site), "--root", str(s.root), "rerender", str(d)]), 0)
            self.assertIn("Where this measurement connected", (d / "index.html").read_text(encoding="utf-8"))
        finally:
            s.close()


if __name__ == "__main__":
    unittest.main()
