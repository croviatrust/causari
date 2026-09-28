# Survival Report #3 — 2026-09-28

Counts of surviving lines from AI-tagged commits in 43 open-source repositories, measured with causari 0.3.0, method v3. Counts, not grades: no rank, no verdict; rows are alphabetical.

Page: https://causari.dev/reports/survival/2026/03/  
Data: https://causari.dev/reports/survival/2026/03/report.json  
Feed: https://causari.dev/reports/survival/feed.xml  
Licence: CC-BY-4.0  

## Aggregate

13,068,031 of 21,286,167 lines introduced by 39,706 AI-tagged commits in 43 open-source repositories are still at HEAD (61.4 %). 95 % interval over the sampled repositories: 40.6 % to 76.5 %.

- Repositories aggregated: 43
- Commits in those repositories (no merges): 575,311
- AI-tagged (VERIFIED) commits: 39,706
- Lines introduced by them: 21,286,167
- Still attributed to them at HEAD: 13,068,031
- Line-weighted ratio: 61.4 %
- 95 % bootstrap interval over the sampled repositories: 40.6 % to 76.5 %
- Median of per-repository capped ratios: 73.9 %
- 95 % bootstrap interval on that median: 67.6 % to 76.2 %

95 % percentile interval from 2000 bootstrap resamples of the 43 aggregated repositories (with replacement, seed 3). It describes the sampled repositories, not all AI-assisted code, and not the repositories not in this sample.


## Baseline: the same repositories' untagged lines, at the same age

gap = AI-tagged line-weighted survival minus untagged survival re-weighted to the age mix of the AI-tagged lines of the same repository, over age windows where both cohorts hold at least 5 commits, computed inside each repository; untagged = commits with no machine-readable AI signal (human-written, inline-completed and untagged-agent code alike); age = commit date to HEAD date. A negative gap means AI-tagged lines survive less than untagged lines of the same age in the same repository.

