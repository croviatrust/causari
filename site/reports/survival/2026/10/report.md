# Survival Report #10 — 2026-11-16

Counts of surviving lines from AI-tagged commits in 59 open-source repositories, measured with causari 0.5.0, method v6. Counts, not grades: no rank, no verdict; rows are alphabetical.

Page: https://causari.dev/reports/survival/2026/10/  
Data: https://causari.dev/reports/survival/2026/10/report.json  
Feed: https://causari.dev/reports/survival/feed.xml  
Licence: CC-BY-4.0  

## Aggregate

22,605,946 of 44,078,314 lines introduced by 88,938 AI-tagged commits in 59 open-source repositories are still at HEAD (51.3 %). 95 % interval over the sampled repositories: 39.3 % to 68.3 %.

- Repositories aggregated: 59
- Commits in those repositories (no merges): 962,103
- AI-tagged (VERIFIED) commits: 88,938
- Lines introduced by them: 44,078,314
- Still attributed to them at HEAD: 22,605,946
- Line-weighted ratio: 51.3 %
- 95 % bootstrap interval over the sampled repositories: 39.3 % to 68.3 %
- Median of per-repository capped ratios: 73.7 %
- 95 % bootstrap interval on that median: 68.1 % to 76.3 %

95 % percentile interval from 2000 bootstrap resamples of the 59 aggregated repositories (with replacement, seed 10). It describes the sampled repositories, not all AI-assisted code, and not the repositories not in this sample.


## Baseline: the same repositories' untagged lines, in the matched age windows

gap = AI-tagged line-weighted survival minus untagged survival re-weighted to the age mix of the AI-tagged lines of the same repository, over age windows where both cohorts hold at least 5 commits, computed inside each repository; untagged = commits with no machine-readable AI signal (human-written, inline-completed and untagged-agent code alike); age = commit date to HEAD date. A negative gap means AI-tagged lines survive less than untagged lines in the matched age windows of the same repository.

