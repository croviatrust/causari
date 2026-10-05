#!/usr/bin/env python3
"""Static checks for the read-only publication preflight. No network."""

from __future__ import annotations

import json
import os
import stat
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "publication-preflight.yml"
PRODUCTION = ("", "0", "false", "no")
ZENODO_URL = "https://zenodo.org/api/deposit/depositions?size=1"
FORBIDDEN = (
    "git push",
    "git commit",
    "git checkout",
    "git switch",
    "git branch",
    "git tag",
    "git reset",
    "--delete",
    "newversion",
    "survival-report-recover",
    "survival_report",
    "upload-artifact",
    "download-artifact",
    "zenodo_deposit",
    "sandbox.zenodo.org",
    "access_token",
    "--location",
    "--retry",
    "workflow_call",
    "workflow_run",
    "repository_dispatch",
    "pull_request",
    "contents: write",
    "actions: write",
    "github.token",
    "secrets.GITHUB_TOKEN",
)


def run_block(text: str, step_name: str) -> str:
    marker = f"      - name: {step_name}\n"
    rest = text[text.index(marker):]
    body = rest[rest.index("        run: |\n") + len("        run: |\n"):]
    lines = []
    for line in body.splitlines():
        if line.startswith("          "):
            lines.append(line[10:])
        elif line == "":
            lines.append("")
        else:
            break
    return "\n".join(lines) + "\n"