- Repositories with an age-matched gap: 42 of 43 with a baseline
- Median gap across them: +4.6 pts
- 95 % bootstrap interval on that median: -0.9 pts to +7.0 pts
- Gaps below zero: 18 · above zero: 24
- Cleared or rewritten (more than 50% of the repository's commits predate the oldest line still at HEAD): OpenHands/OpenHands. Nothing from before that date survives in them, tagged or not; their rows measure the rewrite as much as the code.

Age windows, counts summed across the repositories with a baseline, one row per age window; one large repository can dominate a window, so no gap is computed from these rows: the gap is computed inside each repository and only its median crosses repositories.

| Line age | AI-tagged commits | AI-tagged lines | Still at HEAD | AI-tagged | Untagged commits | Untagged lines | Still at HEAD | Untagged |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0–30 d | 6,256 | 3,906,783 | 3,229,939 | 82.7 % | 15,776 | 8,699,442 | 7,218,325 | 83.0 % |
| 30–90 d | 5,911 | 3,909,985 | 2,897,480 | 74.1 % | 25,908 | 15,454,516 | 11,921,878 | 77.1 % |
| 90–180 d | 8,254 | 8,807,386 | 4,011,955 | 45.6 % | 37,488 | 14,666,620 | 8,391,656 | 57.2 % |
| 180–365 d | 6,545 | 3,707,005 | 2,544,042 | 68.6 % | 67,322 | 17,490,150 | 8,908,668 | 50.9 % |
| 365–730 d | 9,652 | 885,129 | 351,427 | 39.7 % | 91,157 | 14,685,995 | 6,029,789 | 41.1 % |
| 730+ d | 3,088 | 69,879 | 33,188 | 47.5 % | 297,898 | 53,147,979 | 11,974,767 | 22.5 % |

## Repositories (alphabetical)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Line-weighted | Capped | Median per commit | Largest commit | Untagged, same age | Gap | Reproduce |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Aider-AI/aider | 12,461 | 11,156 | 374,687 | 235,782 | 62.9 % | 41.9 % | 33.3 % | 27 % | 52.7 % | +10.3 pts | `re audit Aider-AI/aider --json` |
| anthropics/anthropic-sdk-python | 1,408 | 39 | 4,961 | 4,116 | 83.0 % | 79.7 % | 83.3 % | 32 % | 87.6 % | -4.1 pts | `re audit anthropics/anthropic-sdk-python --json` |
| anthropics/anthropic-sdk-typescript | 1,334 | 50 | 14,219 | 11,877 | 83.5 % | 85.6 % | 96.4 % | 39 % | 89.9 % | -1.0 pts | `re audit anthropics/anthropic-sdk-typescript --json` |
| anthropics/claude-code | 740 | 71 | 31,257 | 29,225 | 93.5 % | 74.6 % | 47.2 % | 70 % | 79.8 % | +13.7 pts | `re audit anthropics/claude-code --json` |
| BerriAI/litellm | 43,551 | 4,967 | 1,509,602 | 938,291 | 62.2 % | 67.6 % | 68.4 % | 5 % | 54.9 % | +7.2 pts | `re audit BerriAI/litellm --json` |
| browser-use/browser-use | 7,003 | 262 | 19,773 | 4,283 | 21.7 % | 28.9 % | 12.7 % | 19 % | 48.4 % | -26.7 pts | `re audit browser-use/browser-use --json` |
| cline/cline | 7,363 | 155 | 95,924 | 63,756 | 66.5 % | 68.1 % | 72.7 % | 12 % | 53.7 % | +12.8 pts | `re audit cline/cline --json` |
| cloudflare/workers-sdk | 8,471 | 194 | 88,939 | 70,205 | 78.9 % | 77.3 % | 72.1 % | 8 % | 68.5 % | +10.4 pts | `re audit cloudflare/workers-sdk --json` |
| continuedev/continue | 16,266 | 228 | 27,756 | 18,063 | 65.1 % | 63.5 % | 68.1 % | 11 % | 58.4 % | +6.7 pts | `re audit continuedev/continue --json` |
| cozystack/cozystack | 4,971 | 2,247 | 528,186 | 406,831 | 77.0 % | 77.2 % | 84.5 % | 7 % | 81.1 % | -4.0 pts | `re audit cozystack/cozystack --json` |
| crewAIInc/crewAI | 2,815 | 192 | 3,342,690 | 148,579 | 4.4 % | 75.7 % | 90.0 % | 97 % | 64.4 % | -59.9 pts | `re audit crewAIInc/crewAI --json` |
| croviatrust/causari | 225 | 14 | 6,267 | 5,172 | 82.5 % | 82.5 % | 92.7 % | 25 % | 82.1 % | +3.3 pts | `re audit croviatrust/causari --json` |
| danny-avila/LibreChat | 5,690 | 88 | 84,488 | 78,512 | 92.9 % | 90.4 % | 88.2 % | 24 % | 87.6 % | +5.3 pts | `re audit danny-avila/LibreChat --json` |
| fastrepl/anarlog | 9,598 | 1,402 | 749,201 | 471,060 | 62.9 % | 44.3 % | 9.1 % | 16 % | 51.6 % | +11.3 pts | `re audit fastrepl/anarlog --json` |
| flutter/flutter | 89,205 | 152 | 85,088 | 17,532 | 20.6 % | 66.7 % | 86.5 % | 39 % | 38.3 % | -17.7 pts | `re audit flutter/flutter --json` |
| flutter/website | 8,542 | 149 | 49,836 | 47,422 | 95.2 % | 82.1 % | 94.9 % | 66 % | 66.2 % | +28.9 pts | `re audit flutter/website --json` |
| ghostty-org/ghostty | 13,500 | 80 | 3,768 | 2,611 | 69.3 % | 73.6 % | 87.0 % | 12 % | 50.6 % | +18.7 pts | `re audit ghostty-org/ghostty --json` |
| google-gemini/gemini-cli | 6,441 | 349 | 73,569 | 46,851 | 63.7 % | 62.3 % | 64.2 % | 4 % | 66.9 % | -4.6 pts | `re audit google-gemini/gemini-cli --json` |
| google/sam | 1,479 | 35 | 247 | 96 | 38.9 % | 42.2 % | 0.0 % | 71 % | 33.8 % | +5.0 pts | `re audit google/sam --json` |
| GoogleCloudPlatform/scion | 4,562 | 82 | 14,657 | 10,911 | 74.4 % | 73.9 % | 83.5 % | 16 % | 81.7 % | -7.2 pts | `re audit GoogleCloudPlatform/scion --json` |
| langchain-ai/langchain | 16,744 | 43 | 7,289 | 5,388 | 73.9 % | 69.8 % | 87.4 % | 24 % | 53.1 % | +17.3 pts | `re audit langchain-ai/langchain --json` |
| langgenius/dify | 13,698 | 569 | 645,898 | 373,921 | 57.9 % | 58.2 % | 59.4 % | 16 % | 47.1 % | +10.8 pts | `re audit langgenius/dify --json` |
| lidge-ai/cli-jaw | 5,681 | 1,359 | 445,333 | 197,637 | 44.4 % | 75.2 % | 68.2 % | 47 % | 53.9 % | -9.5 pts | `re audit lidge-ai/cli-jaw --json` |
| lidge-ai/ima2-gen | 1,974 | 452 | 127,182 | 46,747 | 36.8 % | 53.2 % | 60.0 % | 19 % | 57.4 % | -20.7 pts | `re audit lidge-ai/ima2-gen --json` |
| lobehub/lobe-chat | 13,956 | 1,979 | 2,240,779 | 1,941,644 | 86.7 % | 79.4 % | 81.7 % | 2 % | 81.5 % | +5.1 pts | `re audit lobehub/lobe-chat --json` |
| mem0ai/mem0 | 2,641 | 93 | 37,012 | 23,936 | 64.7 % | 64.9 % | 82.8 % | 9 % | 65.8 % | -4.8 pts | `re audit mem0ai/mem0 --json` |
| microsoft/vscode-copilot-chat | 3,713 | 336 | 995,871 | 939,159 | 94.3 % | 69.0 % | 76.3 % | 88 % | 55.7 % | +38.6 pts | `re audit microsoft/vscode-copilot-chat --json` |
| openai/codex | 11,511 | 364 | 183,908 | 109,291 | 59.4 % | 58.9 % | 58.4 % | 6 % | 54.7 % | +4.5 pts | `re audit openai/codex --json` |
| openai/openai-agents-python | 2,289 | 5 | 95,773 | 95,593 | 99.8 % | 98.5 % | 96.2 % | 98 % | no shared window | — | `re audit openai/openai-agents-python --json` |
| OpenHands/OpenHands · rewritten | 8,342 | 2,644 | 940,660 | 283,984 | 30.2 % | 28.3 % | 0.0 % | 14 % | 37.3 % | -7.1 pts | `re audit OpenHands/OpenHands --json` |
| OpenHands/software-agent-sdk | 2,430 | 1,682 | 472,771 | 375,367 | 79.4 % | 76.2 % | 77.9 % | 12 % | 79.9 % | -0.5 pts | `re audit OpenHands/software-agent-sdk --json` |
| pydantic/pydantic-ai | 3,578 | 333 | 682,096 | 592,763 | 86.9 % | 79.8 % | 77.6 % | 64 % | 82.3 % | +4.6 pts | `re audit pydantic/pydantic-ai --json` |
| QwenLM/qwen-code | 9,439 | 563 | 659,736 | 573,206 | 86.9 % | 82.6 % | 65.8 % | 21 % | 74.2 % | +12.7 pts | `re audit QwenLM/qwen-code --json` |
| ray-project/ray | 31,775 | 814 | 495,301 | 319,321 | 64.5 % | 78.6 % | 90.5 % | 29 % | 67.0 % | -2.5 pts | `re audit ray-project/ray --json` |
| richlander/dotnet-inspect | 5,438 | 4,025 | 3,990,432 | 3,053,584 | 76.5 % | 77.3 % | 79.8 % | 33 % | 76.6 % | -0.1 pts | `re audit richlander/dotnet-inspect --json` |
| RooCodeInc/Roo-Code | 6,210 | 43 | 88,016 | 39,395 | 44.8 % | 57.0 % | 62.6 % | 67 % | 45.6 % | -0.9 pts | `re audit RooCodeInc/Roo-Code --json` |
| run-llama/llama_index | 7,947 | 13 | 8,542 | 7,872 | 92.2 % | 92.2 % | 90.4 % | 73 % | 65.3 % | +18.0 pts | `re audit run-llama/llama_index --json` |
| secdev/scapy | 5,718 | 62 | 8,472 | 8,217 | 97.0 % | 94.2 % | 100.0 % | 29 % | 90.4 % | +2.1 pts | `re audit secdev/scapy --json` |
| sst/opencode | 15,732 | 50 | 25,998 | 9,466 | 36.4 % | 35.4 % | 39.5 % | 43 % | 42.7 % | -6.4 pts | `re audit sst/opencode --json` |
| vercel/next.js | 35,681 | 236 | 59,460 | 45,774 | 77.0 % | 74.3 % | 82.5 % | 8 % | 60.5 % | +16.5 pts | `re audit vercel/next.js --json` |
| vllm-project/llm-compressor | 3,234 | 154 | 17,210 | 13,816 | 80.3 % | 74.5 % | 85.3 % | 30 % | 75.4 % | +4.9 pts | `re audit vllm-project/llm-compressor --json` |
| youknowone/pyre | 84,965 | 1,857 | 1,905,484 | 1,375,998 | 72.2 % | 69.7 % | 72.5 % | 17 % | 61.9 % | +10.4 pts | `re audit youknowone/pyre --json` |
| zed-industries/zed | 36,990 | 118 | 47,829 | 24,777 | 51.8 % | 55.6 % | 69.1 % | 16 % | 70.2 % | -18.7 pts | `re audit zed-industries/zed --json` |

## By agent, across aggregated repositories (alphabetical)

| Agent | Repositories | Commits | Introduced | Still at HEAD | Line-weighted |
|---|---:|---:|---:|---:|---:|
| ai | 3 | 59 | 5,295 | 4,631 | 87.5 % |
| aider | 4 | 11,167 | 375,563 | 235,811 | 62.8 % |
| claude-code | 37 | 11,631 | 7,616,756 | 5,909,108 | 77.6 % |
| cursor | 27 | 1,205 | 4,298,144 | 769,932 | 17.9 % |
| devin | 10 | 4,472 | 1,090,190 | 671,543 | 61.6 % |
| gemini | 11 | 1,290 | 407,555 | 239,284 | 58.7 % |
| github-copilot | 31 | 4,714 | 5,650,529 | 4,247,013 | 75.2 % |
| grok | 1 | 70 | 66,959 | 52,329 | 78.2 % |
| jules | 8 | 38 | 3,534 | 2,112 | 59.8 % |
| llm | 1 | 432 | 100,575 | 75,418 | 75.0 % |
| openai-codex | 14 | 492 | 359,328 | 267,778 | 74.5 % |
| opencode | 1 | 3 | 3,189 | 2,842 | 89.1 % |
| openhands | 4 | 4,133 | 1,308,550 | 590,230 | 45.1 % |

## Measured but not aggregated (fewer than 5 AI-tagged commits)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Reproduce |
|---|---:|---:|---:|---:|---|
| openai/openai-python | 1,644 | 3 | 63 | 61 | `re audit openai/openai-python --json` |
| stackblitz/bolt.new | 101 | 0 | 0 | 0 | `re audit stackblitz/bolt.new --json` |

## Excluded from this report

- Shallow clones (history truncated; method v3 refuses them): none
- Audits that failed in this run: llvm/llvm-project
- Opted out by their maintainers (https://github.com/croviatrust/causari/blob/main/.github/survival-optout.txt): 0

## Method

Method v3, causari 0.3.0. Detection from commit metadata only; survival from `git blame -w -M -C` at HEAD. Per-commit cap: a commit weighs at most the 95th percentile of per-commit introduced line counts in its repository, and never more than 10,000 lines. Sample floor: 5 VERIFIED commits. VERIFIED only; PROBABLE is listed but never summed. Full clones only. Baseline (method v3): the untagged lines of the same repository, by age; the gap is defined in the Baseline section. Details, limits and how to contest a number: https://causari.dev/method.

## What this report is, and is not

This report counts lines. For each repository it states how many lines were introduced by commits that carry machine-readable AI authorship metadata (trailers such as Co-Authored-By naming an agent, bot author identities, aider markers, git-ai notes), and how many of those lines git blame still attributes to those commits at HEAD, under the method version stated on the page (blame with -w -M -C, a per-commit weight cap, a sample floor, full clones only; from method v3 the untagged lines of the same repository, at the same age, stand next to the AI-tagged ones). Every row is reproducible with one command.

It is not a quality judgement: deleted lines include removed features and rewritten prototypes; surviving lines include dead code. It is not a sample of all AI-assisted code: inline completions leave no trace in git, untagged agent commits are invisible, and the repositories were selected, not drawn at random: 30 hand-picked and 16 found by GitHub commit search as public repositories with at least 5 commits carrying the same AI authorship metadata and at least 100 stars, most-starred first (discovered 2026-09-27); the selection rule and the counts behind it are public. The intervals describe the sampled repositories only.

Prior measurement work asks related questions with different instruments. GitClear publishes churn reports built from code-change patterns across the repositories it analyses; arXiv 2601.16809 ("Will It Survive?") follows the modification of agent-authored code in 201 projects with its own detector and finds that such code is modified less often than human-written code. This report does not reproduce either method and does not adjudicate between them: it publishes counts from git metadata alone, with the method version, the tool version and the exact bytes behind every number, so that the three can be read side by side.

## Cite

Crovia Trust. Survival Report #3 (2026-09-28). https://causari.dev/reports/survival/2026/03/
