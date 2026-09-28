#!/usr/bin/env python3
"""Tests for scripts/survival_discover.py: no network, a fake GitHub.

    python3 scripts/tests/test_survival_discover.py
    python3 -m pytest scripts/tests -q
"""

from __future__ import annotations

import contextlib
import io
import json
import re
import sys
import tempfile
import time
import unittest
import urllib.parse
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPTS = HERE.parent
sys.path.insert(0, str(SCRIPTS))

import survival_discover as sd  # noqa: E402

HEADER = (
    "# Survival Report — measured repositories\n"
    "# One repository per line (owner/repo). Lines starting with # are ignored.\n"
    "Aider-AI/aider\n"
    "croviatrust/causari\n"
)

CLAUDE = "co-authored-by:claude"
AIDER = "author:aider"
SIGNALS = {CLAUDE: '"Co-Authored-By: Claude"', AIDER: "author-name:aider"}
NOW = "2026-10-01T04:23:00Z"


def commit(repo: str, sha: str, fork: bool = False) -> dict:
    return {"sha": sha, "repository": {"full_name": repo, "fork": fork}}


def commits(repo: str, n: int, prefix: str = "", fork: bool = False) -> list[dict]:
    return [commit(repo, f"{prefix}{repo}-{i:03d}", fork=fork) for i in range(n)]


class FakeGitHub:
    """Answers the three requests the script makes.

    sample:  /search/commits?q=<signal query> is:public merge:false author-date:<=D
             from `samples` (signal query → items), paged
    count:   /search/commits?q=repo:<name> <signal query> is:public merge:false
             total_count from `counts[(name.lower(), signal query)]`, else the
             number of sampled items of that repository for that query
    details: /repos/<name> from `details` (404 when absent)
    Records every URL and every sleep; can rate-limit the first N requests."""

    def __init__(self, samples: dict[str, list[dict]], details: dict[str, dict] | None = None,
                 counts: dict[tuple[str, str], int] | None = None) -> None:
        self.samples = samples
        self.details = details if details is not None else {}
        self.counts = counts or {}
        self.default_details = {"stars": 1, "archived": False}
        self.urls: list[str] = []
        self.sleeps: list[float] = []
        self.rate_limit_first = 0

    def sleep(self, s: float) -> None:
        self.sleeps.append(s)

    def fetch(self, url: str) -> tuple[int, dict[str, str], bytes]:
        self.urls.append(url)
        if self.rate_limit_first > 0:
            self.rate_limit_first -= 1
            return 403, {"retry-after": "7", "x-ratelimit-remaining": "0"}, b'{"message":"rate limited"}'
        parsed = urllib.parse.urlparse(url)
        if parsed.path == "/search/commits":
            qs = urllib.parse.parse_qs(parsed.query)
            q = qs["q"][0]
            if q.startswith("repo:"):
                name, _, rest = q[len("repo:"):].partition(" ")
                query = rest.replace(" " + sd.QUALIFIERS, "")
                total = self.counts.get((name.lower(), query))
                if total is None:
                    total = sum(1 for i in self.samples.get(query, []) if i["repository"]["full_name"].lower() == name.lower())
                return 200, {}, json.dumps({"total_count": total, "incomplete_results": False, "items": []}).encode()
            q = re.sub(r" author-date:<=\d{4}-\d{2}-\d{2}$", "", q).replace(" " + sd.QUALIFIERS, "")
            page = int(qs.get("page", ["1"])[0])
            per_page = int(qs.get("per_page", ["100"])[0])
            if page > 10:
                return 422, {}, b'{"message":"Only the first 1000 search results are available"}'
            items = self.samples.get(q, [])
            batch = items[(page - 1) * per_page: page * per_page]
            return 200, {}, json.dumps({"total_count": len(items), "incomplete_results": False, "items": batch}).encode()
        if parsed.path.startswith("/repos/"):
            name = parsed.path[len("/repos/"):]
            v = next((v for k, v in self.details.items() if k.lower() == name.lower()), None)
            if v is None:
                if self.default_details is None:
                    return 404, {}, b'{"message":"Not Found"}'
                v = self.default_details
            return 200, {}, json.dumps({"full_name": v.get("full_name", name), "stargazers_count": v.get("stars", 0),
                                        "archived": v.get("archived", False), "fork": v.get("fork", False),
                                        "private": v.get("private", False)}).encode()
        return 500, {}, b""


