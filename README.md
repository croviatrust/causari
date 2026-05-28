<p align="center">
  <img src="assets/logo-readme.png" alt="Causari — intent-addressable code" width="520">
</p>

<h3 align="center">Trace intent. Debug causality.</h3>
<p align="center"><em>Intent-addressable code for AI agents.</em></p>

<p align="center">
  <a href="https://causari.dev"><strong>causari.dev</strong></a>
  &nbsp;·&nbsp;
  <a href="https://github.com/croviatrust/causari/releases">Releases</a>
  &nbsp;·&nbsp;
  <a href="https://github.com/croviatrust/causari/discussions">Discussions</a>
  &nbsp;·&nbsp;
  <a href="#mcp-server">MCP</a>
  &nbsp;·&nbsp;
  <a href="LICENSE">License (BSL 1.1)</a>
</p>

<p align="center">
  <img alt="CI" src="https://github.com/croviatrust/causari/actions/workflows/ci.yml/badge.svg?branch=main">
  <img alt="License" src="https://img.shields.io/badge/license-BSL%201.1-7c3aed">
  <img alt="Rust" src="https://img.shields.io/badge/rust-stable-orange">
  <img alt="Platform" src="https://img.shields.io/badge/platform-linux%20%7C%20macOS%20%7C%20windows-blue">
</p>

---

> *Causari* (Latin, deponent verb): *to plead a cause, to argue why.* Because
> every line of AI-generated code deserves to be defended, traced, and
> understood.

Causari records every action an AI agent takes on your codebase — not just
the bytes that changed, but the **prompt that asked**, the **model that
answered**, the **files it read**, and the **reasoning behind the change**.

You can then ask questions no version control system has ever answered:

```bash
re why    src/auth.ts:42      # who/what produced this exact line?
re trace  src/auth.ts:42      # full UPSTREAM causal cone: every event that
                              #   contributed transitively, through reads/writes
re impact <event-id>          # full DOWNSTREAM cone: what flowed from this action,
                              #   transitively (causality-aware blast radius)
re lens   src/auth.ts         # render a file with per-line provenance annotations
re find   "the JWT refactor"  # search every prompt, reasoning and message
re bisect --test "npm test"   # find the agent action that broke the build
re fork   experiment-claude   # branch into a parallel timeline
re revert <id>                # undo an action with causal preview of what else
                              #   you are implicitly undoing
```

When an agent touches 30 files and something breaks, you don't need to read
4 000 lines of chat. You ask Causari *why* and *when*.

## What makes it different

Existing tools either track text (git), track sessions (IDE checkpoints), or
track conversations (LangSmith, Helicone). **None of them connect a line of
code to the intent that produced it.** Causari does:

| You ask… | Causari answers… |
|---|---|
| `re why src/auth.ts:42` | The prompt, model, agent, tool, and reasoning that wrote that line. |
| **`re trace src/auth.ts:42`** | **Upstream causal cone.** Every prior event that contributed, transitively, through the files it read or wrote. The intellectual ancestry of a piece of code. |
| **`re impact <event>`** | **Downstream causal cone.** Every later event that depended, transitively, on what this one produced. The blast radius of an action. |
| **`re lens src/auth.ts`** | The file rendered with **per-line provenance annotations**: each line painted with the event id that introduced it. |
| `re find "the JWT refactor"` | Every event whose prompt, message or reasoning matches your query, ranked by relevance. |
| `re bisect --test "<cmd>"` | The first agent action whose output fails your tests. |
| `re fork claude-attempt` | A new timeline you can extend without touching the original. |
| `re diff a..b` | The exact file delta between two agent actions. |
| `re revert <id>` | Workspace snapped back to the pre-state of that action, **with a causal preview** of every downstream event you are implicitly undoing. |

### The bidirectional causal graph

Most version control is one-dimensional: a chain of commits. Causari is two-dimensional:

```
                 PAST                            FUTURE
   ┌──────────────────────────┐  ┌──────────────────────────┐
   │                          │  │                          │
   │   re trace foo.rs:42     │  │   re impact <event>      │
   │                          │  │                          │
   │   ← prompts & events     │  │   events & prompts →     │
   │   that produced this     │  │   that flowed from this  │
   │                          │  │                          │
   └──────────────┬───────────┘  └─────────────┬────────────┘
                  │                            │
                  │       a single event       │
                  └────────────►●◄─────────────┘
```

This unlocks a question nothing else can answer:

> *"If I revert this action, what else am I implicitly undoing?"*

`re revert` answers it before touching a single byte.

### Why `re trace` matters

Git blame names one author. `re why` names one event. **`re trace`** reconstructs
the *intellectual ancestry* of a piece of code:

```
calc.js:2
  export function sum(a, b) { return a - b; }

trace: 3 causal contributors found

● 0b8424ee83  align calc.js with updated spec
   agent: gpt-4o
   prompt: the spec was updated, make calc.js match
   because: wrote calc.js:2
  └─ 45230e9cda  update spec to redefine sum
     agent: gpt-4o
     prompt: the team decided sum should compute a-b, update the spec
     because: wrote spec.md which event 0b8424ee83 read
  └─ 55a6dd9392  implement calc per spec
     agent: claude-3.5
     prompt: implement sum() following the spec in spec.md
     because: wrote calc.js which event 0b8424ee83 read
```

