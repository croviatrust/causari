# X thread (7 posts, each ≤ 280 characters)

## 1

Everyone argues about how much code AI writes. Nobody can check the numbers.

I built a command that counts one thing from plain git history: how many lines from AI-tagged commits are still at HEAD.

re audit. Any repo, no account, no upload.

https://causari.dev

## 2

How it works: it finds commits carrying machine-readable AI authorship metadata (Co-Authored-By naming an agent, bot authors, AI-* trailers, git-ai notes) and asks git blame -w -M -C how many of the lines they introduced are still there.

No model, no estimate, no survey.

## 3

First weekly Survival Report, 10 open-source repos: 462,838 of 754,476 lines from 12,349 AI-tagged commits are still at HEAD. 61.3 %, 95 % bootstrap interval over that sample 53.0 % to 64.9 %.

Those 10 repos. Not "AI code".

https://causari.dev/reports/survival/2026/01/

## 4

What it cannot see: inline completions (Copilot, Cursor Tab, Windsurf) leave no git trace and are not measured; that absence is not a finding that a human wrote them. Untagged commits are never counted. A rewritten line is a death even if the meaning is unchanged.

A count, not a grade. Limits first: https://causari.dev/method

## 5

re audit --json prints the exact bytes behind every published row. Every row of the report names the command that reproduces it. If your reproduction differs, open an issue with the JSON; corrections are made in public.

## 6

re audit --seal signs the audit JSON, bound to the commit and method version. re seal verify checks it offline; so does https://causari.dev/verify in the browser, no network request.

A valid seal proves the numbers were not altered, not that they are true. Rerun and compare.

## 7

One Rust binary, ~5 MB, Linux/macOS/Windows. Apache-2.0. A GitHub Action posts the count on every PR. Maintainers opt out of the report with one line.

curl -fsSL https://causari.dev/install.sh | sh
re audit

https://github.com/croviatrust/causari
