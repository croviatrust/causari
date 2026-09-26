# Changelog

Each section is the release note of the tag with the same number; the
release workflow copies it verbatim. Counts, not adjectives.

## Unreleased

### Install

- `npx causari` and `pipx run causari`: the npm and PyPI launchers are
  published (0.3.0, the npm one by hand once, both registries now through
  Trusted Publishing from `publish-shims.yml`). Each downloads the release
  archive for the platform, checks it against `SHA256SUMS.txt` and runs
  it. Listed on the home page, in the README and in `llms.txt`. From the
  next tag the npm package carries a provenance attestation.

## 0.3.0 — 2026-09-24

Method v3: `re audit` compares AI-tagged lines with the same repository's
untagged lines of the same age, so every ratio now has a baseline from
the same tree, the same reviewers and the same history. Cursor native
hooks, secret redaction before every clear-text write, a threat model,
and a Survival Report that corrects by revision and counts a renamed
repository once. Every v2 figure is computed exactly as before; the
audit JSON gains a `baseline` block and the report schema is unchanged.

### Record

- `re hook cursor`: capture from Cursor's native hooks. Merges seven
  command hooks into `.cursor/hooks.json` (`--project`, the default, also
  run by Cursor cloud agents) or `~/.cursor/hooks.json` (`--user`);
  `--dry-run` prints the merged file; idempotent, other hooks in the file
  are kept. `beforeSubmitPrompt` records the prompt, its attachments and
  the model per `conversation_id`; `preToolUse` (Shell|Write) snapshots
  the tree before the tool; `afterFileEdit` records one declared event per
  written file (agent `cursor`, evidence `cursor-hook`, model from the
  payload, prompt of the same conversation, attachments as `reads`);
  `afterShellExecution` records a command that changed the tree with the
  command as message; `afterAgentResponse` stores the answer as an
  exchange under agent `cursor`; `stop` drops the conversation's unused
  pre-states; `sessionStart` returns the experience briefing as
  `additional_context`. Files outside the repository are ignored. Every
  hook answers a JSON object, `{"permission":"allow"}` for the permission
  hook, even without a ledger. `re` must be on `PATH` for Cursor.
- Prompt records carry the runtime's `model` and `attachments` when its
  payload has them; Claude Code lines are unchanged.
- Integration matrix: Cursor is its own row (prompt and file exact, model
  yes, tokens and cost no); Windsurf and Copilot stay on MCP self-report.

### Words

- The one-liner is "AI-written code has no author. It has causes. Causari
  records them." — `records`, not `proves`: the ledger records prompts,
  reads and writes as the runtime reports them; a seal proves that a record
  or an audit was not altered, not that it is true. Same line in the
  canon, README, MANIFESTO, home page, OG card, `llms.txt`, `re --help`
  and the npm/PyPI READMEs.
- `re audit` prints "Verified AI-tagged" where it said "Verified
  AI-authored": the class is what the commit metadata says, not who typed
  the code. The JSON keys (`verified`, `probable`) are unchanged.
- README, Family: Causari "records why a line of code exists and measures
  whether it is still there"; it does not prove why.

### Security

- Secret redaction before every clear-text write: prompts (`re proxy`,
  Claude Code and Cursor hooks), completions and agent answers, shell
  commands, and the `message`/`prompt`/`reasoning` of every event
  (`commit_event` is the choke point, so `re record`, MCP and `re watch`
  are covered). Recognised shapes: `sk-…` API keys, Stripe, GitHub,
  GitLab, Slack, Hugging Face, npm and PyPI tokens, AWS access key ids,
  Google API keys, `Bearer` values, JWTs and PEM private-key blocks —
  replaced by `[redacted:<kind>]`; the record carries `redactions: <n>`
  (absent when zero, so untouched bytes and object ids are unchanged).
  Prefix-anchored, no classifier: a bare password passes through, and
  `SECURITY.md` says so.
- `re proxy`, `re hook claude-code` and `re hook cursor` print once what
  is stored, where, and what the redaction does not catch.
- `SECURITY.md`: reporting, the storage table (what, by which command,
  where, in clear or hashed), redaction scope, permissions, retention and
  deletion, the loopback proxy, what a seal proves. `docs/threat-model.md`:
  assets, actors, and for each claim what is defended, what is not, and
  the assumption behind it.

### Site