The buggy line is not the root cause — the *prompt that asked the agent to
redefine the spec* is. Causari surfaces it. **You can debug prompts, not just
code.**

## How it works

Every event is a content-addressable object (BLAKE3) containing:

- `pre_snapshot` and `post_snapshot` — the workspace tree before and after
- `agent`, `model`, `tool`
- `prompt` — the user task that triggered the action
- `reasoning` — the agent's chain-of-thought when exposed
- `reads`, `writes`, `tokens_in`, `tokens_out`, `cost_usd`
- `parent` — the previous event in the timeline

Snapshots are incremental (only changed files create new blobs, just like
git's object store), so the storage cost is bounded by the *delta*, not the
absolute size of the workspace.

## Status

Working end-to-end today:

- `re init` — initialize a repository
- `re record` — record an event manually (CLI flags or `--stdin` JSON)
- `re watch` — auto-record every filesystem change as an event
- `re log` / `re show` / `re diff`
- `re revert <id>` — restore workspace, with causal preview of impacted downstream events
- **`re why <file>:<line>`** — provenance for any line of code
- **`re trace <file>:<line>`** — upstream causal cone
- **`re impact <event>`** — downstream causal cone (blast radius)
- **`re lens <file>`** — file with per-line provenance annotations
- **`re guard`** — scan recent events for risky patterns (critical edits without tests, bulk edits, etc.)
- **`re guard --badge`** — generate `.causari/guard-badge.svg` for your README
- **`re guard --summary`** — emit Markdown table for PR comments
- **`re find <query>`** — text search across prompts, messages, reasoning
- **`re bisect --good <id> --bad <id> --test "<cmd>"`** — find the broken action
- **`re fork <name> [--from <id>]`** — branch into a parallel timeline

Next on the roadmap:

- TUI à la `lazygit` for visual exploration
- Cross-event semantic search over prompts and diffs (embeddings)
- Counterfactual `re replay --with <model>` (re-execute past events under different models)
- Cryptographic timestamps (RFC 3161) for audit-grade timelines

## Plugging Causari into your agent (MCP)

Causari ships its own MCP server. Any agent runtime that speaks MCP
(Claude Desktop, Claude Code, Cursor, Cline, Windsurf, …) can register
Causari and get three new tools for free:

| Tool | What the agent uses it for |
|---|---|
| `causari_record` | Record one of its own actions into the ledger after each tool call. |
| `causari_recall` | Find past similar events *before* acting, to avoid repeating mistakes. |
| `causari_why`    | Inspect the provenance of a line before modifying code it didn't write. |

Get the JSON snippet to paste into your agent's config:

```bash
re mcp --install
```

Then in any conversation the agent can call those tools by name. Causari
silently builds a complete, queryable, causally-linked history of the session.

## Quickstart

### Install (one line)

Linux & macOS:

```bash
curl -sSf https://causari.dev/install.sh | sh
```

Windows (PowerShell):

```powershell
iwr -useb https://causari.dev/install.ps1 | iex
```

Installs a SHA256-verified, ~800 KB pre-built binary into `~/.local/bin`
(or `%LOCALAPPDATA%\Programs\causari` on Windows).

Prefer building from source?

```bash
cargo install --git https://github.com/croviatrust/causari
# or, with a local clone:
cargo build --release
./target/release/re --help
```

Scripted demos live in `scripts/`:

- **`demo.sh`** / **`demo.ps1`** — full happy-path with `re why` and `re bisect`
- **`demo-trace.sh`** / **`demo-trace.ps1`** — upstream causal cone (`re trace`)
- **`demo-bidir.sh`** / **`demo-bidir.ps1`** — bidirectional causality
  (`re impact`, `re lens`, causality-aware `re revert`)
- **`demo-mcp.sh`** / **`demo-mcp.ps1`** — MCP server end-to-end via JSON-RPC

Causari runs natively on **Linux, macOS, and Windows** — the binary is a
single ~2 MB executable with no runtime dependencies.

## License

Causari is released under the **Business Source License 1.1** (see
`LICENSE`). In plain English:

- **Free for you, forever**, in all of these cases:
  - personal use, including paid client work;
  - any organization's own internal development, in production and CI/CD,
    at any scale;
  - use by AI agents acting on infrastructure you control (laptop, server,
    CI runner) — this is the default way Causari is meant to be used;
  - academic research, teaching, non-commercial open-source projects;
  - redistribution of unmodified binaries via cargo / brew / apt / choco /
    nix / winget.
- **Not free** if you want to resell Causari itself to third parties as a
  hosted or managed service whose primary value is version control,
  causality or provenance tracking for AI agents. For that, talk to us.
- **Becomes Apache 2.0 automatically** four years after each version is
  published. The change is mechanical: nothing the project maintainers can
  prevent or accelerate.

Why BSL? Causari is meant to be widely used and modified, but we are not
interested in subsidizing a future closed-source clone built by a
better-distributed competitor. This is the same model that lets Sentry,
HashiCorp and CockroachDB stay genuinely useful and contributable while
sustaining the people who build them.

Contributing? See `CLA.md`.