class PublicationPreflight(unittest.TestCase):
    def setUp(self):
        self.text = WORKFLOW.read_text(encoding="utf-8")

    def test_dispatch_only_and_contents_read(self):
        self.assertEqual(self.text.count("\non:\n"), 1)
        self.assertIn("\non:\n  workflow_dispatch:\n", self.text)
        self.assertEqual(self.text.count("workflow_dispatch"), 1)
        self.assertIn("\npermissions:\n  contents: read\n", self.text)
        self.assertEqual(self.text.count("contents:"), 1)
        self.assertNotIn("push:", self.text)
        self.assertNotIn("schedule:", self.text)
        self.assertNotRegex(self.text, r"\b(POST|PUT|DELETE|PATCH)\b")
        self.assertNotRegex(self.text, r"\bpublish\b")
        self.assertNotRegex(self.text, r"\bupload\b")
        for needle in FORBIDDEN:
            self.assertNotIn(needle, self.text)
        self.assertEqual(self.text.count(ZENODO_URL), 1)
        self.assertEqual(self.text.count("secrets.REPORT_PUSH_TOKEN"), 1)
        self.assertEqual(self.text.count("secrets.ZENODO_TOKEN"), 1)
        self.assertEqual(self.text.count("vars.ZENODO_SANDBOX"), 1)
        self.assertIn("gh api user --jq .login", self.text)
        self.assertIn("/collaborators/${login}/permission", self.text)
        self.assertIn("enforce_admins.enabled", self.text)
        self.assertIn('"$login" != "croviatrust"', self.text)
        self.assertIn('"$perm" != "admin"', self.text)
        self.assertIn('"$enforce" != "false"', self.text)
        self.assertIn("unset GITHUB_TOKEN", self.text)
        self.assertIn("export GH_HOST=github.com", self.text)
        self.assertNotIn("uses:", self.text)
        for line in self.text.splitlines():
            if "echo" in line or "printf" in line:
                self.assertNotIn("REPORT_PUSH_TOKEN}", line)
                self.assertNotIn("ZENODO_TOKEN}", line)
                self.assertNotIn("ZENODO_SANDBOX}", line)
                self.assertNotIn("GH_TOKEN}", line)

    def test_sandbox_values_match_the_publication_gate(self):
        gate = (ROOT / "scripts" / "recovery_publish_gate.py").read_text(encoding="utf-8")
        self.assertIn('PRODUCTION_SANDBOX = {"", "0", "false", "no"}', gate)
        script = run_block(self.text, "Zenodo environment")
        self.assertIn('""|0|false|no)', script)
        for good in PRODUCTION:
            proc = self._run(script, {"ZENODO_SANDBOX": good, "ZENODO_TOKEN": ""})
            self.assertEqual(proc.returncode, 1, proc.stderr)
            self.assertEqual(proc.stdout, "ZENODO_PRODUCTION=PASS\nZENODO_TOKEN=FAIL\n")
            self.assertEqual(proc.stderr, "")
            self.assertEqual(self._calls(proc, "curl"), [])

    def test_invalid_sandbox_is_not_printed_and_skips_zenodo(self):
        script = run_block(self.text, "Zenodo environment")
        for bad in ("1", "true", "yes", "False", "NO", " false", "sandbox", "bad;echo LEAK"):
            proc = self._run(script, {"ZENODO_SANDBOX": bad, "ZENODO_TOKEN": "present"})
            self.assertEqual(proc.returncode, 1)
            self.assertEqual(proc.stdout, "ZENODO_PRODUCTION=FAIL\n")
            self.assertNotIn(bad, proc.stderr)
            if bad not in "ZENODO_PRODUCTION=FAIL\n":
                self.assertNotIn(bad, proc.stdout)
            self.assertEqual(self._calls(proc, "curl"), [])

    def test_zenodo_get_once_and_hides_body_and_header(self):
        script = run_block(self.text, "Zenodo environment")
        token = "zenodo-test-token"
        proc = self._run(
            script,
            {"ZENODO_SANDBOX": "false", "ZENODO_TOKEN": token, "PREFLIGHT_CURL_STATUS": "200"},
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(
            proc.stdout,
            "ZENODO_PRODUCTION=PASS\nZENODO_TOKEN=PASS\nZENODO_HTTP=200\n",
        )
        self.assertNotIn(token, proc.stdout)
        self.assertNotIn("Authorization", proc.stdout)
        self.assertNotIn("PRIVATE-DEPOSITION", proc.stdout)
        calls = self._calls(proc, "curl")
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0]["url"], ZENODO_URL)
        self.assertEqual(calls[0]["method"], "GET")
        self.assertTrue(calls[0]["auth"])
        self.assertEqual(calls[0]["count"], 1)

        refused = self._run(
            script,
            {"ZENODO_SANDBOX": "0", "ZENODO_TOKEN": token, "PREFLIGHT_CURL_STATUS": "401"},
        )
        self.assertEqual(refused.returncode, 1)
        self.assertEqual(refused.stdout, "ZENODO_PRODUCTION=PASS\nZENODO_TOKEN=FAIL\nZENODO_HTTP=401\n")
        self.assertNotIn(token, refused.stdout)
        self.assertNotIn("PRIVATE-DEPOSITION", refused.stdout)

        down = self._run(
            script,
            {"ZENODO_SANDBOX": "no", "ZENODO_TOKEN": token, "PREFLIGHT_CURL_EXIT": "7"},
        )
        self.assertEqual(down.returncode, 1)
        self.assertEqual(down.stdout, "ZENODO_PRODUCTION=PASS\nZENODO_TOKEN=FAIL\nZENODO_HTTP=0\n")

    def test_github_credential_outcomes(self):
        script = run_block(self.text, "GitHub publication credential")
        absent = self._run(script, {"REPORT_PUSH_TOKEN": "", "GITHUB_REPOSITORY": "croviatrust/causari"})
        self.assertEqual(absent.stdout, "REPORT_PUSH_TOKEN=FAIL\n")
        self.assertEqual(self._calls(absent, "gh"), [])

        unauth = self._github(script, user=("1", ""))
        self.assertEqual(unauth.stdout, "REPORT_PUSH_TOKEN=FAIL\n")
        self.assertEqual([c["kind"] for c in self._calls(unauth, "gh")], ["user"])

        other = self._github(script, user=("0", "someone"))
        self.assertEqual(other.stdout, "REPORT_PUSH_TOKEN=FAIL\nGITHUB_LOGIN=someone\n")
        self.assertEqual([c["kind"] for c in self._calls(other, "gh")], ["user"])

        weird = self._github(script, user=("0", "ok\nREPORT_PUSH_TOKEN=PASS"))
        self.assertEqual(weird.stdout, "REPORT_PUSH_TOKEN=FAIL\n")
        self.assertNotIn("REPORT_PUSH_TOKEN=PASS", weird.stdout)

        writer = self._github(script, user=("0", "croviatrust"), permission=("0", "write"))
        self.assertEqual(
            writer.stdout,
            "REPORT_PUSH_TOKEN=FAIL\nGITHUB_LOGIN=croviatrust\nGITHUB_PERMISSION=write\n",
        )
        self.assertEqual([c["kind"] for c in self._calls(writer, "gh")], ["user", "permission"])

        blocked = self._github(
            script,
            user=("0", "croviatrust"),
            permission=("0", "admin"),
            protection=("0", "true"),
        )
        self.assertEqual(
            blocked.stdout,
            "REPORT_PUSH_TOKEN=FAIL\nGITHUB_LOGIN=croviatrust\n"
            "GITHUB_PERMISSION=admin\nENFORCE_ADMINS=true\n",
        )

        unknown = self._github(
            script,
            user=("0", "croviatrust"),
            permission=("0", "admin"),
            protection=("1", ""),
        )
        self.assertEqual(
            unknown.stdout,
            "REPORT_PUSH_TOKEN=FAIL\nGITHUB_LOGIN=croviatrust\nGITHUB_PERMISSION=admin\n",
        )
        self.assertNotIn("unknown", unknown.stdout)

        passed = self._github(
            script,
            user=("0", "croviatrust"),
            permission=("0", "admin"),
            protection=("0", "false"),
        )
        self.assertEqual(passed.returncode, 0, passed.stderr)
        self.assertEqual(
            passed.stdout,
            "REPORT_PUSH_TOKEN=PASS\nGITHUB_LOGIN=croviatrust\n"
            "GITHUB_PERMISSION=admin\nENFORCE_ADMINS=false\n",
        )
        kinds = self._calls(passed, "gh")
        self.assertEqual([c["kind"] for c in kinds], ["user", "permission", "protection"])
        self.assertEqual(kinds[1]["path"], "repos/croviatrust/causari/collaborators/croviatrust/permission")
        self.assertEqual(kinds[2]["path"], "repos/croviatrust/causari/branches/main/protection")
        self.assertTrue(all(c["token"] == "push" for c in kinds))

    def _github(self, script: str, user, permission=("0", "admin"), protection=("0", "false")):
        return self._run(
            script,
            {
                "REPORT_PUSH_TOKEN": "push-test-token",
                "GITHUB_TOKEN": "workflow-token-must-not-be-used",
                "GITHUB_REPOSITORY": "croviatrust/causari",
                "PREFLIGHT_GH_USER": json.dumps(user),
                "PREFLIGHT_GH_PERMISSION": json.dumps(permission),
                "PREFLIGHT_GH_PROTECTION": json.dumps(protection),
            },
        )

    def _run(self, script: str, env: dict) -> subprocess.CompletedProcess:
        with tempfile.TemporaryDirectory() as tmp:
            bindir = Path(tmp) / "bin"
            bindir.mkdir()
            log = Path(tmp) / "calls.jsonl"
            self._install(bindir)
            full = {
                "PATH": f"{bindir}{os.pathsep}/usr/bin{os.pathsep}/bin",
                "HOME": tmp,
                "PREFLIGHT_LOG": str(log),
                "PREFLIGHT_WORKFLOW_TOKEN": "workflow-token-must-not-be-used",
            }
            full.update(env)
            proc = subprocess.run(
                ["bash", "-c", script],
                env=full,
                capture_output=True,
                text=True,
                check=False,
            )
            proc.calls = log.read_text(encoding="utf-8") if log.exists() else ""  # type: ignore[attr-defined]
            return proc

    def _calls(self, proc: subprocess.CompletedProcess, program: str) -> list[dict]:
        rows = []
        for line in getattr(proc, "calls", "").splitlines():
            row = json.loads(line)
            if row.get("program") == program:
                rows.append(row)
        return rows

    def _install(self, bindir: Path) -> None:
        (bindir / "gh").write_text(textwrap.dedent("""\
            #!/usr/bin/env python3
            import json, os, sys
            args = sys.argv[1:]
            joined = " ".join(args)
            if len(args) > 1 and args[1] == "user":
                kind, spec = "user", json.loads(os.environ["PREFLIGHT_GH_USER"])
                path = "user"
            elif "collaborators" in joined:
                kind, spec = "permission", json.loads(os.environ["PREFLIGHT_GH_PERMISSION"])
                path = args[1]
            elif "protection" in joined:
                kind, spec = "protection", json.loads(os.environ["PREFLIGHT_GH_PROTECTION"])
                path = args[1]
            else:
                kind, spec, path = "other", ["1", ""], ""
            token = os.environ.get("GH_TOKEN", "")
            if token == os.environ.get("REPORT_PUSH_TOKEN") and token:
                which = "push"
            elif token == os.environ.get("PREFLIGHT_WORKFLOW_TOKEN"):
                which = "workflow"
            else:
                which = "other"
            row = {"program": "gh", "kind": kind, "path": path, "token": which, "args": args}
            with open(os.environ["PREFLIGHT_LOG"], "a", encoding="utf-8") as fh:
                fh.write(json.dumps(row) + "\\n")
            code, out = int(spec[0]), spec[1]
            if out:
                sys.stdout.write(out if out.endswith("\\n") else out + "\\n")
            raise SystemExit(code)
            """), encoding="utf-8")
        (bindir / "curl").write_text(textwrap.dedent(f"""\
            #!/usr/bin/env python3
            import json, os, sys
            args = sys.argv[1:]
            def value(flag):
                for i, arg in enumerate(args):
                    if arg == flag and i + 1 < len(args):
                        return args[i + 1]
                return ""
            method = value("--request")
            url = value("--url")
            headers = [args[i + 1] for i, arg in enumerate(args) if arg == "--header" and i + 1 < len(args)]
            auth = any(item.startswith("Authorization: Bearer ") and len(item) > len("Authorization: Bearer ") for item in headers)
            discarded = "--output" in args and value("--output") == "/dev/null"
            row = {{
                "program": "curl",
                "method": method,
                "url": url,
                "auth": auth,
                "discarded": discarded,
                "count": args.count("--url"),
            }}
            with open(os.environ["PREFLIGHT_LOG"], "a", encoding="utf-8") as fh:
                fh.write(json.dumps(row) + "\\n")
            code = int(os.environ.get("PREFLIGHT_CURL_EXIT", "0"))
            if code:
                raise SystemExit(code)
            if not discarded:
                sys.stdout.write("PRIVATE-DEPOSITION\\n")
            sys.stdout.write(os.environ.get("PREFLIGHT_CURL_STATUS", "200"))
            raise SystemExit(0)
            """), encoding="utf-8")
        for name in ("gh", "curl"):
            path = bindir / name
            path.chmod(path.stat().st_mode | stat.S_IEXEC)


if __name__ == "__main__":
    unittest.main()
