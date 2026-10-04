# r/programming

Self-promotion norms of the subreddit: the post is about the method, not
the product; the maintainer is named as the author in the first line;
criticism is asked for explicitly; no repost, no cross-post, answer every
comment.

## Title

Counting AI-written code survival from git metadata alone: method, limits, first numbers

## Body (≤ 200 words)

Author here. I wanted a number about AI-written code that anyone could re-run, so I wrote a tool that uses nothing but git history.

Method: walk `git log --no-merges`, classify each commit from its trailers and author identity only (Co-Authored-By naming an agent, bot authors, AI-* and Assisted-by trailers, git-ai notes). No model, no heuristic on the diff. For those commits, count the lines they added, then run `git blame -w -M -C` at HEAD and count how many lines still belong to them. Cap each commit's weight at the repository's 95th percentile so one bulk commit cannot dominate; publish no ratio below 5 tagged commits.

First weekly report, 10 open-source repositories: 462,838 of 754,476 lines from 12,349 AI-tagged commits still at HEAD, 61.3 %, 95 % bootstrap interval over that sample 53.0 % to 64.9 %.

Limits: inline completions leave no git trace and are not measured; that absence is not a finding that a human wrote them. Untagged commits are never counted; a rewritten line is a death even if the meaning is unchanged; a surviving line may be dead code. It is a count, not a grade.

Method with the known ways the number misleads: https://causari.dev/method. Code, Apache-2.0: https://github.com/croviatrust/causari

Where is the method wrong?
