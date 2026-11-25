# The Survival Report

Weekly, numbered, dated, citable. Published at
[causari.dev/reports/survival/](https://causari.dev/reports/survival/) as a
static page, an Open Graph card, an Atom feed, a JSON dataset and a Zenodo
deposit with a DOI. Same pipeline shape as the
[Crovia Silence Report](https://croviatrust.com/report/), so the two read
as siblings.

## What it is

Counts of surviving lines from AI-tagged commits in N open-source
repositories. Reports #1 and #2 are method v2. Reports #3 and #4, and the
0.3.0 release, are method v3. Report #4 is DOI 10.5281/zenodo.23196011.
Reports #5 (2026-10-12) and #6 (2026-10-19) are method v4, measured with
causari 0.4.1 (2026-10-08, commit d4c4949). That binary counts a git-ai
note only when it names a non-empty tool; method v3 counted any parseable
note. The published binary 0.5.0 (2026-10-21) is method v6, the method this
source computes; reports from #7 on are measured with it. No binary shipped
method v5. Published reports are not recomputed. For each repository the
report states how many lines were introduced by commits that carry
machine-readable AI authorship metadata (`Co-Authored-By` trailers naming
an agent, bot author identities, aider markers, `Assisted-by:`, and a
git-ai note under the method version of that report) and how many of
those lines `git blame -w -M -C` still attributes to those commits at HEAD.
Alongside the line-weighted ratio it gives the capped ratio (no commit
weighs more than the 95th percentile of per-commit introduced counts in its
repository, never more than 10,000 lines), the median per-commit ratio and
the share of the largest commit. The aggregate carries a 95 % bootstrap
interval computed by resampling repositories (2,000 resamples, seeded from
the report number so the bytes are reproducible).

Every number links to the audit bytes behind it (`repos/<owner>__<repo>.json`)
and to the command that reproduces it: `re audit <owner/repo> --json`.

## What it is not

It is not a quality judgement, a productivity measure, total AI usage,
proof that a model wrote the line, a security finding, a correctness
finding, or a causal effect of AI. Deleted lines include removed features,
moved code and rewritten prototypes; surviving lines include dead code.
There is no rank, no colour, no verdict; rows are alphabetical. Granularity
is the commit: a qualifying signal tags the commit's introduced and
surviving lines, not a line range inside a git-ai note.

It is not a sample of "all AI code". Inline completions (Copilot, Cursor
Tab, Windsurf) leave no trace in git and are invisible. Untagged agent
commits are UNKNOWN and never counted. The repositories in
[`.github/survival-repos.txt`](../.github/survival-repos.txt) were selected,
not drawn at random: a hand-picked part added by pull request, and a part
filled every week by [`scripts/survival_discover.py`](../scripts/survival_discover.py)
with the most-starred public repositories where GitHub commit search finds
at least five commits carrying the same AI authorship metadata (at least
100 stars, read before the repository-wide count so repositories under that
floor are not search-counted; forks, archived repositories and opt-outs dropped; stars first,
then commits; up to 100 in total; the rule
is on the [method page](https://causari.dev/method#selection) and the
counts behind each selection in
[`.github/survival-discovery.json`](../.github/survival-discovery.json)).
That order chooses the sample and nothing else; the list and every report
page are alphabetical. The intervals describe the sampled repositories
only, not a population.

The line-weighted column of the archive is not a series. Each rate is that
report's own aggregated repositories. Report #1 aggregated 10 (method v2),
#2 aggregated 54 (method v2), #3 aggregated 43 (method v3); a later report
with a different count, including one near 61, does not continue those
rates. The generator states the counts under the table whenever the
repository set or the method differs (`archive_rate_note`). A repository
followed across reports is on its own page.

## Prior measurement work

GitClear publishes churn reports built from code-change patterns across the
repositories it analyses. arXiv 2601.16809 ("Will It Survive?") follows the
modification of agent-authored code in 201 projects with its own detector
and finds that such code is modified less often than human-written code.
The Survival Report does not reproduce either method and does not
adjudicate between them: it publishes counts from git metadata alone, with
the method version, the tool version and the exact bytes behind every
number, so that the three can be read side by side.

## Rules baked into the generator

`scripts/survival_report.py` enforces what the 2026-09-20 reviews asked
for, so no editor has to remember it:

| Rule | Where |
|---|---|
| Rows alphabetical (case-insensitive), never by rate | `collect()` |
| No rank column, no per-row colour, no adjectives | templates; `scripts/audit_surfaces.py --gate` checks the forbidden words of `canon/canon.json` on the generated pages |
| VERIFIED only; PROBABLE listed, never summed | `collect()` |
| `coverage.small_sample` (fewer than 5 VERIFIED commits) → "measured but not aggregated" | `collect()` |
| `coverage.shallow` → excluded with a note | `collect()` |
| Opt-out list honoured (`.github/survival-optout.txt`, case-insensitive, `#` comments) | workflow skips them; generator drops them again |
| One repository counts once, whatever it is called: audits that measured the same `repository.head` (written by `re audit` from 0.2.1) or are byte-identical are one measurement; the name in `.github/survival-repos.txt` is kept, the other is listed under `excluded.duplicates` with the name it was counted under | `drop_duplicate_audits()`; discovery resolves every seed through `GET /repos` so a renamed seed is never discovered a second time (`resolve_seeds()`) |
| Bootstrap interval over repositories, 2,000 resamples, seed = report number, labelled as an interval over the sample | `bootstrap_rate()`, `bootstrap_median()` |
| Archive line-weighted column names each report's repository count and method, and says the column is not a series when the set or the method differs | `archive_rate_note()`, `render_index()` |
| Method section states method version, tool version, blame flags, cap rule, sample floor; links `/method` | `render_report()` |
| Report directories are append-only; a directory holding a different report is never overwritten, and the same number is never rebuilt in place: a correction is a revision | `write_report()`, `revise()` |
| A correction keeps the superseded bytes (`report.r<K>.json`, `report.r<K>.md`) next to the page; the new `report.json` carries `revision`, `revised_at` and `corrections[]` (what changed, the previous aggregate, the previous file and DOI); page, markdown, archive row and feed entry say so; the measurement date does not move | `revise()`, `correction_lines()` |
| Old method v1 data cannot be relabelled as v2 | `from_existing()` refuses rows without a `coverage` block |

## Files

```
site/reports/survival/
  index.html                 archive, newest first
  feed.xml                   Atom, one entry per report
  latest.json                copy of the newest report.json
  zenodo.json                Concept DOI and one record per report (written by the deposit)
  <YYYY>/<NN>/
    index.html               the report page
    report.json              counts, intervals, coverage, DOI (schema causari.survival_report.v1);
                             from method v3 each row and the aggregate carry a `baseline` block
    report.r<K>.json, .md    revision K as it was published, unchanged, when a later revision exists
    report.md                plain-text version, also the Zenodo description
    card.svg, card.png       Open Graph card, identity style
    repos/<owner>__<repo>.json   the exact `re audit --json` bytes per repository
    reach.sheet.json         signed PNX run sheet (crovia.pnx.v1) with the reach record: where the
                             measurement connected, under which policy (from the first run behind the witness)
    egress-policy.json       the policy the witness applied, byte for byte; its hash is bound in the sheet
```

`site/_redirects` and `site/sitemap.xml` contain a block managed by the
generator: `/survival` → `/reports/survival/`, `/survival-data.json` →
`latest.json`, `/report` → the latest report.

Cloudflare Workers refuses a deploy that holds one static asset above
25 MiB, and the site then keeps serving the last deploy that worked. A
generated file above that size (the audit of a large repository) stays in
the repository with its bytes unchanged; the generator lists it in the
managed block of `site/.assetsignore`, so it is not uploaded, and adds a
redirect from its path to the same file on `raw.githubusercontent.com`.
`scripts/audit_surfaces.py` fails when a file above the limit is not
handled this way.

## How a report is made

[`.github/workflows/survival-report.yml`](../.github/workflows/survival-report.yml),
Mondays 05:17 UTC or on demand:

1. Twenty `audit` jobs run in parallel (`strategy.matrix.shard: 0…19`,
   `fail-fast: false`). Each installs the latest published release (`causari` and
   `re`), currently 0.5.0, method v6, checksum-verified, and takes every repository of
   `.github/survival-repos.txt` whose index in the list is `shard mod 20`,
   skipping `.github/survival-optout.txt`: full `git clone` (method v2
   refuses shallow clones; `CLONE_TIMEOUT_S`, 1800 s), a commit graph
   (`git commit-graph write --reachable --changed-paths`, faster blame,
   same results), `re audit <clone> --json` (`AUDIT_TIMEOUT_S`, 10800 s: a
   full `-w -M -C` blame of ~20,000 files over ~20,000 commits takes about
   two hours on a 4-core runner), keep the bytes. A repository that fails
   is recorded in the shard's `failed` list, never fatal; a timeout is
   logged as such and written to the repository's `.err`. The clones and
   audits run behind an egress witness (`scripts/egress_witness.py`, a
   CONNECT proxy on `127.0.0.1:3128` under
   [`.github/egress-policy.json`](../.github/egress-policy.json):
   `github.com:443` and nothing else): `https_proxy` points git at it,
   `GIT_LFS_SKIP_SMUDGE=1` keeps LFS objects, a second destination, from
   being fetched (blame reads committed pointers either way). A destination
   outside the policy is refused with 403 and written down as `blocked`;
   every attempt is one line of `reach-shard-<k>.jsonl`. The shard uploads
   `/tmp/run` (audits, the `.err` of each failure, `run-shard-<k>.json`, the
   connection log) as the artifact `run-shard-<k>`.
2. The `report` job (`needs: audit`, `if: always()`) downloads every shard
   into `/tmp/run` and runs `python3 scripts/survival_report.py merge-shards
   --run /tmp/run`: one `run.json` with the same schema (`generated_at`,
   `tool`, `tool_version`, `method`, `command`, `repos`, `failed`,
   `opted_out`); a repository no shard reported (a shard that hit its
   timeout) is recorded as failed.
3. The shards' connection logs become one signed run sheet:
   `tacet-pnx witness --reach /tmp/run/reach.jsonl --policy
   .github/egress-policy.json --reach-mode enforce` (the PNX reference,
   `crovia-tacet`, installed from the countersign repository until 0.5.0 is
   on PyPI), signed with the seed in the secret `PNX_WITNESS_SEED` as
   `urn:causari:survival-report:witness` or, without the secret, with a key
   made for the run (a notice says so; the public half is in the sheet).
   `tacet-pnx verify reach.sheet.json --policy .github/egress-policy.json
   --json` writes `reach.verify.json`; the build refuses a sheet without a
   passing verification and publishes a verified `outside-policy` verdict
   as what it is. A run whose shards uploaded no log is published without a
   receipt, with a warning.
4. `python3 scripts/survival_report.py build --run /tmp/run`: report number =
   existing report directories + 1; writes the report, the archive, the feed,
   `latest.json`, the redirect and sitemap blocks. `scripts/audit_surfaces.py
   --gate` runs on the result.
   The report's `reach` block (sheet, policy hash, destinations with outcome
   and byte counts, verdict, what is and is not covered) feeds the section
   "Where this measurement connected" of the page and of `report.md`; the
   sheet and the policy are copied next to the report and into the deposit.
   Covered: the connection attempts in the shard logs that were uploaded
   and concatenated into the sheet. Not covered: a shard that uploaded no
   log; what the runner does outside the measurement step (checkout, tool
   install, artifact upload); and any connection that did not go through
   the witness. The policy file is the allowlist and nothing else. The
   sheet does not say the whole Actions job spoke only to github.com.
5. Commit `site/reports/survival/**` to `main`: plain commit, never a
   force-push. A concurrency group keeps two runs from racing. `main` is
   protected (required check `lint`), so the checkout uses the secret
   `REPORT_PUSH_TOKEN` (a fine-grained token of an administrator, Contents
   read and write) and falls back to `github.token`, which cannot pass the
   protection: the report is then uploaded as the artifact `survival-report`
   and applied by hand. The `push-check` workflow (manual) proves the secret
   works without touching `main`: token identity, admin permission,
   `enforce_admins` off, one push to a throwaway branch, deleted.
7. `python3 scripts/survival_dataset.py build /tmp/hf-dataset`, then, if
   `HF_TOKEN` is set, `python3 scripts/survival_dataset.py push /tmp/hf-dataset`:
   the whole report tree as a dataset at
   `huggingface.co/datasets/CroviaResearch/causari-survival` (three JSONL
   tables derived from the reports, the raw bytes unchanged under `raw/`, the
   card rendered from `docs/survival-dataset.md`). Without the token the step
   builds and prints a dry run.
6. If `ZENODO_TOKEN` is set: `python3 scripts/zenodo_deposit.py <report dir>`
   publishes the record, writes the DOI into `report.json`, re-renders the page
   and the archive, and commits again. Without the secret the step prints a
   notice and the dry-run payload; the report is published without a DOI and
   the page says "DOI: pending deposit".

Locally:

```sh
mkdir -p /tmp/run && for r in owner/repo …; do
  git clone --single-branch "https://github.com/$r" "/tmp/clones/${r/\//__}"
  re audit "/tmp/clones/${r/\//__}" --json > "/tmp/run/${r/\//__}.json"
done
# run.json: generated_at, tool, tool_version, method, command, repos, failed, opted_out
python3 scripts/survival_report.py build --run /tmp/run
python3 scripts/zenodo_deposit.py --dry-run site/reports/survival/2026/01
python3 -m pytest scripts/tests -q
```

## How the list is filled

[`.github/workflows/survival-discover.yml`](../.github/workflows/survival-discover.yml),
every Sunday, 04:23 UTC (the day before the report), or on demand, runs
`python3 scripts/survival_discover.py` with the workflow token and commits
`.github/survival-repos.txt` and `.github/survival-discovery.json` to `main`
as `causari-report[bot]` (same push rules and fallback as the report). The
script samples the most recent public commits per VERIFIED signal from
`GET /search/commits`, counts them repository-wide with `repo:`-scoped
queries, keeps repositories with at least 5 and at least 100 stars
(`--min-stars`; a raw commit count selects contribution-graph painters and
mirrors), drops forks, archived repositories and opt-outs, orders by stars
then commits, keeps the hand-picked seeds above the `# discovered …` line
and fills the list up to 100 (shorter when fewer clear the floors, never
padded). `--dry-run` prints without
writing; the tests in `scripts/tests/test_survival_discover.py` run
against a fake GitHub.

## Zenodo

The deposit mirrors `ops/phase0/zenodo_deposit_weekly.py` of the Silence
Report: `upload_type` publication / report, creators Crovia Trust, licence
CC-BY-4.0, keywords, related identifiers back to the page, the JSON, the
method page and the repository. One Concept DOI for the series; each report
is a version of it. Idempotent per report: same bytes → skip; changed bytes →
new version of the same record, never a duplicate.

Setup for the repository owner:

- Create a personal access token on [zenodo.org](https://zenodo.org/account/settings/applications/)
  with `deposit:write` and `deposit:actions`, add it as the repository secret
  `ZENODO_TOKEN`.
- To rehearse against the sandbox first, create a token on
  [sandbox.zenodo.org](https://sandbox.zenodo.org/) and set the repository
  variable `ZENODO_SANDBOX=1`; sandbox records are kept apart in
  `zenodo.json` so a sandbox concept is never reused live.
- The first successful deposit creates the Concept DOI; it is then shown on
  every report page and in the archive.

## Correcting a published report

A published number that turns out to be wrong is not edited: it is
superseded. `python3 scripts/survival_report.py revise --number N --run
<dir> --note "<what was wrong, what changed>"` freezes the current
`report.json` and `report.md` as `report.r<K>.json` / `.md`, builds the
report again from the run directory (the original one, or a corrected
one), and writes `revision: K+1`, `revised_at` and a `corrections` entry
that names the note, the previous aggregate, the previous file and the
previous DOI. The report date stays the date of the measurement. A DOI
belongs to bytes, so the new revision starts without one; the next
deposit (`survival-report.yml` with `deposit_only`) publishes it as a new
Zenodo version `#N-rK+1` under the same Concept DOI and writes the DOI
back. A repository page that existed only under a dropped name is removed
and its URLs (page, badges, `latest.json`) redirect to the kept name.

Report #2, revision 2 (2026-09-24): `All-Hands-AI/OpenHands` and
`OpenHands/OpenHands` were one repository counted twice (byte-identical
audits); 54 repositories, 13,733,809 of 27,108,452 lines (50.7 %), where
revision 1 said 55, 14,015,893 of 28,046,116 (50.0 %). Revision 1 is
`report.r1.json`, DOI 10.5281/zenodo.22928161.

## Report #1

Report #1 (2026-09-20) was generated from fresh `re audit --json` runs on
full clones of ten repositories from the measured list. The retired
`survival-data.json` of 2026-08-25 was method v1 (blame without `-w -M -C`,
no cap, no coverage block) and was therefore not relabelled as Report #1;
`survival_report.py from-existing` refuses such data by design.

## Method v6: Exact Measurement Inputs and Structural Limits

A Method v6 receipt (`re audit --json` / `re audit --seal`) is determined by
the following inputs, all recorded in `repository` and `coverage`:

| Input | Field in receipt | How Method v6 reads it |
|---|---|---|
| Commit graph and tree at `HEAD` | `repository.head`, `repository.root_commits` | `git --no-replace-objects` on every invocation; `.git/refs/replace/*` is ignored so local replacement refs cannot alter history or blame without changing `HEAD`. |
| Method version | `method`, `coverage.method` | `"v6"` |
| Blame flags & `HEAD` tree enumeration | `coverage.blame_flags` | `["-w", "-M", "-C"]` over regular-file blobs (`100644`/`100755`) enumerated directly from `git ls-tree -r -z HEAD` with `core.quotepath=false` (never from `.git/index`, so staged `git rm --cached` and `--bare` clones match `HEAD`). On Unix, raw non-UTF-8 path bytes are passed via `OsStrExt::from_bytes`; on Windows, non-UTF-8 paths in `HEAD` are refused with an explicit error rather than replaced with `U+FFFD` (which would falsify survival to `0%`). Any `git blame` failure on a `HEAD` blob is propagated as an error, never counted as `0` surviving lines. Method v6 keeps only the paths `git diff-tree -r --numstat -z <empty tree> HEAD` gives a line count (git's own binary decision, the one `git log --numstat` makes for `introduced`), adds `120000` symlinks (one line each, as numstat counts them), and records the blobs left out in `coverage.binary_files`. Under v5 a binary blob was blamed and its newline bytes counted as surviving lines no commit had introduced. |
| Numstat flags | `coverage.numstat_flags` | `["-M", "-z"]` on `git log --no-merges --numstat -M -z` so pure file renames do not count existing lines as newly introduced on the rename commit, and NUL-delimited paths preserve literal `" => "` or non-ASCII filenames without heuristic splitting. |
| Committed blame ignore list | `coverage.ignore_revs_file`, `coverage.ignore_revs_sha256` | Read exclusively from `HEAD:.git-blame-ignore-revs` (`git cat-file -p`); untracked or dirty worktree copies are ignored. Every listed SHA must resolve to a commit in the repository. Bound in `crovia.seal.v1` `generator.params`. |
| External `git-ai` notes ref | `coverage.notes_oid` | `git rev-parse --verify --quiet refs/notes/ai` (40-hex OID or `null`). Bound in `crovia.seal.v1` `generator.params` because `refs/notes/ai` lives outside the commit object. |
| Shallow boundary | `coverage.shallow` | Refused by default (exit 2); allowed only with `--allow-shallow`. |

### Structural limits that Method v6 states and does not claim to solve

1. **Formal definition of `surviving`, copy detection (`-C`), and `copy_capped_lines`**:
   - For each non-merge commit $c$, `introduced` is the number of added lines in
     non-generated tracked paths from `git log --no-merges --numstat -M`, and
     `owned_at_head` is the number of lines at `HEAD` in non-generated tracked
     paths attributed to $c$ by `git blame --line-porcelain -w -M -C`.
   - `surviving` is defined as $\text{surviving}(c) = \min(\text{owned\_at\_head}(c),\; \text{introduced}(c))$.
   - `coverage.copy_capped_lines` is defined as $\sum_{c \in \text{VERIFIED}} \max(0,\; \text{owned\_at\_head}(c) - \text{introduced}(c))$.
   - **Critical interpretation rule**: `copy_capped_lines == 0` means **only**
     that no `VERIFIED` commit exceeded its `introduced` count at `HEAD`. It must
     **never** be interpreted as absence of copied or duplicated lines: when
     `owned_at_head <= introduced`, lines copied into a second file modified in
     the same commit (`-C`) can compensate for deleted lines of the same commit.
     `surviving` is therefore a **per-commit capped attribution count at `HEAD`**,
     not a count of distinct original lines retained.
2. **Committer timestamps in `baseline.by_age`**: age buckets (`0-30 d`, `30-90 d`, …)
   measure `HEAD.committed_at - commit.committed_at`. A history rewrite (`git rebase`,
   `git filter-repo`) that resets committer dates changes `HEAD` and shifts age
   buckets even when tree contents are unchanged.
3. **Evidence Diff (`re audit --diff <BEFORE> <AFTER>`)**: separates
   `repository_diff` (different `HEAD`, same method and flags, non-empty
   `shared_root_commits`), `method_diff` (same `HEAD`, different method or flags),
   and `metadata_diff` (same `HEAD` and method, different `notes_oid` or
   `ignore_revs_sha256`).
   - **Offline commit-set relation (`commit_set_relation`)**: because `--diff`
     operates offline on two receipts without querying a Git repository, it
     reports `commit_set_relation` (`same_head`, `subset_forward`,
     `subset_reverse`, `same_commit_set`, `diverged`, `unknown`) computed over
     the serialized `--no-merges` commit sets $B$ and $A$. It does **not** claim
     to prove Git DAG ancestry (`git merge-base --is-ancestor`).
   - **Provenance tripartition (`provenance`)**: distinguishes (a) cryptographic
     validity (`before_signature_valid`, `after_signature_valid`), (b) issuer key
     concordance (`status`: `unsealed`, `partially_sealed`, `sealed_same_issuer`,
     `sealed_different_issuers`, plus `before_pubkey_hex` / `after_pubkey_hex`),
     and (c) issuer trust (`issuer_trust: "unverified_offline"` — a valid
     Ed25519 signature proves the holder of `pubkey_hex` signed those bytes, not
     that the issuer is trusted without an external trust root). Commit lists in
     a diff are set differences ($B \setminus A$ and $A \setminus B$), not a
     causal attribution.
   - **Single-axis rule and missing-field semantics (`metadata_status`)**:
     `metadata_status` is `"unchanged"` only when both receipts explicitly
     record `coverage.notes_oid` and `coverage.ignore_revs_sha256` (or both
     record `ignore_revs_file == false`) and the values are identical;
     `"changed"` when any recorded metadata field differs; and `"indeterminate"`
     when a field is absent on at least one side (such as an authentic Method v4
     receipt, which did not record `coverage.notes_oid`). A missing field is
     **never** treated as proof that external metadata stayed unchanged. When
     `HEAD` is identical, method/flags differ, and
     `metadata_status == "indeterminate"`, `kind` is reported as
     `"indeterminate_method_diff"` rather than a pure `"method_diff"`.
