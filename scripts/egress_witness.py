#!/usr/bin/env python3
"""A CONNECT proxy that writes down where a job connected — the capture side
of a PNX reach record (crovia.pnx.reach.v1, PNX draft 0.4 §4a).

    python3 scripts/egress_witness.py --listen 127.0.0.1:3128 \\
        --policy .github/egress-policy.json --log /tmp/run/reach.jsonl &
    export https_proxy=http://127.0.0.1:3128 http_proxy=http://127.0.0.1:3128
    ... the measured work ...
    kill -TERM %1; wait

Every client CONNECT to `host:port` becomes one line of the log,
`{"at", "host", "port", "outcome", "bytes_out", "bytes_in", "ip"}`, in the
format `tacet-pnx witness --reach LOG --policy FILE` turns into the signed
reach record of a run sheet. The policy is a `crovia.pnx.policy.v1`
document (an allowlist of `host` or `host:port` rules; `*.` matches one or
more labels, never the apex). Under `enforce` a destination no rule allows
is answered `403` and recorded as `blocked` — nothing is sent to it; under
`observe` everything is relayed and recorded, and the verifier judges.
Without a policy every destination is recorded and none is judged.

The witness sees tunnels, not bodies (TLS ends at the destination), so the
record it feeds says where the job connected and how many bytes crossed,
nothing about their content. Traffic that does not go through the proxy is
outside the record; the workflow sets `https_proxy` for the steps it wants
covered, and a destination that refuses tunnels shows up as `failed`.

Plain (non-CONNECT) proxy requests are not relayed: under `enforce` they are
recorded as `blocked` at port 80 (no rule of a policy for a TLS-only job
allows them), otherwise as `failed`; the client gets `405`.

Standard library only. Ports to any CI runner that has Python 3.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import select
import signal
import socket
import socketserver
import sys
import threading
from pathlib import Path
from typing import Any

POLICY_VERSION = "crovia.pnx.policy.v1"
IDLE_TIMEOUT_S = 300.0
CONNECT_TIMEOUT_S = 20.0


# ----------------------------------------------------------------------------- policy

def split_rule(rule: str) -> tuple[str, int | None]:
    host, sep, port = rule.rpartition(":")
    if sep and port.isdigit():
        return host.lower(), int(port)
    return rule.lower(), None


def rule_matches(rule: str, host: str, port: int) -> bool:
    """Same semantics as tacet.reach.rule_matches: `host` or `host:port`;
    `*.` matches one or more labels, never the apex; case-insensitive."""
    rhost, rport = split_rule(rule)
    host = host.lower()
    if rport is not None and rport != port:
        return False
    if rhost.startswith("*."):
        suffix = rhost[1:]
        return host.endswith(suffix) and len(host) > len(suffix)
    return host == rhost


class Policy:
    def __init__(self, allow: list[str]) -> None:
        self.allow = list(allow)

    @classmethod
    def load(cls, path: Path) -> Policy:
        doc = json.loads(path.read_text(encoding="utf-8"))
        if doc.get("version") != POLICY_VERSION:
            raise SystemExit(f"{path}: unknown policy version {doc.get('version')!r}")
        allow = doc.get("allow")
        if not isinstance(allow, list) or not all(isinstance(r, str) and r for r in allow):
            raise SystemExit(f"{path}: policy.allow must be a list of non-empty strings")
        return cls(allow)

    def allows(self, host: str, port: int) -> bool:
        return any(rule_matches(r, host, port) for r in self.allow)


# ----------------------------------------------------------------------------- log

class ReachLog:
    """Appends one attempt per line; one writer lock for all tunnel threads."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.lock = threading.Lock()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.touch()

    def write(self, host: str, port: int, outcome: str, *, bytes_out: int = 0, bytes_in: int = 0,
              ip: str | None = None, at: str | None = None) -> dict[str, Any]:
        line: dict[str, Any] = {
            "at": at or now(), "host": host.lower(), "port": port, "outcome": outcome,
            "bytes_out": bytes_out, "bytes_in": bytes_in,
        }
        if ip:
            line["ip"] = ip
        with self.lock, self.path.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(line, separators=(",", ":")) + "\n")
        return line


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# ----------------------------------------------------------------------------- proxy

def parse_authority(authority: str) -> tuple[str, int] | None:
    """`host:port` of a CONNECT request line; IPv6 in brackets."""
    authority = authority.strip()
    if authority.startswith("["):
        host, _, rest = authority[1:].partition("]")
        port = rest[1:] if rest.startswith(":") else ""
    else:
        host, _, port = authority.rpartition(":")
        if not _:
            return None
    if not host or not port.isdigit() or not 0 < int(port) < 65536:
        return None
    return host.lower(), int(port)


def relay(a: socket.socket, b: socket.socket, idle: float) -> tuple[int, int]:
    """Copy bytes both ways until either side closes or `idle` seconds pass
    with nothing to copy. Returns (bytes a→b, bytes b→a)."""
    out_bytes = in_bytes = 0
    open_sides = {a, b}
    while len(open_sides) == 2:
        ready, _, _ = select.select([a, b], [], [], idle)
        if not ready:
            break
        for s in ready:
            try:
                data = s.recv(65536)
            except OSError:
                data = b""
            if not data:
                open_sides.discard(s)
                continue
            dest = b if s is a else a
            try:
                dest.sendall(data)
            except OSError:
                open_sides.discard(dest)
                continue
            if s is a:
                out_bytes += len(data)
            else:
                in_bytes += len(data)
    return out_bytes, in_bytes


