# Causari Real-Session Test Harness

Reproducible end-to-end tests that measure the causal-join confidence of
`re proxy` + `re watch` and the deterministic attribution of `re hook` on
**real, adversarial scenarios** — not the trivial happy path.

## Quick start

```powershell
# 1. Build the release binary
cargo build --release

# 2. Run the harness (Windows PowerShell)
powershell -ExecutionPolicy Bypass -File examples\real-session\run.ps1
```

The harness:
1. Creates a temporary git repo with `re init`
2. Starts a mock OpenAI-compatible upstream on `127.0.0.1:4399`
3. Starts `re proxy` on `127.0.0.1:4242` pointing at the mock
4. Starts `re watch` with 500ms debounce
5. Runs 4 proxy+watch scenarios and 2 hook-path scenarios
6. Prints the watch log (with live confidence scores) and `re why` output
7. Cleans up all processes

## Scenarios

| # | Name | What it tests |
|---|------|---------------|
| S1 | Clean baseline | File written verbatim from completion → expect 100% |
| S2 | Human manual edit | Human changes agent-written lines → expect no false attribution |
| S3 | Formatter reflow | Output reformatted before disk write → expect degraded confidence |
| S4 | Near-simultaneous | Two prompts in window, one file mixes both → expect per-line mis-attribution |
| H1 | Hook declared | Agent declares prompt+tool via hooks → expect exact attribution |
| H2 | Human edit, no hook | Human edits without hook firing → expect "no recorded event" |

## Architecture

```
                    ┌─────────────┐
                    │  run.ps1    │  orchestrator
                    └──┬───┬───┬──┘
                       │   │   │
              ┌────────┘   │   └────────┐
              ▼            ▼            ▼
       ┌──────────┐ ┌───────────┐ ┌──────────┐
       │mock_upstream│ │ re proxy │ │ re watch │
       │ :4399    │ │ :4242    │ │ (bg)     │
       └──────────┘ └─────┬─────┘ └─────┬────┘
                          │             │
                          │  exchanges  │  snapshots
                          ▼             ▼
                   ┌────────────────────────┐
                   │  .causari/             │
                   │  capture/exchanges.jsonl
                   │  objects/              │
                   │  refs/heads/main       │
                   └────────────────────────┘
```

The mock upstream reads its next response from `_next_completion.txt` (set via
`$env:COMPLETION_FILE`). The harness overwrites that file before each proxy
call, so every scenario gets a different model output — and we control exactly
how the code on disk diverges from what the "model" returned.

## Files

- `run.ps1` — orchestrator (PowerShell)
- `mock_upstream.py` — configurable OpenAI-compatible mock (Python 3, stdlib only)
- `RESULTS.md` — findings from the last run
