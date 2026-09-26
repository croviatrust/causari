<h1 align="center">∵ causari</h1>

<p align="center"><strong>AI-written code has no author. It has causes. Causari records them.</strong></p>
<p align="center"><em>How many lines from AI-tagged commits are still alive in your repo? One command, any git repo, no setup. A count, not a grade.</em></p>

<p align="center">
  <a href="https://causari.dev"><strong>causari.dev</strong></a>
  &nbsp;·&nbsp;
  <a href="https://causari.dev/reports/survival/">Weekly Survival Report</a>
  &nbsp;·&nbsp;
  <a href="https://causari.dev/method">Method</a>
  &nbsp;·&nbsp;
  <a href="MANIFESTO.md">Manifesto</a>
  &nbsp;·&nbsp;
  <a href="ROADMAP.md">Roadmap</a>
  &nbsp;·&nbsp;
  <a href="https://github.com/croviatrust/causari/releases">Releases</a>
</p>

<p align="center">
  <img alt="CI" src="https://github.com/croviatrust/causari/actions/workflows/ci.yml/badge.svg?branch=main">
  <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-3b4252">
  <img alt="Platform" src="https://img.shields.io/badge/linux%20%7C%20macOS%20%7C%20windows-3b4252">
  <a href="https://causari.dev/r/croviatrust/causari/"><img alt="AI code survival, measured by this tool on its own repository" src="https://causari.dev/r/croviatrust/causari/badge.svg"></a>
</p>

---

```bash
curl -fsSL https://causari.dev/install.sh | sh     # Linux / macOS (Windows below)

re audit                    # the repo you are in
re audit vercel/next.js     # any public repo, cloned to a temp dir and removed after
```

```console
$ re audit
∵ causari · AI code survival
───────────────────────────────────────────────────
  216 commits analyzed (git metadata only, no setup required)

Verified AI-tagged: 14 commits, 6267 introduced, 5185 survived
  survival 82.7% line-weighted · 82.7% capped · median 92.8%
Probable AI-assisted: none detected
By agent (verified only)
  agent                commits introduced  survived  line-wt   capped   median
  cursor                    14       6267      5185    82.7%    82.7%    92.8%

Baseline: untagged lines of the same repository
  untagged: 202 commits, 95759 introduced, 76085 survived · 79.5% line-weighted · 92.1% median
    line age                AI-tagged                 untagged
      0-30 d       85.6% (13 commits)      82.7% (142 commits)
     30-90 d                        —       75.8% (23 commits)   (below floor on one side)
    90-180 d         73.9% (1 commit)       62.6% (37 commits)   (below floor on one side)
  age-matched: AI-tagged 85.6% vs untagged 82.7% of the same age → +2.9 points, over 1 window holding 75% of AI-tagged lines

Confidence notes
  · VERIFIED = explicit metadata (trailers, bot author, etc.)
  · PROBABLE = weak heuristic; may include human-assisted commits
  · UNKNOWN commits are excluded from headline numbers; they form
    the untagged baseline (human, inline-completed and untagged-agent code alike)
  · Only lines from AI-tagged commits are measured; inline completions
    (Copilot, Cursor Tab, …) leave no git trace and are invisible here
  · A measurement, not a grade: method v3 at https://causari.dev/method
```

Everyone argues about how much code AI writes. Nobody can check the numbers.
`re audit` reads plain git history — `Co-Authored-By` trailers, bot authors,
agent markers — finds the commits that carry machine-readable AI authorship,
and asks `git blame` how many of their lines are still at HEAD. No model, no
estimate, no survey. Anyone re-runs it and gets the same bytes.

- `--json` the exact bytes behind any published row
- `--summary` Markdown for CI; `--badge` / `--card` one-colour SVGs
- `--save` append a snapshot to track your own trend

**Compared with what**: since method v3 every audit puts the repository's
own untagged lines next to the AI-tagged ones, by line age, and states the
age-matched gap: AI-tagged survival against untagged survival of the same
age in the same repository. It also names the oldest line still at HEAD and
how many commits predate it: a repository that was cleared or rewritten
shows there, and its ratio is read accordingly.