class Handler(socketserver.BaseRequestHandler):
    server: "Witness"

    def handle(self) -> None:  # noqa: C901 - one request, several exits
        client: socket.socket = self.request
        client.settimeout(CONNECT_TIMEOUT_S)
        try:
            head = read_head(client)
        except (OSError, ValueError):
            return
        if head is None:
            return
        request_line = head.split(b"\r\n", 1)[0].decode("latin-1", "replace")
        parts = request_line.split()
        witness = self.server
        if len(parts) < 2 or parts[0].upper() != "CONNECT":
            target = parse_plain_target(parts)
            if target is not None:
                host, port = target
                outcome = "blocked" if witness.mode == "enforce" and witness.policy is not None else "failed"
                witness.log.write(host, port, outcome)
            respond(client, 405, "Method Not Allowed", "egress witness: CONNECT only; plain proxy requests are not relayed")
            return
        target = parse_authority(parts[1])
        if target is None:
            respond(client, 400, "Bad Request", "egress witness: malformed CONNECT authority")
            return
        host, port = target
        if witness.policy is not None and witness.mode == "enforce" and not witness.policy.allows(host, port):
            witness.log.write(host, port, "blocked")
            witness.say(f"blocked  {host}:{port}")
            respond(client, 403, "Forbidden", f"egress witness: {host}:{port} is outside the egress policy; not connected")
            return
        at = now()
        try:
            upstream = socket.create_connection((host, port), timeout=CONNECT_TIMEOUT_S)
        except OSError as e:
            witness.log.write(host, port, "failed", at=at)
            witness.say(f"failed   {host}:{port} ({e})")
            respond(client, 502, "Bad Gateway", f"egress witness: cannot connect to {host}:{port}: {e}")
            return
        ip = None
        try:
            ip = upstream.getpeername()[0]
        except OSError:
            pass
        try:
            client.sendall(b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: causari-egress-witness\r\n\r\n")
            client.settimeout(None)
            upstream.settimeout(None)
            out_bytes, in_bytes = relay(client, upstream, witness.idle)
        finally:
            upstream.close()
        witness.log.write(host, port, "allowed", bytes_out=out_bytes, bytes_in=in_bytes, ip=ip, at=at)
        witness.say(f"allowed  {host}:{port} → {out_bytes} out, {in_bytes} in")


def parse_plain_target(parts: list[str]) -> tuple[str, int] | None:
    """`host:port` of an absolute-URI plain request (`GET http://h/x`)."""
    if len(parts) < 2 or "://" not in parts[1]:
        return None
    scheme, _, rest = parts[1].partition("://")
    authority = rest.split("/", 1)[0]
    default = 443 if scheme.lower() == "https" else 80
    parsed = parse_authority(authority) if ":" in authority.rsplit("]", 1)[-1] else (authority.lower(), default)
    if parsed is None or not parsed[0]:
        return None
    return parsed


def read_head(sock: socket.socket) -> bytes | None:
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            return None
        buf += chunk
        if len(buf) > 65536:
            raise ValueError("request head too large")
    return buf


def respond(sock: socket.socket, code: int, reason: str, text: str) -> None:
    body = (text + "\n").encode()
    try:
        sock.sendall(f"HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {len(body)}\r\n"
                     f"Connection: close\r\n\r\n".encode() + body)
    except OSError:
        pass
    finally:
        try:
            sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        sock.close()


class Witness(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True

    def __init__(self, addr: tuple[str, int], log: ReachLog, policy: Policy | None, mode: str,
                 idle: float = IDLE_TIMEOUT_S, quiet: bool = False) -> None:
        super().__init__(addr, Handler)
        self.log = log
        self.policy = policy
        self.mode = mode if policy is not None else "observe"
        self.idle = idle
        self.quiet = quiet

    def say(self, text: str) -> None:
        if not self.quiet:
            print(f"egress witness: {text}", file=sys.stderr, flush=True)


# ----------------------------------------------------------------------------- main

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--listen", default="127.0.0.1:3128", metavar="HOST:PORT")
    ap.add_argument("--log", required=True, type=Path, metavar="FILE", help="reach log to append to (.jsonl)")
    ap.add_argument("--policy", type=Path, metavar="FILE", help="crovia.pnx.policy.v1 document")
    ap.add_argument("--mode", choices=["enforce", "observe"], default="enforce",
                    help="with --policy: refuse destinations outside it (enforce) or relay and record (observe)")
    ap.add_argument("--idle-timeout", type=float, default=IDLE_TIMEOUT_S, metavar="S", help="close a silent tunnel after S seconds")
    ap.add_argument("--quiet", action="store_true")
    a = ap.parse_args(argv)
    host, _, port = a.listen.rpartition(":")
    if not host or not port.isdigit():
        raise SystemExit("--listen must be HOST:PORT")
    policy = Policy.load(a.policy) if a.policy else None
    server = Witness((host, int(port)), ReachLog(a.log), policy, a.mode, a.idle_timeout, a.quiet)
    stop = threading.Event()

    def on_signal(_signum: int, _frame: Any) -> None:
        stop.set()

    signal.signal(signal.SIGTERM, on_signal)
    signal.signal(signal.SIGINT, on_signal)
    bound = server.server_address
    server.say(f"listening on {bound[0]}:{bound[1]} · log {a.log} · "
               + (f"policy {len(policy.allow)} rule(s), {server.mode}" if policy else "no policy: recording only"))
    t = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.2}, daemon=True)
    t.start()
    while not stop.wait(0.2):
        pass
    server.shutdown()
    server.server_close()
    server.say("stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
