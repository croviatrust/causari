# Contributing to Causari

Thanks for considering a contribution. Causari is a small, opinionated codebase
and we want to keep it that way — please read this first.

## Ground rules

- **Causari is licensed under Apache-2.0** (see [`LICENSE`](LICENSE) and
  [`NOTICE`](NOTICE)) — free for any use, personal or commercial, at any
  scale. "Causari" is a trademark of Croviatrust.
- **All contributions require a signed CLA** ([`CLA.md`](CLA.md)). The first
  PR you open will be commented automatically with the CLA bot — confirm and
  you only sign once.
- **English only** in code, comments, commits, issues and PRs.

## Quick start

```bash
git clone https://github.com/croviatrust/causari.git
cd causari
git config core.hooksPath .githooks   # enable the pre-push CI gate
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

The `git config core.hooksPath .githooks` line wires up
[`.githooks/pre-push`](.githooks/pre-push), which runs the **exact** commands
CI's `lint` job runs (`cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`) and blocks the push
if any fail. This catches lint failures locally instead of in CI. Emergency
bypass: `git push --no-verify`.

End-to-end demos:

```bash
# Linux / macOS
./scripts/demo.sh
./scripts/demo-trace.sh
./scripts/demo-bidir.sh
./scripts/demo-mcp.sh

# Windows
pwsh scripts\demo.ps1
pwsh scripts\demo-trace.ps1
pwsh scripts\demo-bidir.ps1
pwsh scripts\demo-mcp.ps1
```

## What we accept

- Bug fixes with a regression test.
- New CLI subcommands that fit the **causal-provenance** thesis (don't bolt
  on unrelated features — open an issue first).
- New MCP tools that expose existing functionality to agent runtimes.
- Documentation, examples, demo scripts.
- Performance work backed by a benchmark.

## What we are unlikely to merge

- Features that compete with or duplicate `git` (we are *not* a VCS).
- Cloud / SaaS / web-UI code in this repo (lives elsewhere).
- Refactors with no behavioural justification.
- New dependencies without a clear win.

## PR checklist

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] All four demo scripts still pass on your platform.
- [ ] If you touched the event schema, you bumped `causari.event.vX.Y` and
      kept backward decode.
- [ ] If you touched MCP tools, you regenerated the install snippet and
      tested with at least one runtime (Claude Desktop, Cursor, Cline,
      Windsurf).
- [ ] You signed the CLA.

## Reporting security issues

Do **not** open a public issue. Email `security@croviatrust.com` with
`[causari]` in the subject. We aim to acknowledge within 72 h.

## Code of conduct

Be civil. Disagree on the merits. We will remove people who can't.
