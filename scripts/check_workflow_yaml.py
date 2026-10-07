#!/usr/bin/env python3
"""Fail if a workflow file is not YAML that GitHub Actions can treat as a workflow.

The lint job is rustfmt and clippy. Neither reads .github/workflows, so a
file GitHub rejects at parse time still left that check green. This script
is the missing parse: every workflow must load, name its jobs, and carry a
trigger. PyYAML follows YAML 1.1 here, including the boolean key `on`.
"""
from __future__ import annotations

import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github" / "workflows"


def trigger_key(data: dict) -> bool:
    return "on" in data or True in data


def main() -> int:
    files = sorted(WORKFLOWS.glob("*.yml")) + sorted(WORKFLOWS.glob("*.yaml"))
    if not files:
        print("workflow yaml: no files found", file=sys.stderr)
        return 1
    for path in files:
        try:
            data = yaml.safe_load(path.read_text())
        except yaml.YAMLError as exc:
            print(f"workflow yaml: {path.relative_to(ROOT)} does not parse: {exc}", file=sys.stderr)
            return 1
        if not isinstance(data, dict) or not trigger_key(data):
            print(f"workflow yaml: {path.name} has no workflow trigger", file=sys.stderr)
            return 1
        jobs = data.get("jobs")
        if not isinstance(jobs, dict) or not jobs:
            print(f"workflow yaml: {path.name} has no jobs", file=sys.stderr)
            return 1
        for name, job in jobs.items():
            if not isinstance(job, dict):
                print(f"workflow yaml: {path.name} job {name} is not a mapping", file=sys.stderr)
                return 1
            if "runs-on" not in job and "uses" not in job:
                print(f"workflow yaml: {path.name} job {name} has neither runs-on nor uses", file=sys.stderr)
                return 1
    print(f"workflow yaml: {len(files)} files parse")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
