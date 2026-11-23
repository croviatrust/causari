# Survival Report #11 — 2026-11-23

Counts of surviving lines from AI-tagged commits in 58 open-source repositories, measured with causari 0.5.0, method v6. Counts, not grades: no rank, no verdict; rows are alphabetical.

Page: https://causari.dev/reports/survival/2026/11/  
Data: https://causari.dev/reports/survival/2026/11/report.json  
Feed: https://causari.dev/reports/survival/feed.xml  
Licence: CC-BY-4.0  

## Aggregate

20,358,717 of 41,153,658 lines introduced by 80,581 AI-tagged commits in 58 open-source repositories are still at HEAD (49.5 %). 95 % interval over the sampled repositories: 38.7 % to 66.5 %.

- Repositories aggregated: 58
- Commits in those repositories (no merges): 816,189
- AI-tagged (VERIFIED) commits: 80,581
- Lines introduced by them: 41,153,658
- Still attributed to them at HEAD: 20,358,717
- Line-weighted ratio: 49.5 %
- 95 % bootstrap interval over the sampled repositories: 38.7 % to 66.5 %
- Median of per-repository capped ratios: 73.9 %
- 95 % bootstrap interval on that median: 68.4 % to 76.7 %

95 % percentile interval from 2000 bootstrap resamples of the 58 aggregated repositories (with replacement, seed 11). It describes the sampled repositories, not all AI-assisted code, and not the repositories not in this sample.


## Baseline: the same repositories' untagged lines, in the matched age windows

gap = AI-tagged line-weighted survival minus untagged survival re-weighted to the age mix of the AI-tagged lines of the same repository, over age windows where both cohorts hold at least 5 commits, computed inside each repository; untagged = commits with no machine-readable AI signal (human-written, inline-completed and untagged-agent code alike); age = commit date to HEAD date. A negative gap means AI-tagged lines survive less than untagged lines in the matched age windows of the same repository.

