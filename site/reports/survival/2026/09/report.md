# Survival Report #9 — 2026-11-09

Counts of surviving lines from AI-tagged commits in 55 open-source repositories, measured with causari 0.5.0, method v6. Counts, not grades: no rank, no verdict; rows are alphabetical.

Page: https://causari.dev/reports/survival/2026/09/  
Data: https://causari.dev/reports/survival/2026/09/report.json  
Feed: https://causari.dev/reports/survival/feed.xml  
Licence: CC-BY-4.0  
DOI: https://doi.org/10.5281/zenodo.24091343  

## Aggregate

15,826,982 of 27,960,795 lines introduced by 57,538 AI-tagged commits in 55 open-source repositories are still at HEAD (56.6 %). 95 % interval over the sampled repositories: 43.6 % to 68.8 %.

- Repositories aggregated: 55
- Commits in those repositories (no merges): 685,276
- AI-tagged (VERIFIED) commits: 57,538
- Lines introduced by them: 27,960,795
- Still attributed to them at HEAD: 15,826,982
- Line-weighted ratio: 56.6 %
- 95 % bootstrap interval over the sampled repositories: 43.6 % to 68.8 %
- Median of per-repository capped ratios: 74.4 %
- 95 % bootstrap interval on that median: 69.8 % to 76.5 %

95 % percentile interval from 2000 bootstrap resamples of the 55 aggregated repositories (with replacement, seed 9). It describes the sampled repositories, not all AI-assisted code, and not the repositories not in this sample.


## Baseline: the same repositories' untagged lines, in the matched age windows

gap = AI-tagged line-weighted survival minus untagged survival re-weighted to the age mix of the AI-tagged lines of the same repository, over age windows where both cohorts hold at least 5 commits, computed inside each repository; untagged = commits with no machine-readable AI signal (human-written, inline-completed and untagged-agent code alike); age = commit date to HEAD date. A negative gap means AI-tagged lines survive less than untagged lines in the matched age windows of the same repository.

