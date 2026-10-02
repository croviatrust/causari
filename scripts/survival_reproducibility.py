#!/usr/bin/env python3
"""Build and verify a reproducibility manifest for a Causari Survival Report.

This helper is intentionally side-effect free: it only reads a report directory
and writes a manifest when --write is explicitly supplied. It does not run
re audit, alter report.json, or modify historical report bytes.
"""
from __future__ import annotations
import argparse, hashlib, json
from pathlib import Path

SCHEMA = "causari.survival.reproducibility.v1"

def sha256(path: Path) -> str:
    h=hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda:f.read(1024*1024), b""): h.update(chunk)
    return h.hexdigest()

def build(report_dir: Path) -> dict:
    report=report_dir/"report.json"
    if not report.is_file(): raise SystemExit(f"missing {report}")
    data=json.loads(report.read_text())
    files=[]
    for p in sorted(x for x in report_dir.rglob("*") if x.is_file() and x.name!="reproducibility.json"):
        files.append({"path":p.relative_to(report_dir).as_posix(),"sha256":sha256(p),"bytes":p.stat().st_size})
    return {
      "schema":SCHEMA,
      "report_id":data.get("id"),
      "report_date":data.get("date"),
      "method":data.get("method"),
      "tool":data.get("tool","causari"),
      "tool_version":data.get("tool_version"),
      "reproduce":{"command":"re audit <owner/repo> --json","note":"Checkout the repository commit recorded by the report before rerunning the audit."},
      "files":files,
    }

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("report_dir",type=Path)
    ap.add_argument("--write",action="store_true")
    ap.add_argument("--verify",action="store_true")
    a=ap.parse_args()
    got=build(a.report_dir)
    out=a.report_dir/"reproducibility.json"
    if a.verify:
        if not out.is_file(): raise SystemExit("reproducibility.json missing")
        expected=json.loads(out.read_text())
        if expected != got: raise SystemExit("reproducibility manifest mismatch")
        print("reproducibility manifest: verified")
    elif a.write:
        out.write_text(json.dumps(got,indent=2,sort_keys=True)+"\n")
        print(out)
    else:
        print(json.dumps(got,indent=2,sort_keys=True))
if __name__=="__main__": main()