class Scratch:
    def __init__(self, list_text: str = HEADER, optout: str = "# none\n") -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / ".github").mkdir()
        (self.root / ".github" / "survival-repos.txt").write_text(list_text, encoding="utf-8")
        (self.root / ".github" / "survival-optout.txt").write_text(optout, encoding="utf-8")

    def list_text(self) -> str:
        return (self.root / ".github" / "survival-repos.txt").read_text(encoding="utf-8")

    def json(self) -> dict:
        return json.loads((self.root / ".github" / "survival-discovery.json").read_text(encoding="utf-8"))

    def close(self) -> None:
        self.tmp.cleanup()


def run(gh: FakeGitHub, s: Scratch, **kw) -> dict:
    client = sd.Client(gh.fetch, sleep=gh.sleep, log=lambda _s: None)
    kw.setdefault("signals", SIGNALS)
    kw.setdefault("now", NOW)
    kw.setdefault("min_stars", 0)  # the star floor has its own test; the others exercise counting
    result = sd.discover(client, s.root, **kw)
    sd.write_outputs(result, s.root)
    return s.json()


def discovered(j: dict) -> list[str]:
    return [r["repo"] for r in j["repositories"] if not r["seed"]]


class SelectionTests(unittest.TestCase):
    def test_sample_count_floor_and_dedupe(self) -> None:
        samples = {
            SIGNALS[CLAUDE]: commits("Big/Repo", 12) + commits("small/repo", 4) + commits("Mid/one", 6) + commits("mid/two", 6) + commits("once/only", 1),
            # the same SHAs again under the author signal are not double counted in the sample
            SIGNALS[AIDER]: commits("Big/Repo", 12) + commits("big/repo", 3, prefix="x"),
        }
        details = {"Big/Repo": {"stars": 10}, "Mid/one": {"stars": 5}, "mid/two": {"stars": 50}}
        s = Scratch()
        try:
            gh = FakeGitHub(samples, details)
            j = run(gh, s)
            by = {r["repo"]: r for r in j["repositories"]}
            self.assertEqual(by["Big/Repo"]["sampled"], 15)  # distinct SHAs in the sample
            # repository-wide: the sum over signals, a commit carrying two signals counts twice (stated in the rule)
            self.assertEqual(by["Big/Repo"]["by_signal"], {AIDER: 15, CLAUDE: 12})
            self.assertEqual(by["Big/Repo"]["commits"], 27)
            self.assertEqual(by["Big/Repo"]["counts"], "repository-wide")
            self.assertNotIn("big/repo", by)  # case-insensitive dedupe
            self.assertNotIn("small/repo", by)  # 4 < floor
            self.assertNotIn("once/only", by)  # below candidate_min: never counted
            self.assertEqual(j["dropped"]["below_floor"], 1)
            self.assertEqual(j["dropped"]["not_candidates"], 1)
            self.assertEqual(j["candidates"], 4)
            self.assertEqual([r["repo"] for r in j["repositories"] if r["seed"]], ["Aider-AI/aider", "croviatrust/causari"])
            # the list: header verbatim, seeds first, marker, discovered alphabetical
            text = s.list_text()
            self.assertTrue(text.startswith(HEADER))
            self.assertIn("# discovered 2026-10-01 by scripts/survival_discover.py: 3 repositories, the most-starred public "
                          "repositories with ≥5 AI-attributed commits found via GitHub commit search and ≥0 stars\n", text)
            tail = text.split("≥0 stars\n", 1)[1].splitlines()
            self.assertEqual([l for l in tail if not l.startswith("#")], ["Big/Repo", "Mid/one", "mid/two"])
            # rows in the JSON are alphabetical too, with stars and discovered_at
            self.assertEqual([r["repo"] for r in j["repositories"]], sorted((r["repo"] for r in j["repositories"]), key=str.lower))
            self.assertEqual(by["mid/two"]["stars"], 50)
            self.assertEqual(by["mid/two"]["discovered_at"], NOW)
            # one count request per (candidate, signal seen); one details request per kept
            # repository and one per seed (renames are resolved for both)
            count_urls = [u for u in gh.urls if "q=repo%3A" in u]
            self.assertEqual(len(count_urls), 5)  # Big/Repo ×2, small ×1, Mid/one ×1, mid/two ×1
            self.assertEqual(sum(1 for u in gh.urls if "/repos/" in u), 3 + 2)
            self.assertEqual(j["selection"]["qualifiers"], "is:public merge:false author-date:<=2026-10-01")
        finally:
            s.close()

    def test_repository_wide_count_ranks_not_the_sample(self) -> None:
        # steady/flow appears twice in the sample but has 400 matching commits
        # in the repository; bursty/one has 30 sampled and 30 in total.
        samples = {SIGNALS[CLAUDE]: commits("bursty/one", 30) + commits("steady/flow", 2) + commits("tiny/four", 2)}
        counts = {("steady/flow", SIGNALS[CLAUDE]): 400, ("tiny/four", SIGNALS[CLAUDE]): 4}
        s = Scratch()
        try:
            j = run(FakeGitHub(samples, counts=counts), s, limit=3)  # room for one
            self.assertEqual(discovered(j), ["steady/flow"])
            self.assertEqual(j["dropped"]["over_limit"], ["bursty/one"])
            self.assertEqual(j["dropped"]["below_floor"], 1)  # tiny/four: 4 repository-wide
        finally:
            s.close()

    def test_limit_uses_stars_then_commits_and_keeps_seeds(self) -> None:
        samples = {SIGNALS[CLAUDE]: commits("a/eight", 8) + commits("b/six-popular", 6) + commits("c/six-quiet", 6) + commits("d/five", 5)}
        details = {"a/eight": {"stars": 1}, "b/six-popular": {"stars": 500}, "c/six-quiet": {"stars": 2}, "d/five": {"stars": 9000}}
        s = Scratch()
        try:
            j = run(FakeGitHub(samples, details), s, limit=4)  # 2 seeds + 2 discovered
            self.assertEqual(sorted(discovered(j)), ["b/six-popular", "d/five"])
            self.assertEqual(j["dropped"]["over_limit"], ["c/six-quiet", "a/eight"])  # 2 stars, then 1
            self.assertEqual(j["seeds"], ["Aider-AI/aider", "croviatrust/causari"])
            self.assertIn("Aider-AI/aider\n", s.list_text())
        finally:
            s.close()

    def test_forks_archived_optout_and_seed_duplicates_skipped(self) -> None:
        samples = {SIGNALS[CLAUDE]: commits("fork/of", 20, fork=True) + commits("old/archived", 20) + commits("Opted/Out", 20)
                   + commits("AIDER-AI/aider", 20) + commits("keep/me", 7)}
        details = {"old/archived": {"archived": True}}
        s = Scratch(optout="# comment\nopted/out\n")
        try:
            gh = FakeGitHub(samples, details)
            j = run(gh, s)
            self.assertEqual(discovered(j), ["keep/me"])
            self.assertEqual(j["dropped"]["archived"], ["old/archived"])
            self.assertEqual(j["dropped"]["opted_out"], ["Opted/Out"])
            self.assertEqual(j["dropped"]["not_candidates"], 1)  # the fork is never a candidate
            self.assertFalse(any("fork%2Fof" in u or "fork/of" in u for u in gh.urls if "repo" in u))
            text = s.list_text()
            self.assertEqual(text.lower().count("aider-ai/aider"), 1)
            self.assertNotIn("Opted/Out", text)
            # the seed's own count is recorded under the seed's spelling, marked as a seed
            seed = next(r for r in j["repositories"] if r["repo"] == "Aider-AI/aider")
            self.assertTrue(seed["seed"])
            self.assertEqual(seed["commits"], 20)
        finally:
            s.close()

    def test_renamed_seed_is_not_discovered_twice(self) -> None:
        # Survival Report #2 counted OpenHands twice: the seed under its old name,
        # the discovery under the name GitHub redirects to. Same bytes, two rows.
        samples = {SIGNALS[CLAUDE]: commits("OpenHands/OpenHands", 40) + commits("keep/me", 7)}
        details = {"All-Hands-AI/OpenHands": {"full_name": "OpenHands/OpenHands", "stars": 90000},
                   "OpenHands/OpenHands": {"stars": 90000}}
        s = Scratch(HEADER + "All-Hands-AI/OpenHands\n")
        try:
            gh = FakeGitHub(samples, details)
            j = run(gh, s)
            self.assertEqual(discovered(j), ["keep/me"])
            self.assertEqual(j["seeds_renamed"], {"All-Hands-AI/OpenHands": "OpenHands/OpenHands"})
            self.assertEqual(j["dropped"]["seed_duplicates"], ["OpenHands/OpenHands"])
            seed = next(r for r in j["repositories"] if r["repo"] == "All-Hands-AI/OpenHands")
            self.assertTrue(seed["seed"])
            self.assertEqual(seed["commits"], 40)  # counted under the new name, credited to the seed
            self.assertEqual(seed["now_named"], "OpenHands/OpenHands")
            self.assertNotIn("renamed_from", seed)
            text = s.list_text()
            self.assertEqual(text.count("OpenHands"), 1)
            self.assertIn("seeds renamed: All-Hands-AI/OpenHands → OpenHands/OpenHands", sd.summary(j))
        finally:
            s.close()

    def test_star_floor_drops_painters_and_mirrors_and_says_so(self) -> None:
        # 912 sampled commits: inside the 1000-result window the search API serves
        samples = {SIGNALS[CLAUDE]: commits("bot/graph-painter", 600) + commits("org/mirror", 300) + commits("real/project", 12)}
        details = {"bot/graph-painter": {"stars": 43}, "org/mirror": {"stars": 0}, "real/project": {"stars": 2400}}
        s = Scratch()
        try:
            j = run(FakeGitHub(samples, details), s, min_stars=100)
            self.assertEqual(discovered(j), ["real/project"])
            self.assertEqual(j["dropped"]["below_stars"], ["bot/graph-painter", "org/mirror"])
            self.assertEqual(j["selection"]["min_stars"], 100)
            self.assertIn("≥100 stars", s.list_text())
        finally:
            s.close()

    def test_details_resolve_stars_renames_and_gone_repositories(self) -> None:
        samples = {SIGNALS[CLAUDE]: commits("needs/details", 9) + commits("gone/repo", 9) + commits("now/private", 9) + commits("was/renamed", 9)}
        details = {"needs/details": {"stars": 42}, "now/private": {"private": True}, "was/renamed": {"full_name": "new/name", "stars": 3}}
        s = Scratch()
        try:
            gh = FakeGitHub(samples, details)
            gh.default_details = None  # unknown repositories are 404
            j = run(gh, s)
            by = {r["repo"]: r for r in j["repositories"]}
            self.assertEqual(by["needs/details"]["stars"], 42)
            self.assertEqual(by["new/name"]["renamed_from"], "was/renamed")
            self.assertNotIn("gone/repo", by)
            self.assertNotIn("now/private", by)
            self.assertEqual(j["dropped"]["unavailable"], ["gone/repo", "now/private"])
        finally:
            s.close()

    def test_no_verify_ranks_by_sample(self) -> None:
        samples = {SIGNALS[CLAUDE]: commits("x/one", 6)}
        s = Scratch()
        try:
            gh = FakeGitHub(samples, counts={("x/one", SIGNALS[CLAUDE]): 5000})
            j = run(gh, s, verify=False)
            r = next(r for r in j["repositories"] if r["repo"] == "x/one")
            self.assertEqual(r["commits"], 6)
            self.assertEqual(r["counts"], "sampled")
            self.assertFalse(j["selection"]["verified"])
            self.assertFalse(any("q=repo%3A" in u for u in gh.urls))
        finally:
            s.close()

    def test_count_failure_keeps_sampled_count_and_flags_it(self) -> None:
        samples = {SIGNALS[CLAUDE]: commits("x/one", 6)}

        class Flaky(FakeGitHub):
            def fetch(self, url: str):
                if "q=repo%3A" in url:
                    self.urls.append(url)
                    return 422, {}, b'{"message":"Validation Failed"}'
                return super().fetch(url)

        s = Scratch()
        try:
            j = run(Flaky(samples), s)
            r = next(r for r in j["repositories"] if r["repo"] == "x/one")
            self.assertEqual(r["counts"], "sampled")
            self.assertEqual(r["commits"], 6)
            self.assertEqual(j["dropped"]["unverified"], ["x/one"])
        finally:
            s.close()


