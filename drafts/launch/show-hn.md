# Show HN

## Title

Show HN: Causari – count how many lines from AI-tagged commits survive in git

## URL

https://causari.dev

## Body

AI-written code has no author. It has causes. Causari proves them.

I wrote `re audit` because every number about AI in code I could find was a self-report: a vendor's percentage, a survey, a dashboard. None could be re-run by someone else.

`re audit` reads plain git history. It finds commits carrying machine-readable AI authorship metadata (Co-Authored-By trailers naming an agent, bot authors, AI-* trailers, git-ai notes) and asks `git blame -w -M -C` how many of the lines they introduced are still at HEAD. One static Rust binary, any full clone, no account, no upload. `--json` gives the exact bytes; anyone re-runs it and gets the same bytes.

The first weekly Survival Report measured 10 open-source repositories: 462,838 of 754,476 lines from 12,349 AI-tagged commits are still at HEAD, 61.3 %, 95 % bootstrap interval over that sample 53.0 % to 64.9 %. It describes those 10 repositories, not AI code in general.

What it cannot see: inline completions (Copilot, Cursor Tab, Windsurf) leave no git trace and are not measured; that absence is not a finding that a human wrote them. Commits without a trailer are never counted. A rewritten line is a death even if the meaning is unchanged. It is a count, not a grade.

`re audit --seal` signs the audit JSON, bound to the commit and method version. `re seal verify` checks it offline; so does https://causari.dev/verify with no network request. A valid seal proves the numbers were not altered, not that they are true. Rerun and compare.

Method and the known ways the number misleads: https://causari.dev/method. Apache-2.0: https://github.com/croviatrust/causari

Which commits does it misclassify in your repositories?
