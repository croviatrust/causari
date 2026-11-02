# Survival Report #8 — 2026-11-02

Counts of surviving lines from AI-tagged commits in 59 open-source repositories, measured with causari 0.5.0, method v6. Counts, not grades: no rank, no verdict; rows are alphabetical.

Page: https://causari.dev/reports/survival/2026/08/  
Data: https://causari.dev/reports/survival/2026/08/report.json  
Feed: https://causari.dev/reports/survival/feed.xml  
Licence: CC-BY-4.0  
DOI: https://doi.org/10.5281/zenodo.23892452  

## Aggregate

20,408,242 of 41,106,519 lines introduced by 81,683 AI-tagged commits in 59 open-source repositories are still at HEAD (49.6 %). 95 % interval over the sampled repositories: 36.5 % to 68.3 %.

- Repositories aggregated: 59
- Commits in those repositories (no merges): 944,897
- AI-tagged (VERIFIED) commits: 81,683
- Lines introduced by them: 41,106,519
- Still attributed to them at HEAD: 20,408,242
- Line-weighted ratio: 49.6 %
- 95 % bootstrap interval over the sampled repositories: 36.5 % to 68.3 %
- Median of per-repository capped ratios: 73.5 %
- 95 % bootstrap interval on that median: 69.0 % to 76.4 %

95 % percentile interval from 2000 bootstrap resamples of the 59 aggregated repositories (with replacement, seed 8). It describes the sampled repositories, not all AI-assisted code, and not the repositories not in this sample.


## Baseline: the same repositories' untagged lines, in the matched age windows

gap = AI-tagged line-weighted survival minus untagged survival re-weighted to the age mix of the AI-tagged lines of the same repository, over age windows where both cohorts hold at least 5 commits, computed inside each repository; untagged = commits with no machine-readable AI signal (human-written, inline-completed and untagged-agent code alike); age = commit date to HEAD date. A negative gap means AI-tagged lines survive less than untagged lines in the matched age windows of the same repository.