class ListFileTests(unittest.TestCase):
    def test_rerun_replaces_discovered_section_and_keeps_first_seen(self) -> None:
        samples = {SIGNALS[CLAUDE]: commits("x/one", 6)}
        s = Scratch()
        try:
            run(FakeGitHub(samples), s, now="2026-10-01T04:23:00Z")
            first = s.list_text()
            samples[SIGNALS[CLAUDE]] += commits("y/two", 6)
            j = run(FakeGitHub(samples), s, now="2026-11-01T04:23:00Z")
            second = s.list_text()
            self.assertEqual(second.count(sd.MARKER), 1)
            self.assertIn("# discovered 2026-11-01", second)
            self.assertEqual(first.split(sd.MARKER)[0], second.split(sd.MARKER)[0])  # head untouched
            by = {r["repo"]: r for r in j["repositories"]}
            self.assertEqual(by["x/one"]["discovered_at"], "2026-10-01T04:23:00Z")
            self.assertEqual(by["y/two"]["discovered_at"], "2026-11-01T04:23:00Z")
            # a repository that fell out of the sample is counted again repository-wide;
            # with nothing left to count it falls below the floor and leaves, on the record
            gh = FakeGitHub({})
            j = run(gh, s, now="2026-12-01T04:23:00Z")
            self.assertEqual(discovered(j), [])
            self.assertIn("0 repositories", s.list_text())
            self.assertEqual(j["dropped"]["left_list"], ["x/one", "y/two"])
            self.assertEqual(j["previously_discovered"], 2)
            self.assertEqual(j["retained"], [])
            self.assertEqual(sum(1 for u in gh.urls if "q=repo%3A" in u), 2)  # one count each, the signal it was found with
        finally:
            s.close()

    def test_previously_discovered_repository_missed_by_the_sample_is_retained(self) -> None:
        # Week 1: steady/flow is sampled (2 hits) and counted (400 commits). Week 2 the
        # sample is all bursty/one; steady/flow is re-counted with its own signal and stays.
        counts = {("steady/flow", SIGNALS[CLAUDE]): 400}
        s = Scratch()
        try:
            j = run(FakeGitHub({SIGNALS[CLAUDE]: commits("steady/flow", 2)}, counts=counts), s, now="2026-10-01T04:23:00Z")
            self.assertEqual(discovered(j), ["steady/flow"])
            self.assertNotIn("retained", j["repositories"][-1])
            gh = FakeGitHub({SIGNALS[CLAUDE]: commits("bursty/one", 30)}, counts=counts)
            j = run(gh, s, now="2026-10-08T04:23:00Z")
            self.assertEqual(discovered(j), ["bursty/one", "steady/flow"])
            by = {r["repo"]: r for r in j["repositories"]}
            self.assertTrue(by["steady/flow"]["retained"])
            self.assertEqual(by["steady/flow"]["sampled"], 0)
            self.assertEqual(by["steady/flow"]["commits"], 400)
            self.assertEqual(by["steady/flow"]["counts"], "repository-wide")
            self.assertEqual(by["steady/flow"]["discovered_at"], "2026-10-01T04:23:00Z")
            self.assertNotIn("retained", by["bursty/one"])
            self.assertEqual(j["retained"], ["steady/flow"])
            self.assertEqual(j["dropped"]["left_list"], [])
            # counted with the one signal it was found with, not all of them
            self.assertEqual([u for u in gh.urls if "q=repo%3Asteady" in u.replace("%2F", "/")].__len__(), 1)
            self.assertIn("retained from earlier weeks: 1", sd.summary(j | {"_list_text": ""}))
            # week 3: the sample finds it again, it is a plain candidate, still on the record once
            gh = FakeGitHub({SIGNALS[CLAUDE]: commits("steady/flow", 3)}, counts=counts)
            j = run(gh, s, now="2026-10-15T04:23:00Z")
            self.assertEqual(discovered(j), ["steady/flow"])
            self.assertNotIn("retained", {r["repo"]: r for r in j["repositories"]}["steady/flow"])
            self.assertEqual(j["dropped"]["left_list"], ["bursty/one"])
            # --no-verify cannot re-count, so it does not retain
            j = run(FakeGitHub({SIGNALS[CLAUDE]: commits("bursty/one", 30)}), s, now="2026-10-22T04:23:00Z", verify=False)
            self.assertEqual(discovered(j), ["bursty/one"])
            self.assertEqual(j["dropped"]["left_list"], ["steady/flow"])
        finally:
            s.close()

    def test_retained_repository_that_fell_below_the_floors_leaves_and_is_named(self) -> None:
        counts = {("fading/repo", SIGNALS[CLAUDE]): 40}
        s = Scratch()
        try:
            run(FakeGitHub({SIGNALS[CLAUDE]: commits("fading/repo", 2)}, counts=counts), s, now="2026-10-01T04:23:00Z")
            # its repository-wide count is now 3 (< floor 5): it leaves
            j = run(FakeGitHub({}, counts={("fading/repo", SIGNALS[CLAUDE]): 3}), s, now="2026-10-08T04:23:00Z")
            self.assertEqual(discovered(j), [])
            self.assertEqual(j["dropped"]["below_floor"], 1)
            self.assertEqual(j["dropped"]["left_list"], ["fading/repo"])
            self.assertIn("left the list: fading/repo", sd.summary(j | {"_list_text": ""}))
        finally:
            s.close()

    def test_split_list_reads_seeds_and_previous(self) -> None:
        head, seeds, previous = sd.split_list(HEADER + "\n# discovered 2026-10-01 by x: 1 repositories\n# note\nz/prev\n")
        self.assertEqual(seeds, ["Aider-AI/aider", "croviatrust/causari"])
        self.assertEqual(previous, ["z/prev"])
        self.assertEqual(head[-1], "croviatrust/causari")

    def test_real_list_parses_as_seeds(self) -> None:
        text = (SCRIPTS.parent / ".github" / "survival-repos.txt").read_text(encoding="utf-8")
        head, seeds, _ = sd.split_list(text)
        self.assertGreaterEqual(len(seeds), 10)
        self.assertTrue(all("/" in s for s in seeds))
        self.assertTrue(head[0].startswith("#"))

    def test_dry_run_writes_nothing(self) -> None:
        s = Scratch()
        try:
            gh = FakeGitHub({SIGNALS[CLAUDE]: commits("x/one", 6)})
            saved = sd.urllib_fetch, sd.time.sleep, sd.SIGNALS
            sd.urllib_fetch = lambda url, token=None: gh.fetch(url)  # type: ignore[assignment]
            sd.time.sleep = gh.sleep  # type: ignore[assignment]
            sd.SIGNALS = SIGNALS
            out = io.StringIO()
            try:
                with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
                    rc = sd.main(["--root", str(s.root), "--dry-run", "--pages", "1", "--min-stars", "0"])
            finally:
                sd.urllib_fetch, sd.time.sleep, sd.SIGNALS = saved
            self.assertEqual(rc, 0)
            self.assertIn("x/one", out.getvalue())
            self.assertIn("survival_discover:", out.getvalue())
            self.assertTrue(gh.sleeps)  # paced through the fake, never a real sleep
            self.assertEqual(s.list_text(), HEADER)
            self.assertFalse((s.root / ".github" / "survival-discovery.json").exists())
        finally:
            s.close()


