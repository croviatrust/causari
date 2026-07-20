# Causari Real-Session Test Results

**Date**: 2026-07-13
**Binary**: `re 0.1.0` (release build from `main`)
**Method**: `run.ps1` orchestrates a mock OpenAI-compatible upstream, `re proxy`,
`re watch`, and `re hook-event` against a temporary repo. Four adversarial
scenarios exercise the causal-join heuristic; two hook-path scenarios exercise
the deterministic agent-declared path.

## Summary table

| # | Scenario | Path | Confidence | `re why` result |
|---|----------|------|-----------|-----------------|
| S1 | Clean: file == completion verbatim | proxy+watch | **100% (5/5)** | Correct prompt, model, agent |
| S2 | Human hand-edits agent-written lines | proxy+watch | **no correlation** | agent=proxy-watch, no model/prompt — correct silence |
| S3 | Formatter reflows output before disk write | proxy+watch | **50% (3/6)** | Partial attribution to correct prompt; confidence correctly degraded |
| S4 | Two prompts in window, one file mixes lines from both | proxy+watch | **75% (3/4)** | **MIS-ATTRIBUTION**: redis line attributed to `connect_db` prompt |
| H1 | Agent declares prompt + tool via hooks | hook | exact (deterministic) | Exact prompt, agent=claude-code, tool=Write |
| H2 | Human edits file — no hook fires | hook | — | **"no recorded event"** — zero false attribution |

## Detailed findings

### S1 — Clean baseline (100%)

The completion text is written verbatim to disk. All 5 significant lines match
the captured exchange. The causal join correctly identifies the prompt, model
(`gpt-4o-realsession`), and reports 100% confidence.

**Verdict**: The happy path works as designed. This is the baseline against
which degradation is measured.

### S2 — Human manual edit (no false attribution)

After S1 writes `auth.py`, a human changes `hours=24` → `hours=48` and adds a
comment. The watcher records the file change but finds **no correlation** with
any captured exchange — the edited line text does not appear in any completion.
`re why auth.py:3` correctly reports `agent: proxy-watch` with **no model and
no prompt**.

**Verdict**: Human edits are not falsely attributed to an LLM. The system is
honest about what it cannot attribute. This is the correct behavior for a
forensic tool.

### S3 — Formatter between model and disk (50%, down from 100%)

The model returns a one-liner `slugify`. A "formatter" (simulated) reflows it
into a multi-line expression with different quoting before writing to disk.
Only 3 of 6 significant added lines match the completion text. Confidence drops
to 50%.

**Verdict**: The confidence score correctly signals degraded correspondence.
The partial match still identifies the right prompt. In production, a
formatter/linter pipeline between agent output and disk write will reduce
confidence proportionally to the divergence.

### S4 — Near-simultaneous prompts (75%, per-line mis-attribution)

Two completions arrive within the 300s window:
1. `connect_db` (3 significant lines)
2. `cache_get` (3 significant lines)

A single file `infra.py` is written containing lines from **both** completions
(2 from connect_db, 1 from cache_get). The causal join attributes the entire
change to the `connect_db` prompt (75%, 3/4 lines matched) — the best-overlap
winner. `re why infra.py:3` (the redis line) reports the `connect_db` prompt,
which is **wrong**: that line came from `cache_get`.

**Verdict**: This is the honest failure mode of the current heuristic. The
`correlate()` function picks the single best-matching exchange for the *whole
file change*. When one file mixes contributions from multiple prompts, per-line
attribution is lost. The confidence score (75%) looks reasonable at the file
level but masks the per-line error.

### H1 — Hook path (exact, deterministic)

The hook path records the real `UserPromptSubmit` payload (exact prompt text)
and the real `PostToolUse` payload (tool name + file path). `re why` returns
the exact prompt, agent=`claude-code`, tool=`Write`. No heuristic, no
confidence score — the attribution is exact by construction.

### H2 — Human edit, no hook (correct silence)

A human edits `service.py` without any hook firing. `re why service.py:2`
reports **"no recorded event introduced this line"**. The system does not
invent an attribution.

## Key takeaways

1. **The causal join works on clean cases** (100% confidence) and degrades
   honestly on dirty cases (50% on formatter reflow).

2. **Human edits are not falsely attributed** — the system stays silent rather
   than guessing. This is critical for forensic credibility.

3. **Per-line mis-attribution is the real failure mode** (S4). When one file
   mixes contributions from multiple near-simultaneous prompts, the current
   file-level `correlate()` picks the single best match. A per-line or
   per-hunk join would be more accurate but more complex.

4. **The hook path is strictly superior** where available: exact attribution,
   no heuristic, no confidence degradation, correct silence on human edits.
   The proxy+watch path is a fallback for agents without hook support.

5. **Watch records its own artifacts** if the log file is inside the watched
   tree. The harness keeps `watch.log` and `_next_completion.txt` outside the
   repo. In production, `.causari/` is auto-gitignored and filtered by the
   watcher, but other generated files (build artifacts, logs) are not — this
   is a known gap (configurable ignore patterns, on the roadmap).

## Recommendation for v0.1 hero path

**Hook-first.** The hook path is deterministic, exact, and correctly silent on
non-agent changes. The proxy+watch path is a valuable fallback for agents
without hook support (Cursor, Windsurf without base-URL control), but its
heuristic nature and the S4 per-line mis-attribution mean it should not be the
primary narrative.

The README should lead with `re hook claude-code` as the recommended
integration, with `re proxy` + `re watch` presented as the universal fallback.
