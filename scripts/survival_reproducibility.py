#!/usr/bin/env python3
"""Build/verify a deterministic reproduction receipt for a Survival Report.

Reads existing report artifacts only. It never runs an audit or rewrites them.
"""
from __future__ import annotations
import argparse, hashlib, json
from pathlib import Path

SCHEMA="causari.survival.reproducibility.v1"

def digest(path: Path) -> str:
    h=hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda:f.read(1024*1024),b""): h.update(chunk)
    return h.hexdigest()

def build(root: Path) -> dict:
    report_path=root/"report.json"
    report=json.loads(report_path.read_text(encoding="utf-8"))
    repos=[]
    for row in report.get("repositories",[])+report.get("not_aggregated",[]):
        rel=row.get("audit_file")
        if not rel: raise SystemExit(f"missing audit_file for {row.get('repo')}")
        p=root/rel
        if not p.is_file(): raise SystemExit(f"missing {p}")
        audit=json.loads(p.read_text(encoding="utf-8"))
        head=(audit.get("repository") or {}).get("head")
        if not isinstance(head,str) or len(head)!=40 or any(c not in "0123456789abcdefABCDEF" for c in head):
            raise SystemExit(f"missing/invalid repository.head in {rel}")
        repos.append({
          "repo":row["repo"],"head":head.lower(),"audit_file":rel,
          "audit_sha256":digest(p),
          "reproduce":f"git checkout {head.lower()} && re audit --json",
        })
    tool=report.get("tool") or {}
    method=report.get("method") or {}
    return {
      "schema":SCHEMA,
      "report":{"id":report.get("id"),"date":report.get("date"),"sha256":digest(report_path)},
      "tool":{"name":tool.get("name"),"version":tool.get("version")},
      "method":{"version":method.get("version"),"command":method.get("command")},
      "repositories":repos,
    }

def canonical_bytes(value: dict) -> bytes:
    return (json.dumps(value,sort_keys=True,separators=(",",":"),ensure_ascii=False)+"\n").encode()

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("report_dir",type=Path)
    ap.add_argument("--write",action="store_true")
    ap.add_argument("--verify",action="store_true")
    a=ap.parse_args()
    manifest=build(a.report_dir)
    payload=canonical_bytes(manifest)
    out=a.report_dir/"reproducibility.json"
    if a.verify:
        if not out.is_file(): raise SystemExit("reproducibility.json missing")
        if out.read_bytes()!=payload: raise SystemExit("reproducibility manifest mismatch")
        print("reproducibility manifest: verified")
    elif a.write:
        out.write_bytes(payload); print(f"{out} sha256={hashlib.sha256(payload).hexdigest()}")
    else:
        print(payload.decode(),end="")
if __name__=="__main__": main()