- Repositories with an age-matched gap: 54 of 55 with a baseline
- Median gap across them: +1.6 pts
- 95 % bootstrap interval on that median: -0.8 pts to +5.6 pts
- Gaps below zero: 22 · above zero: 32
- Cleared or rewritten (more than 50% of the repository's commits predate the oldest line still at HEAD): OpenHands/OpenHands. Nothing from before that date survives in them, tagged or not; their rows measure the rewrite as much as the code.

Age windows, counts summed across the repositories with a baseline, one row per age window; one large repository can dominate a window, so no gap is computed from these rows: the gap is computed inside each repository and only its median crosses repositories.

| Line age | AI-tagged commits | AI-tagged lines | Still at HEAD | AI-tagged | Untagged commits | Untagged lines | Still at HEAD | Untagged |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0–30 d | 5,718 | 4,805,272 | 3,765,246 | 78.4 % | 18,818 | 30,664,492 | 26,786,362 | 87.4 % |
| 30–90 d | 12,206 | 6,703,091 | 4,393,704 | 65.5 % | 39,013 | 20,451,293 | 15,431,326 | 75.5 % |
| 90–180 d | 11,561 | 8,426,910 | 3,113,545 | 36.9 % | 42,629 | 48,737,924 | 41,006,437 | 84.1 % |
| 180–365 d | 14,372 | 6,671,428 | 3,903,525 | 58.5 % | 83,827 | 24,511,480 | 11,054,003 | 45.1 % |
| 365–730 d | 10,561 | 1,276,346 | 617,953 | 48.4 % | 118,391 | 19,773,647 | 9,629,539 | 48.7 % |
| 730+ d | 3,120 | 77,748 | 33,009 | 42.5 % | 324,906 | 70,074,774 | 15,139,366 | 21.6 % |

## Repositories (alphabetical)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Line-weighted | Capped | Median per commit | Largest commit | Untagged, matched windows | Gap | Reproduce |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Aider-AI/aider | 12,461 | 11,156 | 374,687 | 234,788 | 62.7 % | 41.6 % | 32.4 % | 27 % | 52.7 % | +10.0 pts | `re audit Aider-AI/aider --json` |
| airbytehq/airbyte | 56,115 | 3,019 | 1,277,488 | 530,106 | 41.5 % | 73.4 % | 83.3 % | 39 % | 30.4 % | +11.1 pts | `re audit airbytehq/airbyte --json` |
| anthropics/anthropic-sdk-python | 1,690 | 68 | 22,299 | 20,144 | 90.3 % | 80.7 % | 86.2 % | 63 % | 88.0 % | +2.3 pts | `re audit anthropics/anthropic-sdk-python --json` |
| anthropics/anthropic-sdk-typescript | 1,637 | 91 | 31,957 | 28,525 | 89.3 % | 84.5 % | 96.6 % | 48 % | 88.0 % | +4.4 pts | `re audit anthropics/anthropic-sdk-typescript --json` |
| apache/fluss | 2,423 | 68 | 26,869 | 21,440 | 79.8 % | 79.6 % | 88.7 % | 7 % | 81.7 % | -2.1 pts | `re audit apache/fluss --json` |
| apache/spark | 49,350 | 19 | 1,379 | 1,189 | 86.2 % | 86.2 % | 94.1 % | 19 % | 82.5 % | +9.1 pts | `re audit apache/spark --json` |
| apache/texera | 7,449 | 585 | 229,412 | 181,537 | 79.1 % | 82.0 % | 89.2 % | 8 % | 80.0 % | -0.9 pts | `re audit apache/texera --json` |
| BerriAI/litellm | 46,418 | 6,466 | 2,799,447 | 1,843,323 | 65.8 % | 70.0 % | 68.1 % | 3 % | 60.5 % | +5.3 pts | `re audit BerriAI/litellm --json` |
| browser-use/browser-use | 7,082 | 287 | 20,275 | 4,733 | 23.3 % | 31.2 % | 27.4 % | 19 % | 40.8 % | -17.4 pts | `re audit browser-use/browser-use --json` |
| cfug/flutter.cn | 10,588 | 149 | 68,543 | 55,321 | 80.7 % | 73.0 % | 86.0 % | 48 % | 65.4 % | +15.3 pts | `re audit cfug/flutter.cn --json` |
| cline/cline | 7,657 | 165 | 122,815 | 88,610 | 72.1 % | 72.3 % | 74.0 % | 11 % | 57.0 % | +15.2 pts | `re audit cline/cline --json` |
| cloudflare/workers-sdk | 8,856 | 222 | 95,209 | 75,401 | 79.2 % | 77.9 % | 72.6 % | 7 % | 68.0 % | +11.2 pts | `re audit cloudflare/workers-sdk --json` |
| continuedev/continue | 16,266 | 228 | 27,756 | 17,961 | 64.7 % | 63.4 % | 66.8 % | 11 % | 57.7 % | +7.0 pts | `re audit continuedev/continue --json` |
| cozystack/cozystack | 5,737 | 2,968 | 652,837 | 503,873 | 77.2 % | 77.1 % | 83.3 % | 6 % | 80.4 % | -3.9 pts | `re audit cozystack/cozystack --json` |
| crewAIInc/crewAI | 3,077 | 311 | 3,400,174 | 199,584 | 5.9 % | 81.0 % | 89.4 % | 95 % | 70.3 % | -64.4 pts | `re audit crewAIInc/crewAI --json` |
| croviatrust/causari | 305 | 14 | 6,267 | 4,809 | 76.7 % | 76.7 % | 90.3 % | 25 % | 86.0 % | -8.5 pts | `re audit croviatrust/causari --json` |
| danny-avila/LibreChat | 6,450 | 91 | 84,540 | 77,895 | 92.1 % | 89.0 % | 86.0 % | 24 % | 86.2 % | +5.9 pts | `re audit danny-avila/LibreChat --json` |
| fastrepl/anarlog | 9,950 | 1,544 | 860,632 | 550,317 | 63.9 % | 49.5 % | 15.1 % | 14 % | 57.1 % | +6.9 pts | `re audit fastrepl/anarlog --json` |
| flutter/website | 8,614 | 158 | 50,660 | 43,805 | 86.5 % | 78.4 % | 88.5 % | 65 % | 86.1 % | +0.3 pts | `re audit flutter/website --json` |
| ghostty-org/ghostty | 14,055 | 108 | 5,858 | 4,135 | 70.6 % | 73.1 % | 84.0 % | 8 % | 52.0 % | +18.5 pts | `re audit ghostty-org/ghostty --json` |
| github/gh-aw-mcpg | 6,046 | 5,547 | 476,168 | 259,911 | 54.6 % | 61.3 % | 69.4 % | 3 % | 42.2 % | +12.3 pts | `re audit github/gh-aw-mcpg --json` |
| google-gemini/gemini-cli | 6,598 | 353 | 75,767 | 49,052 | 64.7 % | 63.4 % | 64.8 % | 4 % | 74.0 % | -9.8 pts | `re audit google-gemini/gemini-cli --json` |
| google/sam | 2,010 | 41 | 264 | 101 | 38.3 % | 47.3 % | 0.0 % | 66 % | 59.0 % | -3.4 pts | `re audit google/sam --json` |
| GoogleCloudPlatform/scion | 7,168 | 87 | 24,177 | 19,847 | 82.1 % | 74.2 % | 80.0 % | 35 % | 81.9 % | +0.1 pts | `re audit GoogleCloudPlatform/scion --json` |
| jdubois/boot-ui | 2,161 | 1,682 | 1,103,186 | 796,663 | 72.2 % | 80.1 % | 83.3 % | 11 % | 75.4 % | -3.2 pts | `re audit jdubois/boot-ui --json` |
| langchain-ai/langchain | 17,040 | 43 | 7,289 | 5,385 | 73.9 % | 69.8 % | 87.4 % | 24 % | 55.6 % | +17.7 pts | `re audit langchain-ai/langchain --json` |
| langgenius/dify | 14,994 | 698 | 1,046,297 | 625,152 | 59.7 % | 46.9 % | 53.5 % | 31 % | 47.1 % | +12.7 pts | `re audit langgenius/dify --json` |
| lidge-ai/cli-jaw | 5,757 | 1,361 | 445,819 | 195,581 | 43.9 % | 74.5 % | 66.7 % | 47 % | 65.4 % | -21.5 pts | `re audit lidge-ai/cli-jaw --json` |
| lidge-ai/ima2-gen | 2,070 | 455 | 127,661 | 46,599 | 36.5 % | 52.7 % | 60.0 % | 19 % | 63.4 % | -26.9 pts | `re audit lidge-ai/ima2-gen --json` |
| lobehub/lobe-chat | 15,707 | 2,805 | 3,757,966 | 1,833,821 | 48.8 % | 67.5 % | 80.9 % | 1 % | 48.1 % | +0.7 pts | `re audit lobehub/lobe-chat --json` |
| managarm/managarm | 6,908 | 1,076 | 75,379 | 58,104 | 77.1 % | 77.2 % | 87.5 % | 4 % | 74.9 % | +2.2 pts | `re audit managarm/managarm --json` |
| mem0ai/mem0 | 2,655 | 95 | 37,062 | 23,851 | 64.4 % | 64.5 % | 82.8 % | 9 % | 72.9 % | -8.4 pts | `re audit mem0ai/mem0 --json` |
| microsoft/BCApps | 4,601 | 870 | 544,501 | 459,083 | 84.3 % | 79.8 % | 85.1 % | 20 % | 80.6 % | +3.6 pts | `re audit microsoft/BCApps --json` |
| microsoft/vscode-copilot-chat | 3,713 | 336 | 995,871 | 938,654 | 94.3 % | 69.0 % | 76.3 % | 88 % | 55.7 % | +38.6 pts | `re audit microsoft/vscode-copilot-chat --json` |
| NovaSky-AI/SkyRL | 1,460 | 499 | 186,872 | 151,616 | 81.1 % | 80.2 % | 84.6 % | 3 % | 68.6 % | +12.5 pts | `re audit NovaSky-AI/SkyRL --json` |
| NVIDIA-NeMo/Switchyard | 722 | 28 | 15,791 | 10,509 | 66.6 % | 67.2 % | 77.1 % | 20 % | 64.3 % | +1.3 pts | `re audit NVIDIA-NeMo/Switchyard --json` |
| openai/codex | 13,321 | 364 | 183,908 | 107,088 | 58.2 % | 57.6 % | 57.2 % | 6 % | 47.8 % | +10.4 pts | `re audit openai/codex --json` |
| openai/openai-agents-python | 2,462 | 6 | 97,483 | 88,563 | 90.8 % | 89.7 % | 86.7 % | 97 % | no shared window | — | `re audit openai/openai-agents-python --json` |
| OpenHands/extensions | 466 | 364 | 128,588 | 98,334 | 76.5 % | 76.1 % | 80.4 % | 9 % | 77.8 % | -1.3 pts | `re audit OpenHands/extensions --json` |
| OpenHands/OpenHands · rewritten | 8,997 | 2,962 | 1,116,071 | 397,747 | 35.6 % | 33.1 % | 0.0 % | 12 % | 33.7 % | +1.9 pts | `re audit OpenHands/OpenHands --json` |
| OpenHands/software-agent-sdk | 2,920 | 1,938 | 619,739 | 490,866 | 79.2 % | 75.4 % | 77.5 % | 9 % | 79.1 % | +0.1 pts | `re audit OpenHands/software-agent-sdk --json` |
| PrefectHQ/prefect | 18,736 | 1,832 | 487,217 | 394,695 | 81.0 % | 79.8 % | 89.0 % | 3 % | 76.4 % | +4.7 pts | `re audit PrefectHQ/prefect --json` |
| pydantic/pydantic-ai | 5,365 | 410 | 738,522 | 550,428 | 74.5 % | 76.4 % | 77.1 % | 59 % | 81.6 % | -7.1 pts | `re audit pydantic/pydantic-ai --json` |
| QwenLM/qwen-code | 11,419 | 604 | 683,887 | 559,869 | 81.9 % | 76.5 % | 58.0 % | 21 % | 73.5 % | +8.3 pts | `re audit QwenLM/qwen-code --json` |
| ray-project/ray | 32,396 | 1,176 | 760,330 | 507,207 | 66.7 % | 78.5 % | 90.4 % | 19 % | 68.5 % | -1.8 pts | `re audit ray-project/ray --json` |
| RooCodeInc/Roo-Code | 6,210 | 43 | 88,016 | 39,395 | 44.8 % | 57.0 % | 62.6 % | 67 % | 45.3 % | -0.6 pts | `re audit RooCodeInc/Roo-Code --json` |
| run-llama/llama_index | 7,964 | 14 | 8,543 | 7,873 | 92.2 % | 92.2 % | 95.4 % | 73 % | 84.5 % | -1.3 pts | `re audit run-llama/llama_index --json` |
| secdev/scapy | 5,765 | 73 | 9,079 | 8,634 | 95.1 % | 91.1 % | 100.0 % | 27 % | 89.0 % | -0.8 pts | `re audit secdev/scapy --json` |
| sst/opencode | 22,299 | 56 | 26,343 | 1,911 | 7.3 % | 5.7 % | 0.0 % | 42 % | 16.5 % | -9.6 pts | `re audit sst/opencode --json` |
| TracecatHQ/tracecat | 6,224 | 702 | 522,485 | 361,746 | 69.2 % | 67.2 % | 67.4 % | 4 % | 71.1 % | -1.8 pts | `re audit TracecatHQ/tracecat --json` |
| vercel/next.js | 36,480 | 285 | 75,916 | 59,008 | 77.7 % | 75.5 % | 80.9 % | 6 % | 62.2 % | +15.5 pts | `re audit vercel/next.js --json` |
| vllm-project/llm-compressor | 3,346 | 173 | 28,275 | 22,965 | 81.2 % | 74.9 % | 85.7 % | 19 % | 75.1 % | +6.1 pts | `re audit vllm-project/llm-compressor --json` |
| xing-shuyin/pi-web-ui | 1,366 | 36 | 4,149 | 3,199 | 77.1 % | 74.4 % | 76.2 % | 25 % | 49.9 % | +27.2 pts | `re audit xing-shuyin/pi-web-ui --json` |
| youknowone/pyre | 86,154 | 2,661 | 3,248,295 | 2,135,536 | 65.7 % | 63.3 % | 66.5 % | 10 % | 68.5 % | -2.8 pts | `re audit youknowone/pyre --json` |
| zed-industries/zed | 37,596 | 146 | 54,775 | 30,493 | 55.7 % | 62.4 % | 72.8 % | 14 % | 68.3 % | -12.7 pts | `re audit zed-industries/zed --json` |

## By agent, across aggregated repositories (alphabetical)

| Agent | Repositories | Commits | Introduced | Still at HEAD | Line-weighted |
|---|---:|---:|---:|---:|---:|
| ai | 2 | 65 | 3,145 | 2,790 | 88.7 % |
| aider | 4 | 11,167 | 375,563 | 234,815 | 62.5 % |
| claude-code | 49 | 16,528 | 11,854,304 | 7,629,670 | 64.4 % |
| copilot | 3 | 3,746 | 257,007 | 147,103 | 57.2 % |
| cursor | 37 | 1,332 | 4,410,383 | 835,074 | 18.9 % |
| devin | 17 | 10,320 | 3,958,455 | 2,397,122 | 60.6 % |
| gemini | 14 | 1,439 | 399,334 | 254,780 | 63.8 % |
| github-copilot | 38 | 5,505 | 3,592,159 | 2,527,412 | 70.4 % |
| grok | 1 | 545 | 821,104 | 533,758 | 65.0 % |
| grok-4.6 | 1 | 1 | 51 | 50 | 98.0 % |
| jules | 7 | 37 | 3,366 | 1,885 | 56.0 % |
| llm | 1 | 1,152 | 225,184 | 176,288 | 78.3 % |
| openai-codex | 25 | 801 | 459,465 | 310,375 | 67.6 % |
| opencode | 2 | 6 | 3,249 | 2,880 | 88.6 % |
| openhands | 5 | 4,857 | 1,594,961 | 770,738 | 48.3 % |
| pi | 2 | 37 | 3,065 | 2,242 | 73.1 % |

## Measured but not aggregated (fewer than 5 AI-tagged commits)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Reproduce |
|---|---:|---:|---:|---:|---|
| openai/openai-python | 1,874 | 3 | 63 | 62 | `re audit openai/openai-python --json` |
| stackblitz/bolt.new | 101 | 0 | 0 | 0 | `re audit stackblitz/bolt.new --json` |

## Excluded from this report

- Shallow clones (history truncated; method v6 refuses them): none
- Audits that failed in this run: anthropics/claude-code, fern-api/fern, flutter/flutter, llvm/llvm-project, microsoft/vscode, richlander/dotnet-inspect, smithersai/smithers
- Opted out by their maintainers (https://github.com/croviatrust/causari/blob/main/.github/survival-optout.txt): 0

## Where this measurement connected

The witness recorded, in 19 of 19 uploaded shard logs, 1 destination(s) over 122 connection(s): 1 allowed, 0 blocked, 0 failed, under an allowlist of 1 rule(s) in enforce mode. Every destination the witness saw is one the policy allows.

| Destination | Outcome | Connections | Bytes out | Bytes in |
|---|---|---:|---:|---:|
| github.com:443 | allowed | 122 | 440,137 | 30,668,233,609 |

Signed run sheet: https://causari.dev/reports/survival/2026/09/reach.sheet.json (profile crovia.pnx.v1, witness `urn:causari:survival-report:witness`, capture `proxy-connect`, disclosure `clear`).  
Policy: https://causari.dev/reports/survival/2026/09/egress-policy.json (source https://github.com/croviatrust/causari/blob/main/.github/egress-policy.json), bound in the sheet as `sha256:7a501cbe66356070f9c6c6a5372a8dd425be059bb2315a71bffcccff64692e21`.  
Verify: `tacet-pnx verify reach.sheet.json --policy egress-policy.json` or `re pnx verify reach.sheet.json --policy egress-policy.json`, or paste the sheet and the policy at https://croviatrust.com/registry/seal/verify/.  

Covered: the connection attempts in the shard logs that were uploaded and concatenated into this sheet. Not covered: any shard that uploaded no log; what the runner does outside the measurement step (checking out this repository, installing the tool, uploading the shard); and any connection that did not go through the witness. The record says where the measurement connected through the witness and how many bytes crossed, nothing about their content.

## Method

Method v6, causari 0.5.0. Detection from commit metadata only; survival from `git blame -w -M -C` at HEAD. Per-commit cap: a commit weighs at most the 95th percentile of per-commit introduced line counts in its repository, and never more than 10,000 lines. Sample floor: 5 VERIFIED commits. VERIFIED only; PROBABLE is listed but never summed. Full clones only. Baseline (method v3): the untagged lines of the same repository, by age; the gap is defined in the Baseline section. Details, limits and how to contest a number: https://causari.dev/method.

## What this report is, and is not

This report counts lines. For each repository it states how many lines were introduced by commits that carry machine-readable AI authorship metadata (trailers such as Co-Authored-By naming an agent, bot author identities, aider markers, and, from method v4, git-ai notes that name a non-empty tool; method v3 counted any git-ai note), and how many of those lines git blame still attributes to those commits at HEAD, under the method version stated on the page (blame with -w -M -C, a per-commit weight cap, a sample floor, full clones only; from method v3 the untagged lines of the same repository, in the matched age windows, stand next to the AI-tagged ones). Every row is reproducible with one command.

It is not a quality judgement, a productivity measure, total AI usage, proof of authorship, a security finding, a correctness finding, or a causal effect of AI. Deleted lines include removed features and rewritten prototypes; surviving lines include dead code. It is not a sample of all AI-assisted code: inline completions leave no trace in git, untagged agent commits are invisible, and the repositories were selected, not drawn at random: 30 hand-picked and 34 found by GitHub commit search as public repositories with at least 5 commits carrying the same AI authorship metadata and at least 100 stars, most-starred first (discovered 2026-09-27); the selection rule and the counts behind it are public. The latest discovery run did not finish, and this list was not replaced. The intervals describe the sampled repositories only.

Prior measurement work asks related questions with different instruments. GitClear publishes churn reports built from code-change patterns across the repositories it analyses; arXiv 2601.16809 ("Will It Survive?") follows the modification of agent-authored code in 201 projects with its own detector and finds that such code is modified less often than human-written code. This report does not reproduce either method and does not adjudicate between them: it publishes counts from git metadata alone, with the method version, the tool version and the exact bytes behind every number, so that the three can be read side by side.

## Cite

Crovia Trust. Survival Report #9 (2026-11-09). https://causari.dev/reports/survival/2026/09/ DOI 10.5281/zenodo.24091343