- The weekly live audit fetches every data path (`latest.json`,
  `report.json`, the audit bytes, the feed, `llms.txt`, `sitemap.xml`)
  as a plain script — urllib's default User-Agent, no browser, no cookie —
  and reports a 403 as critical. Today causari.dev answers those clients
  with Cloudflare's Browser Integrity Check (error 1010) while named agents
  and browsers get 200: "reproducible by anyone" is false for a script
  until the path is exempted. The canon carries the rule and the paths.

### Audit

- Method v3: the same repository's untagged lines are the baseline. Every
  UNKNOWN commit (no machine-readable AI signal: human-written,
  inline-completed and untagged-agent code alike) enters a `baseline`
  block: `untagged` (the same figures as `verified`), `by_age` (six
  windows of line age, 0–30 to 730+ days from a commit's committer date to
  HEAD's, AI-tagged and untagged side by side), `age_matched` (AI-tagged
  survival against untagged survival re-weighted to the AI-tagged age mix,
  over windows where both cohorts hold at least 5 commits, with the share
  of AI-tagged lines covered) and `oldest_surviving` (the oldest commit
  still owning a line at HEAD and the commits, lines and AI-tagged commits
  older than it: a cleared or rewritten repository shows there). Costs
  nothing extra: numstat already walked every commit and blame already
  named every line's owner. Terminal and `--summary` print a Baseline
  section; every v2 figure is computed exactly as before. Measured before
  release on OpenHands (30.1 % AI-tagged vs 36.1 % untagged of the same
  age; 6,584 commits older than the oldest surviving line of 2026-04-24),
  gemini-cli (62.4 % vs 67.1 %) and pydantic-ai (87.6 % vs 82.2 %, one
  commit holding 70 % of the AI-tagged lines).
- `re audit --json` names what it measured: `repository.head` (the commit
  at HEAD, 40 hex) and `repository.origin` (the origin URL with credentials
  stripped, or `sha256:` of the path when there is no remote). Two audits
  with the same `head` measured the same tree, whatever the repository is
  called. Audit seals keep binding to the exact bytes, which now include
  this object.

### Survival Report

- The report carries the method v3 baseline. Every row measured with v3
  gains `baseline` (untagged figures, the by-age windows, the age-matched
  gap, the oldest surviving line); the page and `report.md` add the
  columns "Untagged, same age" and "Gap", a Baseline section with the age
  windows summed across repositories as counts (no gap is computed on the
  sums: it would be the gap of the largest repository), the median gap
  across repositories with its bootstrap interval, how many gaps fall on
  each side of zero,
  and the list of repositories where more than half of the commits
  predate the oldest line still at HEAD (cleared or rewritten; marked
  "· rewritten" in the table). Repository pages and `latest.json` carry
  the same block. Rows measured before v3 keep building without it, and
  reports #1 and #2 render as before. Additive: the schema stays
  `causari.survival_report.v1`.
- One repository counts once, whatever it is called. Report #2 counted
  `All-Hands-AI/OpenHands` and `OpenHands/OpenHands` — one repository,
  renamed on GitHub — as two rows with byte-identical audits. The
  generator now drops audits that measured the same commit or are
  byte-identical, keeps the name in `.github/survival-repos.txt`, and
  lists the other under `excluded.duplicates` with the name it was
  counted under. Discovery resolves every hand-picked seed through
  `GET /repos/{owner}/{repo}` and treats the listed name and the name
  GitHub now gives as one seed, so a renamed seed is never discovered a
  second time; the seed line was corrected to `OpenHands/OpenHands`.
- `survival_report.py revise`: a published report is corrected by
  revision, never in place. The superseded `report.json`/`report.md` are
  frozen as `report.r<K>.*`; the new report carries `revision`,
  `revised_at` and `corrections[]` (note, previous aggregate, previous
  file and DOI), shown on the page, in the markdown, in the archive row
  and the feed entry. Rebuilding the same number with `build` is refused.
  The Zenodo deposit publishes a revision as version `#N-rK` under the
  same Concept DOI and states the correction in the record. Repository
  pages that existed only under a dropped duplicate name are removed and
  redirected to the kept name (page, badges, `latest.json`).
- Report #2 revision 2 (2026-09-24): 54 repositories, 13,733,809 of
  27,108,452 lines (50.7 %); revision 1 (55, 50.0 %) stays at
  `report.r1.json`, DOI 10.5281/zenodo.22928161.