- Repositories with an age-matched gap: 58 of 59 with a baseline
- Median gap across them: +1.5 pts
- 95 % bootstrap interval on that median: -0.9 pts to +4.9 pts
- Gaps below zero: 25 · above zero: 33
- Cleared or rewritten (more than 50% of the repository's commits predate the oldest line still at HEAD): OpenHands/OpenHands. Nothing from before that date survives in them, tagged or not; their rows measure the rewrite as much as the code.

Age windows, counts summed across the repositories with a baseline, one row per age window; one large repository can dominate a window, so no gap is computed from these rows: the gap is computed inside each repository and only its median crosses repositories.

| Line age | AI-tagged commits | AI-tagged lines | Still at HEAD | AI-tagged | Untagged commits | Untagged lines | Still at HEAD | Untagged |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0–30 d | 13,348 | 7,607,029 | 6,152,665 | 80.9 % | 21,138 | 36,539,563 | 32,735,997 | 89.6 % |
| 30–90 d | 26,140 | 15,799,076 | 7,204,841 | 45.6 % | 44,662 | 24,319,593 | 17,126,353 | 70.4 % |
| 90–180 d | 16,999 | 10,139,543 | 3,566,286 | 35.2 % | 50,398 | 59,062,933 | 42,789,797 | 72.4 % |
| 180–365 d | 18,107 | 8,074,541 | 4,199,589 | 52.0 % | 99,371 | 27,609,582 | 12,130,551 | 43.9 % |
| 365–730 d | 11,206 | 2,371,059 | 1,449,587 | 61.1 % | 137,590 | 22,481,482 | 10,609,195 | 47.2 % |
| 730+ d | 3,138 | 87,066 | 32,978 | 37.9 % | 519,825 | 85,927,342 | 20,029,827 | 23.3 % |

## Repositories (alphabetical)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Line-weighted | Capped | Median per commit | Largest commit | Untagged, matched windows | Gap | Reproduce |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Aider-AI/aider | 12,461 | 11,156 | 374,687 | 234,788 | 62.7 % | 41.6 % | 32.4 % | 27 % | 52.7 % | +10.0 pts | `re audit Aider-AI/aider --json` |
| airbytehq/airbyte | 56,741 | 3,251 | 1,316,124 | 562,489 | 42.7 % | 74.4 % | 85.5 % | 38 % | 33.5 % | +9.3 pts | `re audit airbytehq/airbyte --json` |
| anthropics/anthropic-sdk-python | 1,731 | 75 | 23,015 | 20,815 | 90.4 % | 81.7 % | 86.0 % | 61 % | 86.3 % | +4.5 pts | `re audit anthropics/anthropic-sdk-python --json` |
| anthropics/anthropic-sdk-typescript | 1,674 | 92 | 32,122 | 28,674 | 89.3 % | 84.6 % | 96.6 % | 48 % | 87.5 % | +4.9 pts | `re audit anthropics/anthropic-sdk-typescript --json` |
| anthropics/claude-code | 883 | 83 | 32,156 | 1,817 | 5.7 % | 20.3 % | 2.0 % | 68 % | 65.9 % | -60.3 pts | `re audit anthropics/claude-code --json` |
| apache/fluss | 2,469 | 77 | 30,959 | 24,814 | 80.2 % | 80.0 % | 87.5 % | 6 % | 80.7 % | -0.7 pts | `re audit apache/fluss --json` |
| apache/spark | 49,528 | 19 | 1,379 | 1,189 | 86.2 % | 86.2 % | 94.1 % | 19 % | 80.5 % | +11.1 pts | `re audit apache/spark --json` |
| apache/texera | 7,500 | 600 | 234,980 | 183,301 | 78.0 % | 80.8 % | 88.2 % | 7 % | 78.9 % | -0.9 pts | `re audit apache/texera --json` |
| BerriAI/litellm | 46,970 | 6,735 | 2,916,139 | 1,907,422 | 65.4 % | 69.3 % | 67.2 % | 3 % | 59.7 % | +5.7 pts | `re audit BerriAI/litellm --json` |
| browser-use/browser-use | 7,603 | 803 | 59,298 | 33,282 | 56.1 % | 57.1 % | 75.0 % | 6 % | 59.2 % | -3.1 pts | `re audit browser-use/browser-use --json` |
| cfug/flutter.cn | 10,588 | 149 | 68,543 | 55,321 | 80.7 % | 73.0 % | 86.0 % | 48 % | 65.4 % | +15.3 pts | `re audit cfug/flutter.cn --json` |
| cline/cline | 7,673 | 165 | 122,815 | 88,578 | 72.1 % | 72.2 % | 74.0 % | 11 % | 56.7 % | +15.4 pts | `re audit cline/cline --json` |
| cloudflare/workers-sdk | 8,924 | 230 | 97,609 | 77,435 | 79.3 % | 78.1 % | 72.9 % | 7 % | 68.1 % | +11.2 pts | `re audit cloudflare/workers-sdk --json` |
| continuedev/continue | 16,266 | 228 | 27,756 | 17,961 | 64.7 % | 63.4 % | 66.8 % | 11 % | 57.7 % | +7.0 pts | `re audit continuedev/continue --json` |
| cozystack/cozystack | 6,202 | 3,432 | 707,433 | 552,008 | 78.0 % | 78.2 % | 84.8 % | 5 % | 80.1 % | -3.5 pts | `re audit cozystack/cozystack --json` |
| crewAIInc/crewAI | 3,117 | 319 | 3,403,059 | 202,159 | 5.9 % | 81.2 % | 89.4 % | 95 % | 72.2 % | -66.3 pts | `re audit crewAIInc/crewAI --json` |
| croviatrust/causari | 307 | 14 | 6,267 | 4,809 | 76.7 % | 76.7 % | 90.3 % | 25 % | 81.7 % | -4.1 pts | `re audit croviatrust/causari --json` |
| danny-avila/LibreChat | 6,798 | 93 | 84,743 | 77,987 | 92.0 % | 88.8 % | 86.0 % | 24 % | 85.2 % | +6.8 pts | `re audit danny-avila/LibreChat --json` |
| fastrepl/anarlog | 9,970 | 1,545 | 861,085 | 550,948 | 64.0 % | 49.5 % | 15.2 % | 14 % | 59.1 % | +4.8 pts | `re audit fastrepl/anarlog --json` |
| flutter/flutter | 90,085 | 164 | 87,348 | 18,599 | 21.3 % | 64.7 % | 83.0 % | 38 % | 40.1 % | -18.8 pts | `re audit flutter/flutter --json` |
| flutter/website | 8,641 | 158 | 50,660 | 43,798 | 86.5 % | 78.3 % | 87.5 % | 65 % | 85.8 % | +0.7 pts | `re audit flutter/website --json` |
| ghostty-org/ghostty | 14,110 | 108 | 5,858 | 4,134 | 70.6 % | 73.1 % | 84.0 % | 8 % | 51.2 % | +19.3 pts | `re audit ghostty-org/ghostty --json` |
| github/gh-aw-mcpg | 6,129 | 5,618 | 483,758 | 264,577 | 54.7 % | 61.4 % | 69.2 % | 3 % | 34.9 % | +19.8 pts | `re audit github/gh-aw-mcpg --json` |
| google-gemini/gemini-cli | 6,619 | 354 | 76,440 | 49,827 | 65.2 % | 63.9 % | 65.1 % | 4 % | 73.1 % | -8.8 pts | `re audit google-gemini/gemini-cli --json` |
| google/sam | 2,014 | 41 | 264 | 101 | 38.3 % | 47.3 % | 0.0 % | 66 % | 60.4 % | -4.9 pts | `re audit google/sam --json` |
| GoogleCloudPlatform/scion | 7,421 | 87 | 24,177 | 19,774 | 81.8 % | 73.7 % | 76.4 % | 35 % | 81.6 % | +0.1 pts | `re audit GoogleCloudPlatform/scion --json` |
| jdubois/boot-ui | 2,233 | 1,747 | 1,149,602 | 835,367 | 72.7 % | 80.3 % | 83.2 % | 10 % | 76.3 % | -3.6 pts | `re audit jdubois/boot-ui --json` |
| langchain-ai/langchain | 17,097 | 43 | 7,289 | 5,375 | 73.7 % | 69.6 % | 87.4 % | 24 % | 59.4 % | +14.3 pts | `re audit langchain-ai/langchain --json` |
| langgenius/dify | 15,256 | 715 | 1,064,860 | 633,942 | 59.5 % | 47.5 % | 53.2 % | 31 % | 47.9 % | +11.6 pts | `re audit langgenius/dify --json` |
| lidge-ai/cli-jaw | 5,762 | 1,361 | 445,819 | 195,580 | 43.9 % | 74.5 % | 66.7 % | 47 % | 66.2 % | -22.3 pts | `re audit lidge-ai/cli-jaw --json` |
| lidge-ai/ima2-gen | 2,116 | 455 | 127,661 | 46,566 | 36.5 % | 52.6 % | 59.9 % | 19 % | 74.7 % | -38.2 pts | `re audit lidge-ai/ima2-gen --json` |
| lobehub/lobe-chat | 16,024 | 3,032 | 3,933,346 | 1,969,936 | 50.1 % | 68.1 % | 81.1 % | 1 % | 48.2 % | +1.9 pts | `re audit lobehub/lobe-chat --json` |
| managarm/managarm | 7,005 | 1,117 | 80,138 | 62,677 | 78.2 % | 77.8 % | 88.2 % | 4 % | 77.1 % | +1.1 pts | `re audit managarm/managarm --json` |
| mem0ai/mem0 | 2,655 | 95 | 37,062 | 23,851 | 64.4 % | 64.5 % | 82.8 % | 9 % | 72.9 % | -8.4 pts | `re audit mem0ai/mem0 --json` |
| microsoft/BCApps | 4,683 | 919 | 574,659 | 482,633 | 84.0 % | 79.7 % | 85.1 % | 19 % | 80.6 % | +3.4 pts | `re audit microsoft/BCApps --json` |
| microsoft/vscode | 153,341 | 10,605 | 3,955,629 | 2,998,844 | 75.8 % | 73.2 % | 76.9 % | 22 % | 65.4 % | +10.4 pts | `re audit microsoft/vscode --json` |
| microsoft/vscode-copilot-chat | 3,713 | 336 | 995,871 | 938,654 | 94.3 % | 69.0 % | 76.3 % | 88 % | 55.7 % | +38.6 pts | `re audit microsoft/vscode-copilot-chat --json` |
| NovaSky-AI/SkyRL | 1,481 | 516 | 189,364 | 153,498 | 81.1 % | 80.0 % | 85.4 % | 3 % | 66.2 % | +14.9 pts | `re audit NovaSky-AI/SkyRL --json` |
| NVIDIA-NeMo/Switchyard | 758 | 28 | 15,791 | 10,517 | 66.6 % | 67.2 % | 76.4 % | 20 % | 63.4 % | +2.2 pts | `re audit NVIDIA-NeMo/Switchyard --json` |
| openai/codex | 13,651 | 364 | 183,908 | 106,766 | 58.1 % | 57.4 % | 56.6 % | 6 % | 47.9 % | +10.1 pts | `re audit openai/codex --json` |
| openai/openai-agents-python | 2,465 | 6 | 97,483 | 88,563 | 90.8 % | 89.7 % | 86.7 % | 97 % | no shared window | — | `re audit openai/openai-agents-python --json` |
| OpenHands/extensions | 471 | 367 | 128,718 | 98,448 | 76.5 % | 76.1 % | 79.9 % | 9 % | 77.7 % | -1.2 pts | `re audit OpenHands/extensions --json` |
| OpenHands/OpenHands · rewritten | 9,112 | 3,002 | 1,123,053 | 402,985 | 35.9 % | 33.6 % | 0.0 % | 12 % | 32.7 % | +3.2 pts | `re audit OpenHands/OpenHands --json` |
| OpenHands/software-agent-sdk | 3,030 | 2,003 | 648,571 | 510,990 | 78.8 % | 75.0 % | 76.4 % | 9 % | 78.6 % | +0.2 pts | `re audit OpenHands/software-agent-sdk --json` |
| PrefectHQ/prefect | 18,813 | 1,870 | 493,017 | 399,579 | 81.0 % | 79.8 % | 89.2 % | 3 % | 77.3 % | +3.8 pts | `re audit PrefectHQ/prefect --json` |
| pydantic/pydantic-ai | 5,632 | 417 | 744,284 | 554,565 | 74.5 % | 76.3 % | 76.7 % | 58 % | 79.7 % | -5.2 pts | `re audit pydantic/pydantic-ai --json` |
| QwenLM/qwen-code | 11,761 | 609 | 698,284 | 571,707 | 81.9 % | 76.8 % | 58.1 % | 20 % | 73.3 % | +8.5 pts | `re audit QwenLM/qwen-code --json` |
| ray-project/ray | 32,489 | 1,218 | 783,107 | 525,206 | 67.1 % | 78.5 % | 90.3 % | 18 % | 69.9 % | -2.9 pts | `re audit ray-project/ray --json` |
| RooCodeInc/Roo-Code | 6,210 | 43 | 88,016 | 39,395 | 44.8 % | 57.0 % | 62.6 % | 67 % | 45.3 % | -0.6 pts | `re audit RooCodeInc/Roo-Code --json` |
| run-llama/llama_index | 7,971 | 15 | 8,543 | 7,872 | 92.1 % | 92.1 % | 95.4 % | 73 % | 83.8 % | -0.3 pts | `re audit run-llama/llama_index --json` |
| secdev/scapy | 5,799 | 87 | 10,064 | 9,543 | 94.8 % | 90.7 % | 100.0 % | 24 % | 91.8 % | -2.7 pts | `re audit secdev/scapy --json` |
| smithersai/smithers | 25,652 | 18,102 | 11,167,594 | 3,125,571 | 28.0 % | 36.3 % | 0.2 % | 30 % | 33.4 % | -5.4 pts | `re audit smithersai/smithers --json` |
| sst/opencode | 22,639 | 57 | 26,347 | 1,911 | 7.3 % | 5.7 % | 0.0 % | 42 % | 14.2 % | -12.0 pts | `re audit sst/opencode --json` |
| TracecatHQ/tracecat | 6,298 | 738 | 543,193 | 378,979 | 69.8 % | 67.9 % | 69.0 % | 4 % | 72.0 % | -2.2 pts | `re audit TracecatHQ/tracecat --json` |
| vercel/next.js | 36,624 | 287 | 76,604 | 59,222 | 77.3 % | 74.9 % | 80.6 % | 6 % | 60.8 % | +16.5 pts | `re audit vercel/next.js --json` |
| vllm-project/llm-compressor | 3,361 | 174 | 28,324 | 22,985 | 81.2 % | 74.8 % | 84.8 % | 19 % | 75.1 % | +6.0 pts | `re audit vllm-project/llm-compressor --json` |
| xing-shuyin/pi-web-ui | 1,564 | 36 | 4,149 | 3,203 | 77.2 % | 74.4 % | 76.2 % | 25 % | 50.9 % | +26.5 pts | `re audit xing-shuyin/pi-web-ui --json` |
| youknowone/pyre | 86,321 | 2,828 | 3,464,171 | 2,283,744 | 65.9 % | 63.6 % | 65.7 % | 9 % | 70.0 % | -4.1 pts | `re audit youknowone/pyre --json` |
| zed-industries/zed | 37,692 | 150 | 55,119 | 30,665 | 55.6 % | 62.3 % | 73.8 % | 14 % | 69.0 % | -13.4 pts | `re audit zed-industries/zed --json` |

## By agent, across aggregated repositories (alphabetical)

| Agent | Repositories | Commits | Introduced | Still at HEAD | Line-weighted |
|---|---:|---:|---:|---:|---:|
| ai | 2 | 78 | 4,084 | 3,653 | 89.4 % |
| aider | 4 | 11,167 | 375,563 | 234,814 | 62.5 % |
| claude-code | 54 | 31,016 | 21,896,560 | 10,171,564 | 46.5 % |
| copilot | 4 | 4,254 | 267,634 | 151,198 | 56.5 % |
| cursor | 39 | 1,343 | 4,418,671 | 832,669 | 18.8 % |
| devin | 17 | 10,870 | 4,131,307 | 2,523,500 | 61.1 % |
| gemini | 15 | 1,602 | 487,180 | 271,222 | 55.7 % |
| github-copilot | 40 | 15,715 | 7,608,935 | 5,579,053 | 73.3 % |
| grok | 1 | 629 | 951,072 | 634,094 | 66.7 % |
| grok-4.6 | 1 | 4 | 6,845 | 5,590 | 81.7 % |
| jules | 8 | 38 | 3,534 | 2,053 | 58.1 % |
| llm | 1 | 1,616 | 279,780 | 224,880 | 80.4 % |
| openai-codex | 26 | 5,634 | 2,026,488 | 1,183,526 | 58.4 % |
| opencode | 2 | 6 | 3,249 | 2,880 | 88.6 % |
| openhands | 5 | 4,929 | 1,614,347 | 783,009 | 48.5 % |
| pi | 2 | 37 | 3,065 | 2,241 | 73.1 % |

## Measured but not aggregated (fewer than 5 AI-tagged commits)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Reproduce |
|---|---:|---:|---:|---:|---|
| openai/openai-python | 1,884 | 3 | 63 | 62 | `re audit openai/openai-python --json` |
| stackblitz/bolt.new | 101 | 0 | 0 | 0 | `re audit stackblitz/bolt.new --json` |

## Excluded from this report

- Shallow clones (history truncated; method v6 refuses them): none
- Audits that failed in this run: fern-api/fern, llvm/llvm-project, richlander/dotnet-inspect
- Opted out by their maintainers (https://github.com/croviatrust/causari/blob/main/.github/survival-optout.txt): 0

## Where this measurement connected

The witness recorded, in 20 of 20 uploaded shard logs, 1 destination(s) over 128 connection(s): 1 allowed, 0 blocked, 0 failed, under an allowlist of 1 rule(s) in enforce mode. Every destination the witness saw is one the policy allows.

| Destination | Outcome | Connections | Bytes out | Bytes in |
|---|---|---:|---:|---:|
| github.com:443 | allowed | 128 | 456,806 | 32,287,712,622 |

Signed run sheet: https://causari.dev/reports/survival/2026/10/reach.sheet.json (profile crovia.pnx.v1, witness `urn:causari:survival-report:witness`, capture `proxy-connect`, disclosure `clear`).  
Policy: https://causari.dev/reports/survival/2026/10/egress-policy.json (source https://github.com/croviatrust/causari/blob/main/.github/egress-policy.json), bound in the sheet as `sha256:7a501cbe66356070f9c6c6a5372a8dd425be059bb2315a71bffcccff64692e21`.  
Verify: `tacet-pnx verify reach.sheet.json --policy egress-policy.json` or `re pnx verify reach.sheet.json --policy egress-policy.json`, or paste the sheet and the policy at https://croviatrust.com/registry/seal/verify/.  

Covered: the connection attempts in the shard logs that were uploaded and concatenated into this sheet. Not covered: any shard that uploaded no log; what the runner does outside the measurement step (checking out this repository, installing the tool, uploading the shard); and any connection that did not go through the witness. The record says where the measurement connected through the witness and how many bytes crossed, nothing about their content.

## Method

Method v6, causari 0.5.0. Detection from commit metadata only; survival from `git blame -w -M -C` at HEAD. Per-commit cap: a commit weighs at most the 95th percentile of per-commit introduced line counts in its repository, and never more than 10,000 lines. Sample floor: 5 VERIFIED commits. VERIFIED only; PROBABLE is listed but never summed. Full clones only. Baseline (method v3): the untagged lines of the same repository, by age; the gap is defined in the Baseline section. Details, limits and how to contest a number: https://causari.dev/method.

## What this report is, and is not

This report counts lines. For each repository it states how many lines were introduced by commits that carry machine-readable AI authorship metadata (trailers such as Co-Authored-By naming an agent, bot author identities, aider markers, and, from method v4, git-ai notes that name a non-empty tool; method v3 counted any git-ai note), and how many of those lines git blame still attributes to those commits at HEAD, under the method version stated on the page (blame with -w -M -C, a per-commit weight cap, a sample floor, full clones only; from method v3 the untagged lines of the same repository, in the matched age windows, stand next to the AI-tagged ones). Every row is reproducible with one command.

It is not a quality judgement, a productivity measure, total AI usage, proof of authorship, a security finding, a correctness finding, or a causal effect of AI. Deleted lines include removed features and rewritten prototypes; surviving lines include dead code. It is not a sample of all AI-assisted code: inline completions leave no trace in git, untagged agent commits are invisible, and the repositories were selected, not drawn at random: 30 hand-picked and 34 found by GitHub commit search as public repositories with at least 5 commits carrying the same AI authorship metadata and at least 100 stars, most-starred first (discovered 2026-09-27); the selection rule and the counts behind it are public. The latest discovery run did not finish, and this list was not replaced. The intervals describe the sampled repositories only.

Prior measurement work asks related questions with different instruments. GitClear publishes churn reports built from code-change patterns across the repositories it analyses; arXiv 2601.16809 ("Will It Survive?") follows the modification of agent-authored code in 201 projects with its own detector and finds that such code is modified less often than human-written code. This report does not reproduce either method and does not adjudicate between them: it publishes counts from git metadata alone, with the method version, the tool version and the exact bytes behind every number, so that the three can be read side by side.

## Cite

Crovia Trust. Survival Report #10 (2026-11-16). https://causari.dev/reports/survival/2026/10/