- Repositories with an age-matched gap: 57 of 58 with a baseline
- Median gap across them: +1.4 pts
- 95 % bootstrap interval on that median: +0.0 pts to +6.0 pts
- Gaps below zero: 21 · above zero: 36
- Cleared or rewritten (more than 50% of the repository's commits predate the oldest line still at HEAD): OpenHands/OpenHands. Nothing from before that date survives in them, tagged or not; their rows measure the rewrite as much as the code.

Age windows, counts summed across the repositories with a baseline, one row per age window; one large repository can dominate a window, so no gap is computed from these rows: the gap is computed inside each repository and only its median crosses repositories.

| Line age | AI-tagged commits | AI-tagged lines | Still at HEAD | AI-tagged | Untagged commits | Untagged lines | Still at HEAD | Untagged |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0–30 d | 10,655 | 5,350,170 | 4,410,826 | 82.4 % | 20,641 | 44,131,761 | 41,785,999 | 94.7 % |
| 30–90 d | 24,460 | 16,303,157 | 7,605,355 | 46.6 % | 43,974 | 26,573,212 | 17,836,833 | 67.1 % |
| 90–180 d | 14,911 | 10,017,832 | 3,511,233 | 35.0 % | 50,071 | 59,847,021 | 43,372,992 | 72.5 % |
| 180–365 d | 16,513 | 7,914,402 | 4,118,147 | 52.0 % | 88,572 | 26,745,296 | 11,700,694 | 43.7 % |
| 365–730 d | 10,895 | 1,480,758 | 680,178 | 45.9 % | 125,707 | 21,650,367 | 10,204,143 | 47.1 % |
| 730+ d | 3,147 | 87,339 | 32,978 | 37.8 % | 406,449 | 78,270,392 | 17,856,005 | 22.8 % |

## Repositories (alphabetical)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Line-weighted | Capped | Median per commit | Largest commit | Untagged, matched windows | Gap | Reproduce |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Aider-AI/aider | 12,461 | 11,156 | 374,687 | 234,788 | 62.7 % | 41.6 % | 32.4 % | 27 % | 52.7 % | +10.0 pts | `re audit Aider-AI/aider --json` |
| airbytehq/airbyte | 57,116 | 3,279 | 1,322,181 | 566,471 | 42.8 % | 74.3 % | 84.1 % | 38 % | 33.9 % | +8.9 pts | `re audit airbytehq/airbyte --json` |
| anthropics/anthropic-sdk-python | 1,805 | 77 | 23,077 | 20,820 | 90.2 % | 81.5 % | 86.0 % | 60 % | 89.0 % | +1.4 pts | `re audit anthropics/anthropic-sdk-python --json` |
| anthropics/anthropic-sdk-typescript | 1,743 | 94 | 32,598 | 29,125 | 89.3 % | 84.8 % | 96.3 % | 47 % | 88.5 % | +3.9 pts | `re audit anthropics/anthropic-sdk-typescript --json` |
| anthropics/claude-code | 966 | 84 | 32,159 | 1,817 | 5.7 % | 20.3 % | 2.4 % | 68 % | 64.1 % | -58.4 pts | `re audit anthropics/claude-code --json` |
| apache/fluss | 2,505 | 82 | 35,165 | 28,603 | 81.3 % | 80.6 % | 87.2 % | 8 % | 80.8 % | +0.5 pts | `re audit apache/fluss --json` |
| apache/spark | 49,687 | 19 | 1,379 | 1,189 | 86.2 % | 86.2 % | 94.1 % | 19 % | 77.7 % | +13.8 pts | `re audit apache/spark --json` |
| apache/texera | 7,544 | 616 | 250,249 | 194,616 | 77.8 % | 80.3 % | 86.7 % | 7 % | 77.4 % | +0.3 pts | `re audit apache/texera --json` |
| BerriAI/litellm | 47,756 | 7,138 | 3,088,413 | 2,004,708 | 64.9 % | 68.7 % | 67.1 % | 3 % | 60.7 % | +4.2 pts | `re audit BerriAI/litellm --json` |
| browser-use/browser-use | 7,626 | 822 | 61,409 | 35,116 | 57.2 % | 57.6 % | 75.0 % | 6 % | 52.3 % | +4.9 pts | `re audit browser-use/browser-use --json` |
| cfug/flutter.cn | 10,588 | 149 | 68,543 | 55,321 | 80.7 % | 73.0 % | 86.0 % | 48 % | 65.4 % | +15.3 pts | `re audit cfug/flutter.cn --json` |
| cline/cline | 7,691 | 166 | 122,889 | 88,621 | 72.1 % | 72.2 % | 74.5 % | 11 % | 55.2 % | +16.0 pts | `re audit cline/cline --json` |
| cloudflare/workers-sdk | 8,990 | 237 | 102,298 | 79,906 | 78.1 % | 77.3 % | 72.7 % | 7 % | 69.7 % | +8.4 pts | `re audit cloudflare/workers-sdk --json` |
| continuedev/continue | 16,266 | 228 | 27,756 | 17,961 | 64.7 % | 63.4 % | 66.8 % | 11 % | 57.7 % | +7.0 pts | `re audit continuedev/continue --json` |
| cozystack/cozystack | 6,370 | 3,599 | 792,397 | 607,768 | 76.7 % | 78.2 % | 84.8 % | 5 % | 79.4 % | -2.7 pts | `re audit cozystack/cozystack --json` |
| crewAIInc/crewAI | 3,171 | 335 | 3,412,670 | 210,337 | 6.2 % | 81.6 % | 90.4 % | 95 % | 73.3 % | -67.1 pts | `re audit crewAIInc/crewAI --json` |
| croviatrust/causari | 311 | 14 | 6,267 | 4,809 | 76.7 % | 76.7 % | 90.3 % | 25 % | 76.3 % | +1.2 pts | `re audit croviatrust/causari --json` |
| danny-avila/LibreChat | 6,937 | 94 | 84,957 | 78,264 | 92.1 % | 88.8 % | 85.9 % | 24 % | 84.8 % | +7.3 pts | `re audit danny-avila/LibreChat --json` |
| fastrepl/anarlog | 10,021 | 1,552 | 862,226 | 550,662 | 63.9 % | 49.3 % | 15.1 % | 14 % | 55.6 % | +8.3 pts | `re audit fastrepl/anarlog --json` |
| flutter/flutter | 90,229 | 166 | 87,434 | 18,647 | 21.3 % | 64.6 % | 83.0 % | 38 % | 40.4 % | -19.1 pts | `re audit flutter/flutter --json` |
| flutter/website | 8,659 | 158 | 50,660 | 43,594 | 86.1 % | 78.2 % | 87.5 % | 65 % | 86.0 % | +0.0 pts | `re audit flutter/website --json` |
| ghostty-org/ghostty | 14,144 | 109 | 7,993 | 6,198 | 77.5 % | 73.6 % | 84.6 % | 27 % | 58.2 % | +19.4 pts | `re audit ghostty-org/ghostty --json` |
| github/gh-aw-mcpg | 6,289 | 5,733 | 494,208 | 269,699 | 54.6 % | 61.5 % | 69.0 % | 3 % | 34.2 % | +20.4 pts | `re audit github/gh-aw-mcpg --json` |
| google-gemini/gemini-cli | 6,638 | 354 | 76,440 | 49,890 | 65.3 % | 63.9 % | 64.5 % | 4 % | 71.7 % | -7.3 pts | `re audit google-gemini/gemini-cli --json` |
| google/sam | 2,059 | 41 | 264 | 101 | 38.3 % | 47.3 % | 0.0 % | 66 % | 56.7 % | -10.9 pts | `re audit google/sam --json` |
| GoogleCloudPlatform/scion | 7,855 | 87 | 24,177 | 19,792 | 81.9 % | 73.7 % | 76.4 % | 35 % | 82.2 % | -0.4 pts | `re audit GoogleCloudPlatform/scion --json` |
| jdubois/boot-ui | 2,397 | 1,898 | 1,232,615 | 909,490 | 73.8 % | 80.8 % | 83.3 % | 10 % | 80.1 % | -6.3 pts | `re audit jdubois/boot-ui --json` |
| langchain-ai/langchain | 17,144 | 43 | 7,289 | 5,379 | 73.8 % | 69.7 % | 87.4 % | 24 % | 58.7 % | +15.0 pts | `re audit langchain-ai/langchain --json` |
| langgenius/dify | 15,465 | 739 | 1,077,496 | 637,524 | 59.2 % | 47.8 % | 53.3 % | 30 % | 44.8 % | +14.4 pts | `re audit langgenius/dify --json` |
| lidge-ai/cli-jaw | 5,762 | 1,361 | 445,819 | 195,580 | 43.9 % | 74.5 % | 66.7 % | 47 % | 66.2 % | -22.3 pts | `re audit lidge-ai/cli-jaw --json` |
| lidge-ai/ima2-gen | 2,249 | 455 | 127,661 | 46,456 | 36.4 % | 52.5 % | 59.0 % | 19 % | 75.4 % | -39.0 pts | `re audit lidge-ai/ima2-gen --json` |
| lobehub/lobe-chat | 16,455 | 3,237 | 4,160,886 | 2,118,156 | 50.9 % | 67.7 % | 80.7 % | 1 % | 50.2 % | +0.7 pts | `re audit lobehub/lobe-chat --json` |
| managarm/managarm | 7,131 | 1,225 | 83,715 | 64,769 | 77.4 % | 76.8 % | 87.5 % | 4 % | 77.3 % | +0.1 pts | `re audit managarm/managarm --json` |
| mem0ai/mem0 | 2,657 | 95 | 37,062 | 23,849 | 64.3 % | 64.5 % | 82.8 % | 9 % | 67.4 % | -2.8 pts | `re audit mem0ai/mem0 --json` |
| microsoft/BCApps | 4,791 | 980 | 603,923 | 507,023 | 84.0 % | 79.8 % | 85.6 % | 18 % | 82.6 % | +1.4 pts | `re audit microsoft/BCApps --json` |
| microsoft/vscode-copilot-chat | 3,713 | 336 | 995,871 | 938,654 | 94.3 % | 69.0 % | 76.3 % | 88 % | 55.7 % | +38.6 pts | `re audit microsoft/vscode-copilot-chat --json` |
| NovaSky-AI/SkyRL | 1,497 | 531 | 199,609 | 162,558 | 81.4 % | 80.5 % | 86.2 % | 3 % | 67.2 % | +14.2 pts | `re audit NovaSky-AI/SkyRL --json` |
| NVIDIA-NeMo/Switchyard | 793 | 28 | 15,791 | 10,488 | 66.4 % | 67.0 % | 76.4 % | 20 % | 53.2 % | +13.3 pts | `re audit NVIDIA-NeMo/Switchyard --json` |
| openai/codex | 14,071 | 364 | 183,908 | 106,441 | 57.9 % | 57.2 % | 55.0 % | 6 % | 48.5 % | +9.4 pts | `re audit openai/codex --json` |
| openai/openai-agents-python | 2,495 | 6 | 97,483 | 88,561 | 90.8 % | 89.7 % | 86.7 % | 97 % | no shared window | — | `re audit openai/openai-agents-python --json` |
| OpenHands/extensions | 479 | 372 | 133,883 | 103,384 | 77.2 % | 76.8 % | 80.0 % | 9 % | 69.8 % | +7.4 pts | `re audit OpenHands/extensions --json` |
| OpenHands/OpenHands · rewritten | 9,204 | 3,042 | 1,142,173 | 419,427 | 36.7 % | 34.3 % | 0.0 % | 12 % | 32.4 % | +4.3 pts | `re audit OpenHands/OpenHands --json` |
| OpenHands/software-agent-sdk | 3,124 | 2,052 | 681,735 | 535,572 | 78.6 % | 75.0 % | 76.9 % | 8 % | 78.4 % | +0.2 pts | `re audit OpenHands/software-agent-sdk --json` |
| PrefectHQ/prefect | 18,887 | 1,922 | 504,399 | 409,075 | 81.1 % | 79.9 % | 89.2 % | 3 % | 78.7 % | +2.4 pts | `re audit PrefectHQ/prefect --json` |
| pydantic/pydantic-ai | 5,940 | 428 | 752,375 | 560,717 | 74.5 % | 76.3 % | 76.2 % | 58 % | 78.2 % | -3.6 pts | `re audit pydantic/pydantic-ai --json` |
| QwenLM/qwen-code | 12,070 | 616 | 714,534 | 586,354 | 82.1 % | 77.3 % | 58.9 % | 20 % | 72.8 % | +9.2 pts | `re audit QwenLM/qwen-code --json` |
| ray-project/ray | 32,606 | 1,280 | 814,135 | 550,159 | 67.6 % | 78.7 % | 90.2 % | 18 % | 69.6 % | -2.0 pts | `re audit ray-project/ray --json` |
| RooCodeInc/Roo-Code | 6,210 | 43 | 88,016 | 39,395 | 44.8 % | 57.0 % | 62.6 % | 67 % | 45.3 % | -0.6 pts | `re audit RooCodeInc/Roo-Code --json` |
| run-llama/llama_index | 7,971 | 15 | 8,543 | 7,872 | 92.1 % | 92.1 % | 95.4 % | 73 % | 83.8 % | -0.3 pts | `re audit run-llama/llama_index --json` |
| secdev/scapy | 5,814 | 90 | 10,334 | 9,787 | 94.7 % | 90.7 % | 100.0 % | 24 % | 92.5 % | -3.1 pts | `re audit secdev/scapy --json` |
| smithersai/smithers | 26,288 | 18,664 | 11,304,006 | 3,251,271 | 28.8 % | 37.6 % | 1.6 % | 30 % | 30.3 % | -1.5 pts | `re audit smithersai/smithers --json` |
| sst/opencode | 23,112 | 58 | 26,350 | 1,916 | 7.3 % | 5.7 % | 0.0 % | 42 % | 16.7 % | -9.6 pts | `re audit sst/opencode --json` |
| TracecatHQ/tracecat | 6,359 | 767 | 566,198 | 398,307 | 70.3 % | 68.4 % | 70.3 % | 4 % | 72.4 % | -2.1 pts | `re audit TracecatHQ/tracecat --json` |
| vercel/next.js | 36,761 | 294 | 80,468 | 62,638 | 77.8 % | 75.7 % | 81.5 % | 6 % | 62.9 % | +15.0 pts | `re audit vercel/next.js --json` |
| vllm-project/llm-compressor | 3,382 | 179 | 29,186 | 23,777 | 81.5 % | 75.6 % | 85.3 % | 18 % | 75.5 % | +6.0 pts | `re audit vllm-project/llm-compressor --json` |
| xing-shuyin/pi-web-ui | 1,760 | 36 | 4,149 | 3,200 | 77.1 % | 74.1 % | 77.6 % | 25 % | 24.0 % | +47.9 pts | `re audit xing-shuyin/pi-web-ui --json` |
| youknowone/pyre | 86,368 | 2,875 | 3,520,445 | 2,325,966 | 66.1 % | 63.8 % | 65.9 % | 9 % | 71.2 % | -5.1 pts | `re audit youknowone/pyre --json` |
| zed-industries/zed | 37,807 | 161 | 61,105 | 36,149 | 59.2 % | 64.8 % | 75.5 % | 13 % | 70.9 % | -11.7 pts | `re audit zed-industries/zed --json` |

## By agent, across aggregated repositories (alphabetical)

| Agent | Repositories | Commits | Introduced | Still at HEAD | Line-weighted |
|---|---:|---:|---:|---:|---:|
| ai | 2 | 80 | 4,347 | 3,890 | 89.5 % |
| aider | 4 | 11,167 | 375,563 | 234,814 | 62.5 % |
| claude-code | 53 | 32,103 | 22,448,312 | 10,590,416 | 47.2 % |
| copilot | 3 | 3,768 | 257,433 | 146,747 | 57.0 % |
| cursor | 38 | 1,344 | 4,434,222 | 841,001 | 19.0 % |
| devin | 17 | 11,368 | 4,311,205 | 2,632,706 | 61.1 % |
| gemini | 15 | 1,606 | 487,720 | 269,198 | 55.2 % |
| github-copilot | 39 | 6,006 | 3,801,996 | 2,698,248 | 71.0 % |
| grok | 1 | 650 | 975,725 | 653,618 | 67.0 % |
| grok-4.6 | 1 | 4 | 6,845 | 5,586 | 81.6 % |
| jules | 8 | 38 | 3,534 | 2,053 | 58.1 % |
| llm | 1 | 1,783 | 364,744 | 280,832 | 77.0 % |
| openai-codex | 26 | 5,641 | 2,030,913 | 1,186,112 | 58.4 % |
| opencode | 2 | 6 | 3,249 | 2,879 | 88.6 % |
| openhands | 5 | 4,980 | 1,644,785 | 808,389 | 49.1 % |
| pi | 2 | 37 | 3,065 | 2,228 | 72.7 % |

## Measured but not aggregated (fewer than 5 AI-tagged commits)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Reproduce |
|---|---:|---:|---:|---:|---|
| openai/openai-python | 1,906 | 3 | 63 | 62 | `re audit openai/openai-python --json` |
| stackblitz/bolt.new | 101 | 0 | 0 | 0 | `re audit stackblitz/bolt.new --json` |

## Excluded from this report

- Shallow clones (history truncated; method v6 refuses them): none
- Audits that failed in this run: fern-api/fern, llvm/llvm-project, microsoft/vscode, richlander/dotnet-inspect
- Opted out by their maintainers (https://github.com/croviatrust/causari/blob/main/.github/survival-optout.txt): 0

## Where this measurement connected

The witness recorded, in 20 of 20 uploaded shard logs, 1 destination(s) over 128 connection(s): 1 allowed, 0 blocked, 0 failed, under an allowlist of 1 rule(s) in enforce mode. Every destination the witness saw is one the policy allows.

| Destination | Outcome | Connections | Bytes out | Bytes in |
|---|---|---:|---:|---:|
| github.com:443 | allowed | 128 | 459,143 | 32,718,780,586 |

Signed run sheet: https://causari.dev/reports/survival/2026/11/reach.sheet.json (profile crovia.pnx.v1, witness `urn:causari:survival-report:witness`, capture `proxy-connect`, disclosure `clear`).  
Policy: https://causari.dev/reports/survival/2026/11/egress-policy.json (source https://github.com/croviatrust/causari/blob/main/.github/egress-policy.json), bound in the sheet as `sha256:7a501cbe66356070f9c6c6a5372a8dd425be059bb2315a71bffcccff64692e21`.  
Verify: `tacet-pnx verify reach.sheet.json --policy egress-policy.json` or `re pnx verify reach.sheet.json --policy egress-policy.json`, or paste the sheet and the policy at https://croviatrust.com/registry/seal/verify/.  

Covered: the connection attempts in the shard logs that were uploaded and concatenated into this sheet. Not covered: any shard that uploaded no log; what the runner does outside the measurement step (checking out this repository, installing the tool, uploading the shard); and any connection that did not go through the witness. The record says where the measurement connected through the witness and how many bytes crossed, nothing about their content.

## Method

Method v6, causari 0.5.0. Detection from commit metadata only; survival from `git blame -w -M -C` at HEAD. Per-commit cap: a commit weighs at most the 95th percentile of per-commit introduced line counts in its repository, and never more than 10,000 lines. Sample floor: 5 VERIFIED commits. VERIFIED only; PROBABLE is listed but never summed. Full clones only. Baseline (method v3): the untagged lines of the same repository, by age; the gap is defined in the Baseline section. Details, limits and how to contest a number: https://causari.dev/method.

## What this report is, and is not

This report counts lines. For each repository it states how many lines were introduced by commits that carry machine-readable AI authorship metadata (trailers such as Co-Authored-By naming an agent, bot author identities, aider markers, and, from method v4, git-ai notes that name a non-empty tool; method v3 counted any git-ai note), and how many of those lines git blame still attributes to those commits at HEAD, under the method version stated on the page (blame with -w -M -C, a per-commit weight cap, a sample floor, full clones only; from method v3 the untagged lines of the same repository, in the matched age windows, stand next to the AI-tagged ones). Every row is reproducible with one command.

It is not a quality judgement, a productivity measure, total AI usage, proof of authorship, a security finding, a correctness finding, or a causal effect of AI. Deleted lines include removed features and rewritten prototypes; surviving lines include dead code. It is not a sample of all AI-assisted code: inline completions leave no trace in git, untagged agent commits are invisible, and the repositories were selected, not drawn at random: 30 hand-picked and 34 found by GitHub commit search as public repositories with at least 5 commits carrying the same AI authorship metadata and at least 100 stars, most-starred first (discovered 2026-09-27); the selection rule and the counts behind it are public. The latest discovery run did not finish, and this list was not replaced. The intervals describe the sampled repositories only.

Prior measurement work asks related questions with different instruments. GitClear publishes churn reports built from code-change patterns across the repositories it analyses; arXiv 2601.16809 ("Will It Survive?") follows the modification of agent-authored code in 201 projects with its own detector and finds that such code is modified less often than human-written code. This report does not reproduce either method and does not adjudicate between them: it publishes counts from git metadata alone, with the method version, the tool version and the exact bytes behind every number, so that the three can be read side by side.

## Cite

Crovia Trust. Survival Report #11 (2026-11-23). https://causari.dev/reports/survival/2026/11/