class ClientTests(unittest.TestCase):
    def test_rate_limit_sleeps_then_retries(self) -> None:
        gh = FakeGitHub({SIGNALS[CLAUDE]: commits("x/one", 6)})
        gh.rate_limit_first = 2
        client = sd.Client(gh.fetch, sleep=gh.sleep, log=lambda _s: None)
        items, incomplete = sd.search_signal(client, "c", SIGNALS[CLAUDE], pages=3, per_page=100, until="2026-10-01")
        self.assertEqual(len(items), 6)
        self.assertFalse(incomplete)
        self.assertEqual(gh.sleeps.count(7.0), 2)
        self.assertEqual(client.requests, 3)

    def test_wait_for_reads_reset_header(self) -> None:
        client = sd.Client(lambda u: (200, {}, b""), sleep=lambda s: None)
        w = client.wait_for(403, {"x-ratelimit-remaining": "0", "x-ratelimit-reset": str(int(time.time()) + 30)})
        self.assertTrue(25 <= w <= 32)
        self.assertEqual(client.wait_for(403, {"x-ratelimit-remaining": "12"}), 60.0)  # secondary limit: no delay named
        self.assertIsNone(client.wait_for(200, {}))
        self.assertIsNone(client.wait_for(422, {}))
        self.assertEqual(client.wait_for(429, {"retry-after": "9999"}), sd.MAX_WAIT_S)

    def test_paging_stops_on_short_page_and_paces(self) -> None:
        gh = FakeGitHub({SIGNALS[CLAUDE]: commits("x/one", 250)})
        client = sd.Client(gh.fetch, sleep=gh.sleep, log=lambda _s: None)
        items, incomplete = sd.search_signal(client, "c", SIGNALS[CLAUDE], pages=10, per_page=100, until="2026-10-01")
        self.assertEqual(len(items), 250)
        self.assertEqual(client.requests, 3)  # 100, 100, 50: stop
        self.assertEqual(gh.sleeps, [sd.SEARCH_PACE_S, sd.SEARCH_PACE_S])
        self.assertFalse(incomplete)

    def test_paging_stops_quietly_at_the_1000th_result(self) -> None:
        gh = FakeGitHub({SIGNALS[CLAUDE]: commits("x/one", 1100)})
        client = sd.Client(gh.fetch, sleep=gh.sleep, log=lambda _s: None)
        items, incomplete = sd.search_signal(client, "c", SIGNALS[CLAUDE], pages=12, per_page=100, until="2026-10-01")
        self.assertEqual(len(items), 1000)
        self.assertFalse(incomplete)  # 422 past the cap is expected, not an incident

    def test_server_error_marks_signal_incomplete(self) -> None:
        client = sd.Client(lambda url: (503, {}, b""), sleep=lambda s: None, log=lambda _s: None)
        items, incomplete = sd.search_signal(client, "c", "x", pages=2, per_page=100, until="2026-10-01")
        self.assertEqual(items, [])
        self.assertTrue(incomplete)

    def test_urls(self) -> None:
        url = sd.sample_url('"Co-Authored-By: Claude"', 2, 100, "2026-10-01")
        qs = urllib.parse.parse_qs(urllib.parse.urlparse(url).query)
        self.assertEqual(qs["q"], ['"Co-Authored-By: Claude" is:public merge:false author-date:<=2026-10-01'])
        self.assertEqual(qs["sort"], ["author-date"])
        self.assertEqual(qs["page"], ["2"])
        qs = urllib.parse.parse_qs(urllib.parse.urlparse(sd.count_url("Owner/Name", "author-name:aider")).query)
        self.assertEqual(qs["q"], ["repo:Owner/Name author-name:aider is:public merge:false"])
        self.assertEqual(qs["per_page"], ["1"])
        self.assertEqual(sd.repo_url("Owner/Name"), "https://api.github.com/repos/Owner/Name")

    def test_signals_are_the_detectors_signals(self) -> None:
        # every VERIFIED signal named on the method page has a query
        joined = " ".join(sd.SIGNALS.values())
        for needle in ("Co-Authored-By: Claude", "Copilot", "Codex", "ChatGPT", "Drafted-With", "Executed-By", "AI-Model", "AI-Agent",
                       "AI-Tool", "AI-Assisted: yes", "Assisted-by", "noreply@anthropic.com", "devin-ai-integration", "openhands",
                       "cursoragent", "google-labs-jules", "aider"):
            self.assertIn(needle, joined, needle)


if __name__ == "__main__":
    unittest.main()