## 0.2.0 — 2026-09-20

The first release after the 2026-09-20 review (`docs/review-2026-09-20/`).
Decisions in `ROADMAP.md`, thesis in `MANIFESTO.md`.

### Measure

- `re audit` method v2: `git blame -w -M -C`, `.git-blame-ignore-revs`
  honoured, only the git trailer block is parsed, `Assisted-by:` and
  `copilot-swe-agent[bot]` recognised, whole-word agent names, per-commit
  cap (p95, ceiling 10,000 lines), median and largest-commit share,
  `coverage` block in every output, shallow clones refused unless
  `--allow-shallow`.
- No verdicts anywhere: no rank, no colour, no "healthy" in `audit`,
  `churn`, `guard`, the Action or the site. `re churn --json`,
  `--fail-below`; `re guard --json`, `--fail-on alert|warning`.
- The public leaderboard is gone. Weekly measurements are published as
  counts, opt-out honoured, no bot opens issues on other people's repos.

### Record

- Proxy parses `tool_calls` (OpenAI), `tool_use` (Anthropic) and the
  Responses API into `response_text`; requests usage on OpenAI streams;
  records exchanges cut short by a disconnect as `truncated`; captures only
  `POST`s to completion endpoints; merges Claude Code hook events with the
  exchange behind them (model, tokens, cost).
- Store: atomic, self-healing object writes; atomic ref/HEAD writes;
  unrestorable names skipped at snapshot time; executable bit stored;
  `.gitignore` semantics in git work trees; stat cache (5,000 files, one
  edit: 21 ms).
- One line-provenance engine behind `why`, `trace`, `lens`, `impact` and
  the MCP `causari_why`; every event carries an evidence class
  (`declared`, `correlated`, `observed`) that every consumer prints.
- `re watch` records nothing when the tree is unchanged.
- `re show` prints model, tokens, cost, prompt, reasoning and evidence;
  `--json`.

### Prove

- `re audit --seal` issues a Crovia Seal (`crovia.seal.v1`) over the audit
  JSON, bound to the audited commit and the method version, hash-chained
  with the proxy's completion seals under one issuer key per repository.
  `re seal verify FILE` checks it offline; so does the static page
  causari.dev/verify (no network request, no third-party code). A seal
  proves the numbers were not altered after the run, not that they are
  true, and the verifier says so.
- `re proof` retired (exit 2 with the replacement named); `re audit
  --seal` and `re seal verify` take its place.
- `re proxy --pnx` witnesses the agent's traffic and writes a signed run
  sheet per session (`crovia.pnx.v1`: winnowing fingerprints, sparse
  Merkle map, Ed25519). `re pnx prove` shows offline that no asset from a
  given set appeared in that traffic; `re pnx verify`, `sheet`, `list`.
  Proofs verify under the Python reference `tacet-pnx` and vice versa; the
  Action verifies a proof handed to it (`pnx-proof`, `pnx-assets`,
  `pnx-fail-on-present`).
- Action inputs `seal` and `seal-key`: the audit seal as an artifact, with
  a persistent issuer identity when a key is passed.
- Seal issuer: `deny_unknown_fields`, dedicated key, domain-separated
  payload, keys written `0600`, `seal_id` on exchanges.

### Report

- Weekly Survival Report at causari.dev/reports/survival/: counts per
  agent across public repositories, archive, Atom feed, `latest.json`, one
  DOI per issue on Zenodo. Method in `docs/survival-report.md`.

### Distribution and identity

- Binary `causari` with `re` as alias, both in every archive; installers,
  Homebrew tap (`croviatrust/tap`), Scoop bucket, crates.io
  (`cargo install causari`), signed SLSA attestations on every archive.
- MCP Registry entry (`io.github.croviatrust/causari`); Claude Code plugin
  (`/plugin marketplace add croviatrust/causari`); one-click Cursor link.
- `--help` grouped by task. DCO replaces the CLA. One legal entity name.
- Identity: the `∵` mark, monospace wordmark, monochrome palette;
  causari.dev rewritten with no external dependencies; `canon/canon.json`
  and `scripts/audit_surfaces.py` check every public claim in CI.

## 0.1.5

Last release before the review. Archives named `re-<tag>-<target>.*`,
single binary `re`.
