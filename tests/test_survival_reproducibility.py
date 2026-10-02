#!/usr/bin/env python3
import importlib.util, json, tempfile, unittest
from pathlib import Path

SCRIPT=Path(__file__).resolve().parents[1]/"scripts"/"survival_reproducibility.py"
spec=importlib.util.spec_from_file_location("survival_reproducibility",SCRIPT)
mod=importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)

HEAD="0123456789abcdef0123456789abcdef01234567"

class ReproducibilityTests(unittest.TestCase):
    def fixture(self):
        td=tempfile.TemporaryDirectory(); root=Path(td.name)
        (root/"repos").mkdir()
        audit={"repository":{"head":HEAD,"origin":"https://example.invalid/o/r"}}
        (root/"repos"/"o__r.json").write_text(json.dumps(audit),encoding="utf-8")
        report={"id":"test/01","date":"2026-01-01","tool":{"name":"causari","version":"test"},
                "method":{"version":"v3","command":"re audit <owner/repo> --json"},
                "repositories":[{"repo":"o/r","audit_file":"repos/o__r.json"}],"not_aggregated":[]}
        (root/"report.json").write_text(json.dumps(report),encoding="utf-8")
        return td,root

    def test_deterministic_and_bound_to_head(self):
        td,root=self.fixture()
        try:
            a=mod.canonical_bytes(mod.build(root)); b=mod.canonical_bytes(mod.build(root))
            self.assertEqual(a,b)
            m=json.loads(a)
            self.assertEqual(m["repositories"][0]["head"],HEAD)
            self.assertIn(HEAD,m["repositories"][0]["reproduce"])
        finally: td.cleanup()

    def test_tamper_changes_receipt(self):
        td,root=self.fixture()
        try:
            before=mod.canonical_bytes(mod.build(root))
            p=root/"repos"/"o__r.json"; data=json.loads(p.read_text()); data["tampered"]=True
            p.write_text(json.dumps(data),encoding="utf-8")
            after=mod.canonical_bytes(mod.build(root))
            self.assertNotEqual(before,after)
            self.assertNotEqual(json.loads(before)["repositories"][0]["audit_sha256"],
                                json.loads(after)["repositories"][0]["audit_sha256"])
        finally: td.cleanup()

    def test_build_does_not_modify_inputs(self):
        td,root=self.fixture()
        try:
            paths=[root/"report.json",root/"repos"/"o__r.json"]
            before={p:p.read_bytes() for p in paths}
            mod.build(root)
            self.assertEqual(before,{p:p.read_bytes() for p in paths})
            self.assertFalse((root/"reproducibility.json").exists())
        finally: td.cleanup()

if __name__=="__main__": unittest.main()