**What it cannot see**: code from inline completions (Copilot, Cursor Tab,
Windsurf, …) leaves no git trace and counts as human, so it is in the
untagged baseline. Commits without a trailer are UNKNOWN. A formatter pass
or a moved function counts as a death under method v1. One bulk commit can
dominate a line-weighted ratio; ratios under 5 AI-tagged commits are
flagged. All of this is written out at
[causari.dev/method](https://causari.dev/method), with how to contest a number.
The questions people ask, answered with the command and the limit: [causari.dev/faq](https://causari.dev/faq); how this differs from `git blame`, vendor dashboards and churn reports: [causari.dev/compare](https://causari.dev/compare).

The weekly [Survival Report](https://causari.dev/reports/survival/) runs
`re audit` on up to 100 open-source repositories: a hand-picked list plus the
most-starred public repositories (at least 100 stars) where GitHub commit search
finds at least five commits carrying the same AI authorship metadata, selected
every week, the day before the report, by
[`scripts/survival_discover.py`](scripts/survival_discover.py) under a rule
stated on the [method page](https://causari.dev/method#selection); rows stay
alphabetical, and one line in `.github/survival-optout.txt` removes a repository.

## In CI: a count on every pull request

```yaml
# .github/workflows/causari.yml
on: pull_request
permissions: { contents: read, pull-requests: write }
jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }      # the audit reads every commit; shallow clones are wrong
      - uses: croviatrust/causari@v1
```

The [Action](https://github.com/marketplace/actions/causari-survival-audit)
downloads the prebuilt Linux binary (a few seconds), runs `re audit --summary`,
writes it to the job summary and posts one sticky comment per PR. No cloud,
no account. A [live example](https://github.com/croviatrust/causari-audit-demo/pull/1).

## The ledger: from "which commit" to "which prompt"

The audit works on any history. If you also want to know *why* a line exists
— the prompt, the model, the files the agent read — Causari records agent
actions as they happen into a local, append-only ledger (`.causari/`,
gitignored), with a snapshot of the tree before and after each one.

```bash
re init                       # create .causari/ (added to .gitignore)
re hook claude-code           # record every Claude Code prompt and edit, exactly
re hook cursor                # same for Cursor, via its hooks.json (prompt, edit, model)
re proxy                      # local LLM proxy: prompts, models, tokens, cost
re watch                      # attribute file changes to captured completions

re why    src/auth.ts:42      # which recorded event introduced this line
re trace  src/auth.ts:42      # upstream: events that fed into it through reads/writes
re impact <event-id>          # downstream: what later events depended on it
re lens   src/auth.ts         # the file annotated line by line with its event
re find   "the JWT refactor"  # search prompts, messages, reasoning
re bisect --test "npm test"   # first recorded event that breaks a test
re revert <id>                # restore the pre-state, with a preview of what else you undo
re fork / re sessions / re switch / re log --all / re diff a..b
```

### What each agent actually gives you today

Claims about "any agent" are cheap. This table is derived from the code and
is kept current; if a cell is wrong, open an issue.

| Agent | Prompt + file, exact | Model, tokens, cost | How |
|---|---|---|---|
| **Claude Code** | yes, via lifecycle hooks | not yet (edits travel as `tool_use`, which the proxy does not join to files yet) | `re hook claude-code` |
| **Aider** | heuristic join, measured | yes | `OPENAI_API_BASE` / `ANTHROPIC_API_BASE` → `re proxy` + `re watch` |
| **Codex CLI**, OpenAI Agents SDK | not yet (Responses API output not parsed) | yes | `OPENAI_BASE_URL` → `re proxy` |
| **Cursor** | yes, via native hooks | model yes (the hook names it); tokens and cost no (Cursor's model calls do not pass through `re proxy`) | `re hook cursor` |
| **Windsurf**, **Copilot** | only what the agent self-reports via MCP | no | `re mcp` |
| **Cline / Roo**, custom scripts, curl | heuristic join when the completion carries the code as text | yes | base URL → `re proxy` + `re watch` |

Two evidence classes, and every output says which one it is:

- **Declared** (hooks, MCP, `re record`): the agent stated what it did. Exact
  prompt and path. If a human edits a file between two hook events, the hook
  snapshot absorbs that edit into the next agent event — a known limit being
  fixed in [Phase 1](ROADMAP.md).
- **Correlated** (proxy + watch): the lines you inserted are searched inside
  completions captured moments before. A score, not a fact. The adversarial
  harness in [`examples/real-session/`](examples/real-session/RESULTS.md)
  gives measured numbers: 100 % on a clean write, 50 % after a formatter pass,
  wrong per-line attribution when two prompts touch one file in the same
  window.

Everything stays on your machine. `re proxy` and the hooks store prompts,
completions and commands in clear under `.causari/` (gitignored); credentials
in recognisable formats — `sk-…` keys, GitHub/GitLab/Slack/npm/PyPI/Hugging
Face tokens, AWS and Google keys, bearer values, JWTs, PEM private keys — are
replaced by `[redacted:<kind>]` before writing, and the record says how many.
Anything else you paste is kept as typed. Snapshots store every non-ignored
file (`.env*`, `node_modules`, `target`, `dist`, `build`, `.git` and a few
others are excluded by default). Treat `.causari/` as sensitive. What is
stored, what is not, and what the tool does and does not defend against:
[`SECURITY.md`](SECURITY.md) and [`docs/threat-model.md`](docs/threat-model.md).

## Receipts you can verify without us

**Crovia Seals.** `re proxy --seal` issues a
[crovia.seal.v1](https://github.com/croviatrust/crovia-seal) receipt for every
completion: Ed25519-signed, hash-chained, committing to SHA-256 hashes of the
exact request and response bytes (content never leaves the machine). Each
recorded exchange carries its `seal_id` and the same hashes, so a receipt can
be matched to the completion it covers. The implementation passes the
reference conformance vectors; seals verify under the Python reference
implementation and vice versa.

```bash
re proxy --seal          # issue a receipt per completion
re seal verify           # every signature, whole chain, offline
re seal issuer           # your issuer id and public key (read-only)
```

**PNX — Proof of Non-Exfiltration.** `re proxy --pnx` makes the proxy an
egress witness for the TACET profile
[`crovia.pnx.v1`](https://croviatrust.com/registry/tacet/pnx/): every request
body is fingerprinted (salted winnowing, k-gram 32, window 16) and committed
to a sparse Merkle map *before* it is forwarded; Ctrl-C signs a run sheet
carrying the root. `re pnx prove` then shows, for a set of protected assets,
that none shared a substring of 47 bytes or more with that traffic — or
records which did. Sheet and proof contain no traffic bytes and no asset
bytes, and verify offline with `re pnx verify` or with the Python reference
`tacet-pnx`, in both directions, same verdicts and exit codes. What a proof
does and does not say: [`docs/pnx.md`](docs/pnx.md).

```bash
re proxy --pnx                                    # witness a session; Ctrl-C signs the sheet
re pnx prove --asset api_key=.env --assets-dir src/secret/
re pnx verify .causari/pnx/<run>/proof.json --asset api_key=.env --assets-dir src/secret/
```

**Audit seals.** `re audit --seal` writes the audit result as the same kind of
receipt: a `crovia.seal.v1` over the exact bytes of `re audit --json`, bound to
the audited commit and the method version, hash-chained with the proxy's
completion seals under one issuer key per repository. `re seal verify FILE`
checks it offline; so does the static page
[causari.dev/verify](https://causari.dev/verify), which makes no network
request. A valid seal proves that this key signed these numbers for this
commit and that they were not altered since. It does not prove the numbers
are true: rerun `re audit` on the commit and compare. (`re proof` is retired
in favour of this; it exits 2 and names the replacement.)

```bash
re audit --seal --output audit.seal.json
re seal verify audit.seal.json
```

## Experimental

These commands exist, work in the demos, and are not yet held to the standard
above. They are out of the proof and out of the front page until they are.

- `re skill distill / verify / export / import / pull / trust`: signed units
  of past work; the trust ladder (recorded → verified → proven) currently
  measures file existence and recall counts, not correctness.
- `re brief`: a Markdown briefing of past work for a model's context.
- `re guard`: substring rules over recent changes; gates a build only when
  asked (`--fail-on alert|warning`); `--json` for machines.
- `re churn`, `re report`: survival measured over the ledger instead of git,
  with cost extrapolated from a static price table; `re churn --json`, and
  `--fail-below <percent>` when a team wants a floor of its own choosing.
- `re mcp`: stdio MCP server with `causari_record`, `causari_recall`,
  `causari_why`; `re mcp --install` prints the client config.

## Install

```bash
# Linux / macOS
curl -fsSL https://causari.dev/install.sh | sh

# Windows (PowerShell)
irm https://causari.dev/install.ps1 | iex

# Homebrew (macOS, Linux)
brew install croviatrust/tap/causari

# Scoop (Windows)
scoop bucket add causari https://github.com/croviatrust/scoop-bucket && scoop install causari

# crates.io (Rust 1.85+)
cargo install causari --locked

# no install: launchers that fetch the verified binary on first run
npx causari audit
pipx run causari audit

# from source
cargo install --git https://github.com/croviatrust/causari --locked
```

One program under two names: `causari` is the binary, `re` is the short alias
every example uses. Both are in every archive and both are installed. One
static binary, about 5 MB, for Linux (x86_64, aarch64), macOS (x86_64, Apple
silicon) and Windows (x86_64), installed to `~/.local/bin` (or
`%LOCALAPPDATA%\Programs\causari`). The installer checks the archive's
SHA-256 against the `SHA256SUMS.txt` published with each release and refuses
to install on a mismatch. From v0.2.0, every archive and the sums file carry a
signed SLSA build-provenance attestation from the release workflow:

```bash
gh attestation verify causari-v0.3.0-x86_64-unknown-linux-gnu.tar.gz --repo croviatrust/causari
```

By hand:

```bash
VERSION=$(curl -fsSL https://api.github.com/repos/croviatrust/causari/releases/latest | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p')
TARGET=x86_64-unknown-linux-gnu
base="https://github.com/croviatrust/causari/releases/download/$VERSION"
curl -fsSLO "$base/causari-$VERSION-$TARGET.tar.gz" && curl -fsSLO "$base/SHA256SUMS.txt"
sha256sum --ignore-missing -c SHA256SUMS.txt && tar -xzf "causari-$VERSION-$TARGET.tar.gz" && install -m755 causari re ~/.local/bin/
```

The [Homebrew tap](https://github.com/croviatrust/homebrew-tap) and the
[Scoop bucket](https://github.com/croviatrust/scoop-bucket) render their
manifests from each release's `SHA256SUMS.txt` and re-render every six hours.
The crate is published from the release tag through crates.io Trusted
Publishing (`.github/workflows/publish-crate.yml`): no long-lived token exists.

### As an MCP server

`re mcp` speaks MCP over stdio and exposes `causari_record`, `causari_recall`
and `causari_why`; `re mcp --install` prints the configuration block for
Claude Desktop, Cursor, Windsurf and Cline. The server is listed in the
[MCP Registry](https://registry.modelcontextprotocol.io) from
[`server.json`](server.json):

- MCP Registry name: mcp-name: io.github.croviatrust/causari
- One-click for Cursor: [Add causari to Cursor](https://cursor.com/en/install-mcp?name=causari&config=eyJjb21tYW5kIjoicmUiLCJhcmdzIjpbIm1jcCJdfQ%3D%3D)
  (registers `re mcp`; the binary must be on `PATH`)

### In Cursor

`re hook cursor` merges seven command hooks into the project's
`.cursor/hooks.json` (commit it: teammates and Cursor cloud agents run it
from the repository root); `--user` merges `~/.cursor/hooks.json` instead
for one machine, `--dry-run` prints the result. Hooks other people wrote in
the same file are kept. Each hook runs `re hook-event cursor:<event>`, so
`re` must be on `PATH` for the Cursor process (the same caveat as the Claude
Code hooks); without it, or in a project without `re init`, every hook
answers with a neutral JSON object and nothing is recorded. What lands in
the ledger: the prompt with its attachments and model
(`beforeSubmitPrompt`), a snapshot before each shell or write tool
(`preToolUse`), one event per written file (`afterFileEdit`) and per
command that changed the tree (`afterShellExecution`), the agent's answer
next to its prompt (`afterAgentResponse`), and the experience briefing as
context at `sessionStart`.

### As a Claude Code plugin

The repository is also a plugin marketplace. Inside Claude Code:

```
/plugin marketplace add croviatrust/causari
/plugin install causari@croviatrust
```

The plugin installs the four hooks that `re hook claude-code` writes by hand
(prompt, pre-state, post-state, session briefing), the MCP server, and a
skill that tells the model when `why`, `trace`, `recall` and `record` are the
right tool. It needs the `re` binary on `PATH`; without it, or in a project
without `re init`, every hook is a silent no-op.

Demos: `scripts/demo*.sh|ps1` (mock LLM included), `examples/real-session/`
(the adversarial harness), `scripts/recovery_lab.py` (revert/bisect stress
lab).

## How it works, in one paragraph

Every recorded event is a content-addressed object (BLAKE3) with the tree
before, the tree after, the agent, model and tool, the prompt, declared reads
and writes, tokens and cost, and a parent. Sessions are refs; forks are
implicit. `re why` finds the first event on the current chain whose
before/after diff inserted the line; `re trace` follows reads and writes
backwards from there; `re impact` forwards. Unchanged files share blobs
between snapshots. The full design, and its current limits, are in
[`docs/review-2026-09-20/`](docs/review-2026-09-20/).

## Where this is going

Causari does not compete with provenance trackers (Agent Trace, git-ai,
`Assisted-by:` trailers, Entire checkpoints); it reads them, measures with a
public method, and signs the result so a third party can verify it offline.
Next: `git blame -w -M -C` and per-commit caps in the audit; Agent Trace and
`Assisted-by:` readers; the audit result as a Seal. Done: a PNX witness mode
in the proxy that proves what an agent session did *not* send to the model.
Phases and exit criteria: [`ROADMAP.md`](ROADMAP.md).

## Family

Causari is part of [Crovia](https://croviatrust.com), one grammar in three
tenses: **TACET** proves that a model's public card carried no training-data
disclosure in the hours it was observed, **PNX** proves an agent's egress
carried no protected bytes, **Causari** records why a line of code exists and
measures whether it is still there. Same rules everywhere: reproducible
numbers, no verdicts, verification without our servers, limits stated first.

Role in the Crovia canon — Sibling product: proof of cause for AI-written code
(audit + local ledger); Seal issuer for agent completions and audit results.

## License

Apache-2.0 (see `LICENSE`). "Causari" is a trademark of Crovia Trust; the
license does not grant trademark rights (see `NOTICE`). Contributing: see
`CONTRIBUTING.md`.
