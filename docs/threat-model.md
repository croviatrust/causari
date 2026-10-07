# Threat model

What Causari defends against, what it does not, and the assumption behind
each claim. Written so that a claim on the site or in the README can be
traced to a line here; a claim that has no line here should not be made.

## Assets

1. **The measurement** — `re audit` counts: which commits are AI-tagged,
   how many of their lines are still at HEAD.
2. **The record** — the ledger under `.causari/`: prompts, completions,
   commands, snapshots, events; who did what, from what, and why.
3. **The receipts** — Crovia Seals over audits and exchanges; PNX sheets
   and proofs.
4. **The published reports** — the Survival Report series, its bytes, DOIs
   and repository pages.

## Actors

| Actor | Can | Wants |
|---|---|---|
| Repository author | write any commit metadata, rewrite history, add `.git-blame-ignore-revs` | a better-looking number |
| Agent runtime | report anything through hooks and MCP; omit events | a cleaner story than what happened |
| Local process | read and write `.causari/`, send traffic through the loopback proxy | the record, the keys, or a forged entry |
| Reader of a report | contest a number | the truth, or a reason to dismiss the report |
| Publisher (us) | build and sign reports, hold the Zenodo account | credibility; must not be able to alter a published number quietly |

## The measurement

**Claim.** Every number is reproducible by anyone from public git history
with one command, and the report deposits the exact bytes it counted.

**Defended.** Editing a number after the fact: the bytes are on the page,
on Zenodo under a DOI, and reproducible; a revision keeps the superseded
bytes (`report.r<K>.json`) and states the change. Counting one repository
twice under two names: audits that measured the same commit or are
byte-identical count once (`excluded.duplicates`). Shallow clones: refused
by method v2. Large bulk commits dominating a ratio: the per-commit cap and
the capped ratio, both published. Small samples: measured but not
aggregated below the sample floor. A repository cleared or rewritten, so
that no old line survives whoever wrote it: method v3 names the oldest
surviving line and counts the commits behind it, and puts the untagged
lines from the matched age windows next to the AI-tagged ones.

**Not defended, by design.** A repository author who wants a higher or
lower number can tag or untag commits: the audit reads metadata, it does
not detect AI-written code. Untagged agent commits and inline completions
are invisible; the report says so in every positioning block. A rewrite of
history changes what is measured; the `repository.head` in the audit JSON
says which commit was measured, nothing more. `refs/notes/ai` is not part
of the commit object: the SHA can stay the same while the note, and
therefore a v4 classification, changes. Blame heuristics (`-w -M
-C`) are stated and can misattribute moved code; the method page lists the
known artefacts. Method v3 counted any parseable git-ai note; method v4
counts one only when it names a non-empty tool. Published v3 reports are
not recomputed.

**Assumption.** GitHub serves the same history to us and to the reader.

## The record

**Claim.** The ledger records what the runtime reported, when, against a
snapshot of the tree, on this machine only; a pasted credential in a
recognisable format does not reach disk in clear.

**Defended.** Network exfiltration: nothing is sent anywhere; the proxy
forwards to the upstream the client asked for and adds no credential of
its own. Credential leakage into the record: prefix-anchored redaction of
API keys, platform tokens, bearer values, JWTs and private-key blocks
before every write, with a `redactions` count on the record. Header
leakage: `Authorization`, `x-api-key` and cookies are forwarded and never
written; full bodies are hashed, not stored. Key theft by other users of
the machine: signing keys and the PNX log are `0600`. Torn objects: write
to a temporary file, rename.

**Not defended.** A **lying runtime**: a hook or MCP client can declare a
prompt, a model or a file list that is not what happened. Every event
carries its evidence class (`declared`, `observed`, or `correlated` with a
score) so the reader knows what kind of statement it is; no event is presented as
proven. A **local attacker** with write access to `.causari/` can append,
delete or rewrite events and issue seals with the repository's key; the
ledger is not tamper-evident against its owner (there is no external
anchor for the event chain today; anchoring by TACET is on the roadmap).
**Secrets without a recognisable shape** (bare passwords, home-grown
tokens) are stored as typed. **Secrets inside tracked files** are
snapshotted like any other bytes; `.env*` is excluded, nothing else is
scanned. **Other local processes** can use the loopback proxy and their
exchanges are recorded next to yours.

**Assumption.** The machine's user account is not shared with an
adversary; the runtime is honest about what it reports, or at least the
reader knows it may not be.

## The receipts

**Claim.** A valid seal proves that the named key signed these exact bytes
(an audit JSON, or the hashes of one exchange) at that point in its chain,
and that they have not been altered since. Verification needs the file
alone.

**Defended.** Alteration after signing; substitution of one audit's
numbers for another's (the seal binds commit, method and repository
label); replay of a seal against a different commit.

**Not defended.** The seal does not prove the numbers are true (rerun the
audit), nor who controls the key (the seal names it; trust is the
reader's). Chain gaps are visible only to someone who holds the
neighbouring seals; there is no public log of issued seals. Key compromise
lets the holder issue seals in that name; there is no revocation.

**Assumption.** Ed25519 and SHA-256 hold; the reader verifies offline with
`re seal verify` or the browser page, not by trusting the page that shows
the number.

## The published reports

**Claim.** A published number cannot be changed without the change being
visible.

**Defended.** Each report is a numbered directory, append-only, with a DOI
minted over its bytes; a correction is a new revision that freezes the
previous `report.json`, states the change on the page, in the feed and in
the Zenodo record, and gets a new version DOI under the same concept. The
generator refuses to rebuild a number in place.

**Not defended.** We hold the Zenodo account and the repository: an
administrator can force-push. Branch protection with required checks and
the DOI on independent infrastructure make this visible, not impossible. A
reader who wants certainty compares the deposited bytes on Zenodo with the
page.

**Assumption.** Zenodo keeps published versions immutable, as its policy
states.

## Out of scope

Supply chain of the binary beyond the sha256 check in `install.sh` and the
SLSA build provenance attached to each release; denial of service against the loopback proxy; timing side
channels; the security of the agent runtimes and model providers
themselves.

## Changes to this document

This file changes when a defence is added or a claim is withdrawn; the
CHANGELOG entry names the section. Last revised 2026-10-04.
