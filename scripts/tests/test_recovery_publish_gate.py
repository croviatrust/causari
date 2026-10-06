#!/usr/bin/env python3
"""The Report #4 publication gate. No network, no tokens."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import recovery_publish_gate as gate  # noqa: E402

MEASUREMENT = gate.MEASUREMENT_SHA
OTHER = "897ddbc73bd41519bf10cc571ec9393bc1af0491"
CHILD = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
WORKFLOW = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "survival-report-recover.yml"
REPORT_FIXTURE = Path(__file__).resolve().parent / "fixtures" / "report-2026-04" / "report.json"


def report(**overrides) -> dict:
    body = {
        "id": gate.REPORT_ID,
        "number": 4,
        "generated_at": gate.GENERATED_AT,
        "method": {"version": "v3"},
        "tool": {"name": "causari", "version": "0.3.0"},
        "reach": {"run_id": gate.REACH_RUN, "closed_at": "2026-10-05T18:22:35Z", "witness": {"key_hex": "ab"}},
        "doi": None,
        "concept_doi": None,
        "zenodo": None,
    }
    body.update(overrides)
    return body


def _record(title: str, doi: str = "10.5281/zenodo.1") -> dict:
    return {
        "doi": doi,
        "conceptdoi": gate.CONCEPT_DOI,
        "submitted": True,
        "state": "done",
        "metadata": {
            "title": title,
            "doi": doi,
            "version": gate.ZENODO_VERSION,
            "related_identifiers": [
                {"identifier": gate.REPORT_URL, "relation": "isIdenticalTo", "resource_type": "publication-report"},
            ],
        },
        "files": [{"filename": "report.json", "checksum": "md5:" + ("ab" * 16)}],
    }


def provenance() -> dict:
    return {
        "source_sha": MEASUREMENT,
        "source_run_id": gate.SOURCE_RUN,
        "measurement_generated_at": gate.GENERATED_AT,
        "recovery_assembled_at": "2026-10-05T18:22:35Z",
        "reach_run_id": gate.REACH_RUN,
        "report_id": gate.REPORT_ID,
        "deterministic_report_sha256": gate.CANON_SHA,
    }


class ControlPlane(unittest.TestCase):
    def test_executing_commit_may_differ_from_the_measurement(self):
        gate.assert_control_plane("refs/heads/main", OTHER, OTHER)

    def test_measurement_sha_is_not_what_main_must_equal(self):
        with self.assertRaises(SystemExit):
            gate.assert_control_plane("refs/heads/main", OTHER, MEASUREMENT)

    def test_main_must_be_the_executing_commit(self):
        with self.assertRaises(SystemExit):
            gate.assert_control_plane("refs/heads/main", OTHER, CHILD)

    def test_only_main_can_publish(self):
        with self.assertRaises(SystemExit):
            gate.assert_ref("refs/heads/other")

    def test_pusher_is_the_crovia_admin_and_admins_can_bypass(self):
        gate.assert_pusher("croviatrust", "admin", "false")
        with self.assertRaises(SystemExit):
            gate.assert_pusher("someone-else", "admin", "false")
        with self.assertRaises(SystemExit):
            gate.assert_pusher("croviatrust", "write", "false")
        with self.assertRaises(SystemExit):
            gate.assert_pusher("croviatrust", "admin", "true")

    def test_production_sandbox_values(self):
        for value in ("", "0", "false", "no"):
            gate.assert_production_sandbox(value)
        with self.assertRaises(SystemExit):
            gate.assert_production_sandbox("1")


class History(unittest.TestCase):
    def test_no_commits_means_the_reviewed_tip(self):
        self.assertEqual(gate.classify_history([]), "at-tip")

    def test_report_commit_is_limited_to_publication_paths(self):
        rows = [(CHILD, gate.REPORT_SUBJECT, ["site/reports/survival/latest.json", "site/sitemap.xml"])]
        self.assertEqual(gate.classify_history(rows), "report")
        bad = [(CHILD, gate.REPORT_SUBJECT, ["site/reports/survival/latest.json", "README.md"])]
        with self.assertRaises(SystemExit):
            gate.classify_history(bad)

    def test_doi_commit_must_follow_the_report_commit(self):
        doi = gate.doi_subject("10.5281/zenodo.23019874")
        rows = [
            (CHILD, gate.REPORT_SUBJECT, ["site/reports/survival/2026/04/report.json"]),
            (OTHER, doi, ["site/reports/survival/zenodo.json"]),
        ]
        self.assertEqual(gate.classify_history(rows), "doi")
        with self.assertRaises(SystemExit):
            gate.classify_history([(CHILD, doi, ["site/reports/survival/zenodo.json"])])

    def test_publish_and_deposit_decisions(self):
        self.assertEqual(gate.publish_decision("at-tip", "absent"), "commit")
        self.assertEqual(gate.publish_decision("report", "ready"), "already")
        self.assertEqual(gate.publish_decision("doi", "deposited"), "already")
        self.assertEqual(gate.deposit_phase("report", "ready"), "proceed")
        self.assertEqual(gate.deposit_phase("doi", "deposited"), "skip")
        with self.assertRaises(SystemExit):
            gate.publish_decision("at-tip", "deposited")
        with self.assertRaises(SystemExit):
            gate.deposit_phase("at-tip", "absent")


class DepositChoice(unittest.TestCase):
    def test_doi_fields_do_not_change_the_neutral_hash(self):
        original = report()
        edited = report(doi="10.5281/zenodo.1", concept_doi="10.5281/zenodo.2", zenodo={"id": 1})
        self.assertEqual(gate.canonical_sha(original), gate.canonical_sha(gate.neutralized(edited)))
        self.assertNotEqual(gate.canonical_sha(original), gate.canonical_sha(edited))

    def test_adoption_requires_the_deposited_identity_not_the_title(self):
        title = "Causari — Survival Report #4"
        identity = gate.DepositIdentity(title, gate.REPORT_URL, gate.ZENODO_VERSION, gate.CONCEPT_DOI, "ab" * 16)
        published = _record(title, doi="10.5281/zenodo.1")
        other = {"metadata": {"title": "something else"}, "submitted": True, "doi": "10.5281/zenodo.9"}
        action, chosen = gate.select_record([other, published], identity)
        self.assertEqual(action, "adopt")
        self.assertEqual(chosen["doi"], "10.5281/zenodo.1")
        title_only = {"metadata": {"title": title, "doi": "10.5281/zenodo.2"}, "doi": "10.5281/zenodo.2", "submitted": True}
        with self.assertRaises(SystemExit):
            gate.select_record([title_only], identity)
        draft = {"metadata": {"title": title}, "state": "unsubmitted", "submitted": False}
        with self.assertRaises(SystemExit):
            gate.select_record([draft], identity)
        with self.assertRaises(SystemExit):
            gate.select_record([published, dict(published, doi="10.5281/zenodo.3")], identity)
        self.assertEqual(gate.select_record([other], identity), ("create", None))

    def test_state_and_report_must_agree_before_any_list(self):
        body = report(doi="10.5281/zenodo.23019874")
        state = {"production": {"reports": {"2026/04": {"doi": "10.5281/zenodo.23019874"}}}}
        self.assertEqual(gate.deposit_action(body, state, None), "skip")
        with self.assertRaises(SystemExit):
            gate.deposit_action(report(), state, None)

    def test_adopt_updates_state_and_does_not_create(self):
        import zenodo_deposit

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            site = root / "site"
            report_dir = site / "reports" / "survival" / "2026" / "04"
            report_dir.mkdir(parents=True)
            (report_dir / "report.json").write_text(json.dumps(report()), encoding="utf-8")
            state_path = site / "reports" / "survival" / "zenodo.json"
            state_path.write_text(json.dumps({
                "production": {"concept_doi": "10.5281/zenodo.22863965", "concept_id": 22863966,
                               "reports": {"2026/01": {"doi": "10.5281/zenodo.22863966"}}},
            }), encoding="utf-8")
            calls = []
            originals = {
                name: getattr(zenodo_deposit, name)
                for name in ("deposit", "gather", "content_hash", "write_back")
            }
            zenodo_deposit.deposit = lambda *args, **kwargs: calls.append("deposit")
            zenodo_deposit.gather = lambda _directory: {"report.json": b"{}"}
            zenodo_deposit.content_hash = lambda _files: "abc"
            zenodo_deposit.write_back = lambda report_dir, facts, rec, site: facts.update(doi=rec["doi"])
            try:
                gate.adopt_record(report_dir, site, {
                "id": 99,
                "doi": "10.5281/zenodo.24000000",
                "conceptdoi": "10.5281/zenodo.22863965",
                "metadata": {"title": "x", "publication_date": "2026-10-05"},
                "links": {"record_html": "https://zenodo.org/records/99"},
                "submitted": True,
            })
            finally:
                for name, fn in originals.items():
                    setattr(zenodo_deposit, name, fn)
            saved = json.loads(state_path.read_text(encoding="utf-8"))
            self.assertEqual(calls, [])
            self.assertEqual(saved["production"]["reports"]["2026/01"]["doi"], "10.5281/zenodo.22863966")
            self.assertEqual(saved["production"]["reports"]["2026/04"]["doi"], "10.5281/zenodo.24000000")
            self.assertEqual(saved["production"]["reports"]["2026/04"]["content_sha256"], "abc")
            self.assertFalse(saved["production"]["reports"]["2026/04"]["sandbox"])


class GitHistory(unittest.TestCase):
    def test_unrelated_commit_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            subprocess.check_call(["git", "init", "-b", "main"], cwd=repo, stdout=subprocess.DEVNULL)
            subprocess.check_call(["git", "config", "user.email", "t@example.com"], cwd=repo)
            subprocess.check_call(["git", "config", "user.name", "Test"], cwd=repo)
            (repo / "README.md").write_text("base\n", encoding="utf-8")
            subprocess.check_call(["git", "add", "README.md"], cwd=repo)
            subprocess.check_call(["git", "commit", "-m", "base"], cwd=repo, stdout=subprocess.DEVNULL)
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
            subprocess.check_call(["git", "update-ref", "refs/remotes/origin/main", base], cwd=repo)
            self.assertEqual(gate.classify_history(gate.history_rows(repo, base)), "at-tip")
            (repo / "README.md").write_text("moved\n", encoding="utf-8")
            subprocess.check_call(["git", "add", "README.md"], cwd=repo)
            subprocess.check_call(["git", "commit", "-m", gate.REPORT_SUBJECT], cwd=repo, stdout=subprocess.DEVNULL)
            tip = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
            subprocess.check_call(["git", "update-ref", "refs/remotes/origin/main", tip], cwd=repo)
            with self.assertRaises(SystemExit):
                gate.classify_history(gate.history_rows(repo, base))


IDENT = gate.DepositIdentity(
    "Causari — Survival Report #4", gate.REPORT_URL, gate.ZENODO_VERSION, gate.CONCEPT_DOI, "ab" * 16
)


def next_action(history: str, kind: str, records: list | None = None, *, rows=None, artifact_ok: bool = True) -> str:
    if not artifact_ok:
        return "REFUSE"
    try:
        if rows is not None:
            history = gate.classify_history(rows)
        decision = gate.publish_decision(history, kind)
    except SystemExit:
        return "REFUSE"
    if decision == "commit":
        return "PUBLISH_REPORT"
    if history == "doi" and kind == "deposited":
        return "NOOP"
    if kind != "ready":
        return "REFUSE"
    try:
        action, _chosen = gate.select_record(records or [], IDENT)
    except SystemExit:
        return "REFUSE"
    return {"create": "CREATE_DEPOSIT", "adopt": "ADOPT_DEPOSIT"}[action]


class PartialStates(unittest.TestCase):
    def test_matrix(self):
        title = IDENT.title
        published = _record(title)
        draft = {"metadata": {"title": title, "version": gate.ZENODO_VERSION,
                              "related_identifiers": [{"identifier": gate.REPORT_URL, "relation": "isIdenticalTo"}]},
                 "state": "unsubmitted", "submitted": False}
        unrelated = [( "b" * 40, "docs: unrelated", ["README.md"])]
        self.assertEqual(next_action("at-tip", "absent", []), "PUBLISH_REPORT")
        self.assertEqual(next_action("report", "ready", []), "CREATE_DEPOSIT")
        self.assertEqual(next_action("report", "ready", [published]), "ADOPT_DEPOSIT")
        self.assertEqual(next_action("report", "ready", [draft]), "REFUSE")
        self.assertEqual(next_action("report", "ready", [published, _record(title, "10.5281/zenodo.2")]), "REFUSE")
        self.assertEqual(next_action("doi", "deposited", []), "NOOP")
        self.assertEqual(next_action("at-tip", "absent", rows=unrelated), "REFUSE")
        edited = [("c" * 40, "edit the report page", ["site/reports/survival/2026/04/report.json"])]
        self.assertEqual(next_action("report", "ready", rows=edited), "REFUSE")
        self.assertEqual(next_action("report", "ready", [published], artifact_ok=False), "REFUSE")


class WorkflowText(unittest.TestCase):
    def setUp(self):
        self.text = WORKFLOW.read_text(encoding="utf-8")
        self.assemble = self.text.split("\n  publish:\n", 1)[0]
        self.publish = self.text.split("\n  publish:\n", 1)[1].split("\n  deposit:\n", 1)[0]
        self.deposit = self.text.split("\n  deposit:\n", 1)[1]

    def test_measurement_pin_stays_on_assemble(self):
        self.assertIn("ref: " + MEASUREMENT, self.assemble)
        self.assertIn(gate.SOURCE_RUN, self.assemble)
        self.assertIn(gate.CANON_SHA, self.assemble)
        self.assertNotIn("ref: " + MEASUREMENT, self.publish)

    def test_publication_binds_to_the_executing_commit(self):
        self.assertIn("ref: ${{ github.sha }}", self.publish)
        self.assertIn("fetch-depth: 0", self.publish)
        self.assertIn("assert-control-plane", self.publish)
        self.assertIn("push --ff-only origin HEAD:main", self.publish)
        self.assertIn("push --ff-only origin HEAD:main", self.deposit)
        self.assertNotIn("measured commit", self.publish)
        self.assertNotIn("897ddbc73bd41519bf10cc571ec9393bc1af0491", self.text)
        self.assertIn("cp -a /tmp/assembled/reports/survival/. site/reports/survival/", self.publish)
        self.assertIn("cp -a /tmp/assembled/r/. site/r/", self.publish)
        self.assertNotIn("cp -a /tmp/assembled/reports/survival site/reports/survival", self.publish)
        self.assertNotIn("cp -a /tmp/assembled/r site/r", self.publish)
        self.assertNotIn("/tmp/assembled/site/", self.text)

    def test_deposit_lists_before_it_creates_and_can_adopt(self):
        self.assertIn("deposit-plan", self.deposit)
        self.assertIn("recovery_publish_gate.py adopt", self.deposit)
        self.assertIn("assert-production-sandbox", self.deposit)
        self.assertLess(self.deposit.index("deposit-plan"), self.deposit.index("zenodo_deposit.py"))


def _publication_copies() -> list[str]:
    publish = WORKFLOW.read_text(encoding="utf-8").split("\n  publish:\n", 1)[1].split("\n  deposit:\n", 1)[0]
    return [line.strip() for line in publish.splitlines() if line.strip().startswith("cp -a ")]


def _seed_existing_site(root: Path) -> tuple[Path, Path]:
    """Source artifact plus a site whose survival and r directories already exist."""
    assembled = root / "assembled"
    repo = root / "repo"
    survival = assembled / "reports" / "survival"
    (survival / "2026" / "04").mkdir(parents=True)
    shutil.copy(REPORT_FIXTURE, survival / "2026" / "04" / "report.json")
    (survival / "2026" / "04" / "provenance.json").write_text(
        json.dumps(provenance(), indent=1) + "\n", encoding="utf-8"
    )
    (survival / "latest.json").write_text(
        json.dumps({"id": gate.REPORT_ID, "number": 4}, indent=1) + "\n", encoding="utf-8"
    )
    card = assembled / "r" / "openai-openai-python"
    card.mkdir(parents=True)
    (card / "index.html").write_text("card\n", encoding="utf-8")
    prior = repo / "site" / "reports" / "survival" / "2026" / "03"
    prior.mkdir(parents=True)
    (prior / "report.json").write_text("{}\n", encoding="utf-8")
    old = repo / "site" / "r" / "aider-ai"
    old.mkdir(parents=True)
    (old / "index.html").write_text("old\n", encoding="utf-8")
    return assembled, repo


class ExistingDestinationCopy(unittest.TestCase):
    def test_content_copy_does_not_nest_and_verify_worktree_accepts_it(self):
        copies = _publication_copies()
        self.assertEqual(copies[:2], [
            "cp -a /tmp/assembled/reports/survival/. site/reports/survival/",
            "cp -a /tmp/assembled/r/. site/r/",
        ])
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            assembled, repo = _seed_existing_site(root / "fixed")
            script = "\n".join(line.replace("/tmp/assembled", str(assembled)) for line in copies[:2])
            subprocess.check_call(["bash", "-euo", "pipefail", "-c", script], cwd=repo)
            survival = repo / "site" / "reports" / "survival"
            cards = repo / "site" / "r"
            self.assertFalse((survival / "survival").exists())
            self.assertFalse((cards / "r").exists())
            report_path = survival / "2026" / "04" / "report.json"
            provenance_path = survival / "2026" / "04" / "provenance.json"
            latest_path = survival / "latest.json"
            self.assertTrue(report_path.is_file())
            self.assertTrue(provenance_path.is_file())
            self.assertTrue(latest_path.is_file())
            self.assertEqual((cards / "openai-openai-python" / "index.html").read_text(encoding="utf-8"), "card\n")
            self.assertEqual((cards / "aider-ai" / "index.html").read_text(encoding="utf-8"), "old\n")
            self.assertTrue((survival / "2026" / "03" / "report.json").is_file())
            files = sorted(
                path.relative_to(survival).as_posix() for path in survival.rglob("*") if path.is_file()
            )
            self.assertEqual(files, [
                "2026/03/report.json",
                "2026/04/provenance.json",
                "2026/04/report.json",
                "latest.json",
            ])
            copied = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertEqual(gate.canonical_sha(copied), gate.CANON_SHA)
            subprocess.check_call([
                sys.executable,
                str(Path(__file__).resolve().parents[1] / "recovery_publish_gate.py"),
                "--repo", str(repo),
                "verify-worktree",
            ])
            self.assertEqual(gate.verify_survival_tree(survival, allow_deposited=False), "ready")

            nested_assembled, nested_repo = _seed_existing_site(root / "nested")
            old = "\n".join([
                f"cp -a {nested_assembled}/reports/survival site/reports/survival",
                f"cp -a {nested_assembled}/r site/r",
            ])
            subprocess.check_call(["bash", "-euo", "pipefail", "-c", old], cwd=nested_repo)
            self.assertTrue((nested_repo / "site" / "reports" / "survival" / "survival").is_dir())
            self.assertTrue((nested_repo / "site" / "r" / "r").is_dir())
            self.assertFalse((nested_repo / "site" / "reports" / "survival" / "2026" / "04" / "report.json").exists())
            failed = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).resolve().parents[1] / "recovery_publish_gate.py"),
                    "--repo", str(nested_repo),
                    "verify-worktree",
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(failed.returncode, 1)
            self.assertIn("missing the measured report", failed.stderr)


if __name__ == "__main__":
    unittest.main()