- Repositories with an age-matched gap: 58 of 59 with a baseline
- Median gap across them: +1.1 pts
- 95 % bootstrap interval on that median: -1.0 pts to +5.1 pts
- Gaps below zero: 26 · above zero: 32
- Cleared or rewritten (more than 50% of the repository's commits predate the oldest line still at HEAD): OpenHands/OpenHands. Nothing from before that date survives in them, tagged or not; their rows measure the rewrite as much as the code.

Age windows, counts summed across the repositories with a baseline, one row per age window; one large repository can dominate a window, so no gap is computed from these rows: the gap is computed inside each repository and only its median crosses repositories.

| Line age | AI-tagged commits | AI-tagged lines | Still at HEAD | AI-tagged | Untagged commits | Untagged lines | Still at HEAD | Untagged |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0–30 d | 14,170 | 8,748,748 | 6,125,953 | 70.0 % | 21,026 | 22,916,692 | 18,866,006 | 82.3 % |
| 30–90 d | 20,384 | 12,599,636 | 5,496,079 | 43.6 % | 44,497 | 23,405,541 | 16,097,731 | 68.8 % |
| 90–180 d | 16,548 | 9,793,226 | 3,410,473 | 34.8 % | 51,155 | 56,954,495 | 40,732,202 | 71.5 % |
| 180–365 d | 16,600 | 7,630,456 | 3,955,765 | 51.8 % | 94,199 | 25,029,753 | 10,955,403 | 43.8 % |
| 365–730 d | 10,872 | 2,259,300 | 1,386,952 | 61.4 % | 136,055 | 22,862,484 | 11,016,777 | 48.2 % |
| 730+ d | 3,109 | 75,153 | 33,020 | 43.9 % | 516,133 | 84,718,734 | 19,499,633 | 23.0 % |

## Repositories (alphabetical)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Line-weighted | Capped | Median per commit | Largest commit | Untagged, matched windows | Gap | Reproduce |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Aider-AI/aider | 12,461 | 11,156 | 374,687 | 234,788 | 62.7 % | 41.6 % | 32.4 % | 27 % | 52.7 % | +10.0 pts | `re audit Aider-AI/aider --json` |
| airbytehq/airbyte | 55,552 | 2,855 | 1,236,059 | 497,511 | 40.2 % | 73.1 % | 83.3 % | 40 % | 33.2 % | +7.0 pts | `re audit airbytehq/airbyte --json` |
| anthropics/anthropic-sdk-python | 1,588 | 61 | 21,526 | 19,717 | 91.6 % | 81.7 % | 84.7 % | 65 % | 89.6 % | +2.0 pts | `re audit anthropics/anthropic-sdk-python --json` |
| anthropics/anthropic-sdk-typescript | 1,540 | 85 | 31,902 | 28,847 | 90.4 % | 84.8 % | 96.6 % | 49 % | 88.7 % | +5.1 pts | `re audit anthropics/anthropic-sdk-typescript --json` |
| anthropics/claude-code | 815 | 80 | 31,832 | 1,629 | 5.1 % | 18.8 % | 1.7 % | 69 % | 71.5 % | -66.6 pts | `re audit anthropics/claude-code --json` |
| apache/fluss | 2,371 | 62 | 24,270 | 19,290 | 79.5 % | 79.2 % | 88.5 % | 8 % | 82.0 % | -2.7 pts | `re audit apache/fluss --json` |
| apache/spark | 49,200 | 19 | 1,379 | 1,187 | 86.1 % | 86.1 % | 94.1 % | 19 % | 83.3 % | +8.3 pts | `re audit apache/spark --json` |
| apache/texera | 7,398 | 569 | 222,636 | 175,824 | 79.0 % | 81.9 % | 89.2 % | 8 % | 80.0 % | -1.0 pts | `re audit apache/texera --json` |
| BerriAI/litellm | 46,045 | 6,289 | 2,698,937 | 1,791,424 | 66.4 % | 71.1 % | 69.6 % | 3 % | 62.3 % | +4.1 pts | `re audit BerriAI/litellm --json` |
| browser-use/browser-use | 7,082 | 287 | 20,275 | 4,733 | 23.3 % | 31.2 % | 27.4 % | 19 % | 40.8 % | -17.4 pts | `re audit browser-use/browser-use --json` |
| cfug/flutter.cn | 10,588 | 149 | 68,543 | 55,321 | 80.7 % | 73.0 % | 86.0 % | 48 % | 65.4 % | +15.3 pts | `re audit cfug/flutter.cn --json` |
| cline/cline | 7,631 | 165 | 122,815 | 88,614 | 72.2 % | 72.3 % | 74.0 % | 11 % | 57.3 % | +14.8 pts | `re audit cline/cline --json` |
| cloudflare/workers-sdk | 8,796 | 215 | 93,851 | 74,238 | 79.1 % | 77.8 % | 72.1 % | 7 % | 67.5 % | +11.6 pts | `re audit cloudflare/workers-sdk --json` |
| continuedev/continue | 16,266 | 228 | 27,756 | 17,961 | 64.7 % | 63.4 % | 66.8 % | 11 % | 57.7 % | +7.0 pts | `re audit continuedev/continue --json` |
| cozystack/cozystack | 5,672 | 2,903 | 634,152 | 491,728 | 77.5 % | 77.6 % | 84.6 % | 6 % | 77.8 % | -0.3 pts | `re audit cozystack/cozystack --json` |
| crewAIInc/crewAI | 3,030 | 281 | 3,376,278 | 177,167 | 5.2 % | 79.3 % | 89.3 % | 96 % | 64.9 % | -59.6 pts | `re audit crewAIInc/crewAI --json` |
| croviatrust/causari | 303 | 14 | 6,267 | 4,809 | 76.7 % | 76.7 % | 90.3 % | 25 % | 79.0 % | -1.5 pts | `re audit croviatrust/causari --json` |
| danny-avila/LibreChat | 6,450 | 91 | 84,540 | 77,895 | 92.1 % | 89.0 % | 86.0 % | 24 % | 86.2 % | +5.9 pts | `re audit danny-avila/LibreChat --json` |
| fastrepl/anarlog | 9,949 | 1,544 | 860,632 | 550,336 | 63.9 % | 49.5 % | 15.3 % | 14 % | 56.1 % | +7.9 pts | `re audit fastrepl/anarlog --json` |
| flutter/flutter | 89,800 | 162 | 87,231 | 18,676 | 21.4 % | 65.5 % | 83.2 % | 38 % | 39.6 % | -18.2 pts | `re audit flutter/flutter --json` |
| flutter/website | 8,600 | 157 | 50,618 | 43,858 | 86.6 % | 79.0 % | 88.9 % | 65 % | 85.7 % | +0.9 pts | `re audit flutter/website --json` |
| ghostty-org/ghostty | 13,909 | 108 | 5,858 | 4,144 | 70.7 % | 73.3 % | 84.0 % | 8 % | 51.3 % | +19.5 pts | `re audit ghostty-org/ghostty --json` |
| github/gh-aw-mcpg | 5,982 | 5,488 | 463,618 | 251,375 | 54.2 % | 61.3 % | 69.6 % | 3 % | 41.9 % | +12.3 pts | `re audit github/gh-aw-mcpg --json` |
| google-gemini/gemini-cli | 6,567 | 352 | 74,805 | 47,950 | 64.1 % | 62.7 % | 64.5 % | 4 % | 74.6 % | -10.7 pts | `re audit google-gemini/gemini-cli --json` |
| google/sam | 1,991 | 41 | 264 | 101 | 38.3 % | 47.3 % | 0.0 % | 66 % | 59.0 % | -3.4 pts | `re audit google/sam --json` |
| GoogleCloudPlatform/scion | 6,971 | 87 | 24,177 | 19,849 | 82.1 % | 74.3 % | 80.0 % | 35 % | 81.1 % | +1.0 pts | `re audit GoogleCloudPlatform/scion --json` |
| jdubois/boot-ui | 2,122 | 1,647 | 1,075,202 | 771,797 | 71.8 % | 79.9 % | 82.9 % | 11 % | 75.9 % | -4.1 pts | `re audit jdubois/boot-ui --json` |
| langchain-ai/langchain | 16,997 | 43 | 7,289 | 5,386 | 73.9 % | 69.8 % | 87.4 % | 24 % | 59.2 % | +14.2 pts | `re audit langchain-ai/langchain --json` |
| langgenius/dify | 14,729 | 684 | 1,045,503 | 631,966 | 60.4 % | 47.3 % | 52.8 % | 31 % | 46.8 % | +13.7 pts | `re audit langgenius/dify --json` |
| lidge-ai/cli-jaw | 5,721 | 1,361 | 445,819 | 196,423 | 44.1 % | 75.0 % | 68.0 % | 47 % | 55.7 % | -11.6 pts | `re audit lidge-ai/cli-jaw --json` |
| lidge-ai/ima2-gen | 2,070 | 455 | 127,661 | 46,599 | 36.5 % | 52.7 % | 60.0 % | 19 % | 63.4 % | -26.9 pts | `re audit lidge-ai/ima2-gen --json` |
| lobehub/lobe-chat | 15,418 | 2,657 | 3,610,478 | 1,731,645 | 48.0 % | 67.6 % | 81.4 % | 1 % | 47.0 % | +1.0 pts | `re audit lobehub/lobe-chat --json` |
| managarm/managarm | 6,790 | 979 | 69,292 | 53,612 | 77.4 % | 77.4 % | 87.5 % | 4 % | 72.3 % | +5.1 pts | `re audit managarm/managarm --json` |
| mem0ai/mem0 | 2,652 | 95 | 37,062 | 23,864 | 64.4 % | 64.6 % | 83.0 % | 9 % | 72.5 % | -8.0 pts | `re audit mem0ai/mem0 --json` |
| microsoft/BCApps | 4,525 | 829 | 534,262 | 451,259 | 84.5 % | 79.6 % | 85.0 % | 20 % | 81.6 % | +2.7 pts | `re audit microsoft/BCApps --json` |
| microsoft/vscode | 151,802 | 9,263 | 3,548,948 | 2,707,042 | 76.3 % | 73.5 % | 77.0 % | 25 % | 66.4 % | +9.9 pts | `re audit microsoft/vscode --json` |
| microsoft/vscode-copilot-chat | 3,713 | 336 | 995,871 | 938,654 | 94.3 % | 69.0 % | 76.3 % | 88 % | 55.7 % | +38.6 pts | `re audit microsoft/vscode-copilot-chat --json` |
| NovaSky-AI/SkyRL | 1,440 | 480 | 175,559 | 142,817 | 81.3 % | 80.0 % | 85.9 % | 3 % | 69.3 % | +12.0 pts | `re audit NovaSky-AI/SkyRL --json` |
| NVIDIA-NeMo/Switchyard | 684 | 28 | 15,791 | 10,562 | 66.9 % | 67.5 % | 77.1 % | 20 % | 71.5 % | -7.0 pts | `re audit NVIDIA-NeMo/Switchyard --json` |
| openai/codex | 12,957 | 364 | 183,908 | 107,645 | 58.5 % | 58.0 % | 57.3 % | 6 % | 49.8 % | +8.7 pts | `re audit openai/codex --json` |
| openai/openai-agents-python | 2,430 | 6 | 97,483 | 88,543 | 90.8 % | 89.7 % | 86.7 % | 97 % | no shared window | — | `re audit openai/openai-agents-python --json` |
| OpenHands/extensions | 450 | 351 | 127,741 | 97,805 | 76.6 % | 76.2 % | 81.9 % | 9 % | 79.0 % | -2.4 pts | `re audit OpenHands/extensions --json` |
| OpenHands/OpenHands · rewritten | 8,852 | 2,894 | 1,088,763 | 389,789 | 35.8 % | 33.0 % | 0.0 % | 12 % | 34.5 % | +1.3 pts | `re audit OpenHands/OpenHands --json` |
| OpenHands/software-agent-sdk | 2,771 | 1,895 | 580,028 | 457,417 | 78.9 % | 76.1 % | 78.2 % | 10 % | 79.1 % | -0.2 pts | `re audit OpenHands/software-agent-sdk --json` |
| PrefectHQ/prefect | 18,682 | 1,800 | 483,183 | 391,765 | 81.1 % | 79.9 % | 89.1 % | 3 % | 78.0 % | +3.1 pts | `re audit PrefectHQ/prefect --json` |
| pydantic/pydantic-ai | 5,106 | 383 | 720,393 | 534,269 | 74.2 % | 75.3 % | 75.0 % | 60 % | 83.0 % | -8.8 pts | `re audit pydantic/pydantic-ai --json` |
| QwenLM/qwen-code | 11,026 | 584 | 673,440 | 550,661 | 81.8 % | 76.4 % | 55.0 % | 21 % | 72.1 % | +9.7 pts | `re audit QwenLM/qwen-code --json` |
| ray-project/ray | 32,252 | 1,085 | 683,008 | 446,074 | 65.3 % | 77.3 % | 90.0 % | 21 % | 66.9 % | -1.6 pts | `re audit ray-project/ray --json` |
| RooCodeInc/Roo-Code | 6,210 | 43 | 88,016 | 39,395 | 44.8 % | 57.0 % | 62.6 % | 67 % | 45.3 % | -0.6 pts | `re audit RooCodeInc/Roo-Code --json` |
| run-llama/llama_index | 7,962 | 13 | 8,542 | 7,872 | 92.2 % | 92.2 % | 90.4 % | 73 % | 81.7 % | +10.5 pts | `re audit run-llama/llama_index --json` |
| secdev/scapy | 5,752 | 69 | 8,841 | 8,409 | 95.1 % | 90.9 % | 100.0 % | 28 % | 88.3 % | -0.6 pts | `re audit secdev/scapy --json` |
| smithersai/smithers | 22,798 | 16,007 | 10,247,652 | 2,383,300 | 23.3 % | 31.8 % | 0.0 % | 33 % | 24.7 % | -1.4 pts | `re audit smithersai/smithers --json` |
| sst/opencode | 21,789 | 55 | 26,288 | 3,532 | 13.4 % | 15.2 % | 0.0 % | 42 % | 16.0 % | -2.6 pts | `re audit sst/opencode --json` |
| TracecatHQ/tracecat | 6,171 | 681 | 488,012 | 331,396 | 67.9 % | 66.7 % | 67.3 % | 4 % | 70.4 % | -2.5 pts | `re audit TracecatHQ/tracecat --json` |
| vercel/next.js | 36,341 | 271 | 69,723 | 54,018 | 77.5 % | 75.0 % | 82.1 % | 7 % | 60.9 % | +16.6 pts | `re audit vercel/next.js --json` |
| vllm-project/llm-compressor | 3,320 | 173 | 28,275 | 24,022 | 85.0 % | 77.1 % | 86.3 % | 19 % | 80.2 % | +4.7 pts | `re audit vllm-project/llm-compressor --json` |
| xing-shuyin/pi-web-ui | 1,273 | 34 | 3,052 | 2,575 | 84.4 % | 83.5 % | 92.4 % | 12 % | 72.4 % | +9.3 pts | `re audit xing-shuyin/pi-web-ui --json` |
| youknowone/pyre | 86,021 | 2,529 | 3,113,248 | 2,049,846 | 65.8 % | 63.3 % | 66.7 % | 10 % | 66.9 % | -1.1 pts | `re audit youknowone/pyre --json` |
| zed-industries/zed | 37,514 | 141 | 53,278 | 29,311 | 55.0 % | 61.6 % | 71.4 % | 15 % | 67.4 % | -12.4 pts | `re audit zed-industries/zed --json` |

## By agent, across aggregated repositories (alphabetical)

| Agent | Repositories | Commits | Introduced | Still at HEAD | Line-weighted |
|---|---:|---:|---:|---:|---:|
| ai | 2 | 61 | 2,907 | 2,565 | 88.2 % |
| aider | 4 | 11,167 | 375,563 | 234,815 | 62.5 % |
| claude-code | 52 | 27,737 | 20,700,912 | 9,324,477 | 45.0 % |
| copilot | 4 | 4,194 | 263,944 | 149,290 | 56.6 % |
| cursor | 39 | 1,337 | 4,405,278 | 833,009 | 18.9 % |
| devin | 17 | 9,960 | 3,842,678 | 2,327,474 | 60.6 % |
| gemini | 15 | 1,594 | 484,611 | 273,869 | 56.5 % |
| github-copilot | 39 | 14,101 | 7,058,058 | 5,173,043 | 73.3 % |
| grok | 1 | 469 | 729,403 | 467,331 | 64.1 % |
| grok-4.6 | 1 | 1 | 51 | 50 | 98.0 % |
| jules | 8 | 38 | 3,534 | 2,055 | 58.1 % |
| llm | 1 | 1,087 | 206,499 | 163,232 | 79.0 % |
| openai-codex | 26 | 5,137 | 1,467,238 | 691,915 | 47.2 % |
| opencode | 2 | 6 | 3,249 | 2,880 | 88.6 % |
| openhands | 5 | 4,757 | 1,559,529 | 759,641 | 48.7 % |
| pi | 2 | 37 | 3,065 | 2,596 | 84.7 % |

## Measured but not aggregated (fewer than 5 AI-tagged commits)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Reproduce |
|---|---:|---:|---:|---:|---|
| openai/openai-python | 1,846 | 3 | 63 | 62 | `re audit openai/openai-python --json` |
| stackblitz/bolt.new | 101 | 0 | 0 | 0 | `re audit stackblitz/bolt.new --json` |

## Excluded from this report

- Shallow clones (history truncated; method v6 refuses them): none
- Audits that failed in this run: fern-api/fern, llvm/llvm-project, richlander/dotnet-inspect
- Opted out by their maintainers (https://github.com/croviatrust/causari/blob/main/.github/survival-optout.txt): 0

## Where this measurement connected

The witness recorded, in 20 of 20 uploaded shard logs, 1 destination(s) over 128 connection(s): 1 allowed, 0 blocked, 0 failed, under an allowlist of 1 rule(s) in enforce mode. Every destination the witness saw is one the policy allows.

| Destination | Outcome | Connections | Bytes out | Bytes in |
|---|---|---:|---:|---:|
| github.com:443 | allowed | 128 | 450,798 | 31,279,761,821 |

Signed run sheet: https://causari.dev/reports/survival/2026/08/reach.sheet.json (profile crovia.pnx.v1, witness `urn:causari:survival-report:witness`, capture `proxy-connect`, disclosure `clear`).  
Policy: https://causari.dev/reports/survival/2026/08/egress-policy.json (source https://github.com/croviatrust/causari/blob/main/.github/egress-policy.json), bound in the sheet as `sha256:7a501cbe66356070f9c6c6a5372a8dd425be059bb2315a71bffcccff64692e21`.  
Verify: `tacet-pnx verify reach.sheet.json --policy egress-policy.json` or `re pnx verify reach.sheet.json --policy egress-policy.json`, or paste the sheet and the policy at https://croviatrust.com/registry/seal/verify/.  

Covered: the connection attempts in the shard logs that were uploaded and concatenated into this sheet. Not covered: any shard that uploaded no log; what the runner does outside the measurement step (checking out this repository, installing the tool, uploading the shard); and any connection that did not go through the witness. The record says where the measurement connected through the witness and how many bytes crossed, nothing about their content.

## Method

Method v6, causari 0.5.0. Detection from commit metadata only; survival from `git blame -w -M -C` at HEAD. Per-commit cap: a commit weighs at most the 95th percentile of per-commit introduced line counts in its repository, and never more than 10,000 lines. Sample floor: 5 VERIFIED commits. VERIFIED only; PROBABLE is listed but never summed. Full clones only. Baseline (method v3): the untagged lines of the same repository, by age; the gap is defined in the Baseline section. Details, limits and how to contest a number: https://causari.dev/method.

## What this report is, and is not

This report counts lines. For each repository it states how many lines were introduced by commits that carry machine-readable AI authorship metadata (trailers such as Co-Authored-By naming an agent, bot author identities, aider markers, and, from method v4, git-ai notes that name a non-empty tool; method v3 counted any git-ai note), and how many of those lines git blame still attributes to those commits at HEAD, under the method version stated on the page (blame with -w -M -C, a per-commit weight cap, a sample floor, full clones only; from method v3 the untagged lines of the same repository, in the matched age windows, stand next to the AI-tagged ones). Every row is reproducible with one command.

It is not a quality judgement, a productivity measure, total AI usage, proof of authorship, a security finding, a correctness finding, or a causal effect of AI. Deleted lines include removed features and rewritten prototypes; surviving lines include dead code. It is not a sample of all AI-assisted code: inline completions leave no trace in git, untagged agent commits are invisible, and the repositories were selected, not drawn at random: 30 hand-picked and 34 found by GitHub commit search as public repositories with at least 5 commits carrying the same AI authorship metadata and at least 100 stars, most-starred first (discovered 2026-09-27); the selection rule and the counts behind it are public. The latest discovery run did not finish, and this list was not replaced. The intervals describe the sampled repositories only.

Prior measurement work asks related questions with different instruments. GitClear publishes churn reports built from code-change patterns across the repositories it analyses; arXiv 2601.16809 ("Will It Survive?") follows the modification of agent-authored code in 201 projects with its own detector and finds that such code is modified less often than human-written code. This report does not reproduce either method and does not adjudicate between them: it publishes counts from git metadata alone, with the method version, the tool version and the exact bytes behind every number, so that the three can be read side by side.

## Cite

Crovia Trust. Survival Report #8 (2026-11-02). https://causari.dev/reports/survival/2026/08/ DOI 10.5281/zenodo.23892452
