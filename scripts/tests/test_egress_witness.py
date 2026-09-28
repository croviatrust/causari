#!/usr/bin/env python3
"""Tests for scripts/egress_witness.py: a witness in a thread, a local TCP
"destination", one client per case. Standard library only.

    python3 -m unittest scripts/tests/test_egress_witness.py
"""

from __future__ import annotations

import json
import socket
import socketserver
import sys
import tempfile
import threading
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import egress_witness as ew  # noqa: E402


class Echo(socketserver.BaseRequestHandler):
    """Reads one message, answers it upper-cased, closes."""

    def handle(self) -> None:
        data = self.request.recv(1024)
        if data:
            self.request.sendall(data.upper())


class Destination(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def http_read(sock: socket.socket) -> bytes:
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            break
        buf += chunk
    return buf


class WitnessCase(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.log_path = Path(self.tmp.name) / "reach.jsonl"
        self.dest = Destination(("127.0.0.1", 0), Echo)
        self.dest_port = self.dest.server_address[1]
        threading.Thread(target=self.dest.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True).start()
        self.witness: ew.Witness | None = None

    def tearDown(self) -> None:
        if self.witness is not None:
            self.witness.shutdown()
            self.witness.server_close()
        self.dest.shutdown()
        self.dest.server_close()
        self.tmp.cleanup()

    def start(self, policy: ew.Policy | None, mode: str = "enforce") -> int:
        self.witness = ew.Witness(("127.0.0.1", 0), ew.ReachLog(self.log_path), policy, mode, idle=2.0, quiet=True)
        threading.Thread(target=self.witness.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True).start()
        return self.witness.server_address[1]

    def lines(self) -> list[dict]:
        return [json.loads(l) for l in self.log_path.read_text(encoding="utf-8").splitlines() if l.strip()]

    def connect(self, port: int, host: str, dest_port: int, method: str = "CONNECT") -> tuple[socket.socket, bytes]:
        c = socket.create_connection(("127.0.0.1", port), timeout=5)
        if method == "CONNECT":
            c.sendall(f"CONNECT {host}:{dest_port} HTTP/1.1\r\nHost: {host}:{dest_port}\r\n\r\n".encode())
        else:
            c.sendall(f"GET http://{host}:{dest_port}/x HTTP/1.1\r\nHost: {host}\r\n\r\n".encode())
        return c, http_read(c)

    # -- cases -----------------------------------------------------------------

    def test_a_tunnel_within_the_policy_is_relayed_and_recorded_with_its_bytes(self) -> None:
        port = self.start(ew.Policy([f"127.0.0.1:{self.dest_port}"]))
        c, head = self.connect(port, "127.0.0.1", self.dest_port)
        self.assertTrue(head.startswith(b"HTTP/1.1 200 Connection Established"), head)
        c.sendall(b"hello witness")
        self.assertEqual(c.recv(1024), b"HELLO WITNESS")
        c.close()
        # The destination closes after its one answer; the witness writes the line
        # when the tunnel ends.
        for _ in range(100):
            if self.lines():
                break
            threading.Event().wait(0.05)
        (line,) = self.lines()
        self.assertEqual(line["host"], "127.0.0.1")
        self.assertEqual(line["port"], self.dest_port)
        self.assertEqual(line["outcome"], "allowed")
        self.assertEqual(line["bytes_out"], len(b"hello witness"))
        self.assertEqual(line["bytes_in"], len(b"HELLO WITNESS"))
        self.assertEqual(line["ip"], "127.0.0.1")
        self.assertRegex(line["at"], r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
        self.assertEqual(set(line), {"at", "host", "port", "outcome", "bytes_out", "bytes_in", "ip"})

    def test_a_destination_outside_the_policy_gets_403_and_is_recorded_blocked_without_connecting(self) -> None:
        port = self.start(ew.Policy(["github.com:443"]))
        c, head = self.connect(port, "Example.ORG", 443)
        self.assertTrue(head.startswith(b"HTTP/1.1 403 Forbidden"), head)
        self.assertIn(b"outside the egress policy", head)
        c.close()
        (line,) = self.lines()
        self.assertEqual((line["host"], line["port"], line["outcome"]), ("example.org", 443, "blocked"))
        self.assertEqual((line["bytes_out"], line["bytes_in"]), (0, 0))
        self.assertNotIn("ip", line)

    def test_observe_mode_relays_outside_the_policy_and_leaves_the_verdict_to_the_verifier(self) -> None:
        port = self.start(ew.Policy(["github.com:443"]), mode="observe")
        c, head = self.connect(port, "127.0.0.1", self.dest_port)
        self.assertTrue(head.startswith(b"HTTP/1.1 200"), head)
        c.sendall(b"x")
        self.assertEqual(c.recv(10), b"X")
        c.close()
        for _ in range(100):
            if self.lines():
                break
            threading.Event().wait(0.05)
        (line,) = self.lines()
        self.assertEqual(line["outcome"], "allowed")

    def test_without_a_policy_the_mode_is_observe_and_everything_is_recorded(self) -> None:
        port = self.start(None, mode="enforce")
        self.assertEqual(self.witness.mode, "observe")
        c, head = self.connect(port, "127.0.0.1", self.dest_port)
        self.assertTrue(head.startswith(b"HTTP/1.1 200"), head)
        c.close()

    def test_a_destination_that_refuses_the_connection_is_recorded_failed(self) -> None:
        spare = socket.socket()
        spare.bind(("127.0.0.1", 0))
        closed_port = spare.getsockname()[1]
        spare.close()
        port = self.start(ew.Policy([f"127.0.0.1:{closed_port}"]))
        c, head = self.connect(port, "127.0.0.1", closed_port)
        self.assertTrue(head.startswith(b"HTTP/1.1 502 Bad Gateway"), head)
        c.close()
        (line,) = self.lines()
        self.assertEqual(line["outcome"], "failed")

    def test_a_plain_proxy_request_is_not_relayed_and_is_recorded_blocked_under_enforce(self) -> None:
        port = self.start(ew.Policy(["github.com:443"]))
        c, head = self.connect(port, "example.org", 80, method="GET")
        self.assertTrue(head.startswith(b"HTTP/1.1 405"), head)
        c.close()
        (line,) = self.lines()
        self.assertEqual((line["host"], line["port"], line["outcome"]), ("example.org", 80, "blocked"))

    def test_a_malformed_connect_is_refused_and_not_recorded(self) -> None:
        port = self.start(ew.Policy(["github.com:443"]))
        c = socket.create_connection(("127.0.0.1", port), timeout=5)
        c.sendall(b"CONNECT github.com HTTP/1.1\r\n\r\n")
        head = http_read(c)
        self.assertTrue(head.startswith(b"HTTP/1.1 400"), head)
        c.close()
        self.assertEqual(self.lines(), [])


class RuleCase(unittest.TestCase):
    def test_rules_match_like_the_reference(self) -> None:
        self.assertTrue(ew.rule_matches("github.com", "GitHub.com", 443))
        self.assertTrue(ew.rule_matches("github.com:443", "github.com", 443))
        self.assertFalse(ew.rule_matches("github.com:443", "github.com", 80))
        self.assertTrue(ew.rule_matches("*.github.com", "api.github.com", 443))
        self.assertTrue(ew.rule_matches("*.github.com", "a.b.github.com", 443))
        self.assertFalse(ew.rule_matches("*.github.com", "github.com", 443))
        self.assertFalse(ew.rule_matches("*.github.com", "evilgithub.com", 443))

    def test_the_policy_document_is_checked(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "policy.json"
            p.write_text(json.dumps({"version": "crovia.pnx.policy.v1", "allow": ["github.com:443"]}))
            self.assertEqual(ew.Policy.load(p).allow, ["github.com:443"])
            self.assertTrue(ew.Policy.load(p).allows("github.com", 443))
            p.write_text(json.dumps({"version": "other", "allow": []}))
            with self.assertRaises(SystemExit):
                ew.Policy.load(p)
            p.write_text(json.dumps({"version": "crovia.pnx.policy.v1", "allow": [""]}))
            with self.assertRaises(SystemExit):
                ew.Policy.load(p)

    def test_authorities(self) -> None:
        self.assertEqual(ew.parse_authority("GitHub.com:443"), ("github.com", 443))
        self.assertEqual(ew.parse_authority("[::1]:8443"), ("::1", 8443))
        self.assertIsNone(ew.parse_authority("github.com"))
        self.assertIsNone(ew.parse_authority("github.com:0"))
        self.assertEqual(ew.parse_plain_target(["GET", "http://Example.org/x", "HTTP/1.1"]), ("example.org", 80))
        self.assertEqual(ew.parse_plain_target(["GET", "https://example.org:8443/x"]), ("example.org", 8443))
        self.assertIsNone(ew.parse_plain_target(["GET", "/x", "HTTP/1.1"]))


if __name__ == "__main__":
    unittest.main()
