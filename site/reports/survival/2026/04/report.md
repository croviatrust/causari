# Survival Report #4 — 2026-10-05

Counts of surviving lines from AI-tagged commits in 59 open-source repositories, measured with causari 0.3.0, method v3. Counts, not grades: no rank, no verdict; rows are alphabetical.

Page: https://causari.dev/reports/survival/2026/04/  
Data: https://causari.dev/reports/survival/2026/04/report.json  
Feed: https://causari.dev/reports/survival/feed.xml  
Licence: CC-BY-4.0  
DOI: https://doi.org/10.5281/zenodo.23196011  

## Aggregate

17,512,878 of 32,730,982 lines introduced by 68,274 AI-tagged commits in 59 open-source repositories are still at HEAD (53.5 %). 95 % interval over the sampled repositories: 37.4 % to 75.6 %.

- Repositories aggregated: 59
- Commits in those repositories (no merges): 907,458
- AI-tagged (VERIFIED) commits: 68,274
- Lines introduced by them: 32,730,982
- Still attributed to them at HEAD: 17,512,878
- Line-weighted ratio: 53.5 %
- 95 % bootstrap interval over the sampled repositories: 37.4 % to 75.6 %
- Median of per-repository capped ratios: 75.2 %
- 95 % bootstrap interval on that median: 69.0 % to 76.7 %

95 % percentile interval from 2000 bootstrap resamples of the 59 aggregated repositories (with replacement, seed 4). It describes the sampled repositories, not all AI-assisted code, and not the repositories not in this sample.


## Baseline: the same repositories' untagged lines, at the same age

gap = AI-tagged line-weighted survival minus untagged survival re-weighted to the age mix of the AI-tagged lines of the same repository, over age windows where both cohorts hold at least 5 commits, computed inside each repository; untagged = commits with no machine-readable AI signal (human-written, inline-completed and untagged-agent code alike); age = commit date to HEAD date. A negative gap means AI-tagged lines survive less than untagged lines of the same age in the same repository.

- Repositories with an age-matched gap: 58 of 59 with a baseline
- Median gap across them: +3.7 pts
- 95 % bootstrap interval on that median: -0.4 pts to +6.9 pts
- Gaps below zero: 23 · above zero: 35
- Cleared or rewritten (more than 50% of the repository's commits predate the oldest line still at HEAD): OpenHands/OpenHands. Nothing from before that date survives in them, tagged or not; their rows measure the rewrite as much as the code.

Age windows, counts summed across the repositories with a baseline, one row per age window; one large repository can dominate a window, so no gap is computed from these rows: the gap is computed inside each repository and only its median crosses repositories.

| Line age | AI-tagged commits | AI-tagged lines | Still at HEAD | AI-tagged | Untagged commits | Untagged lines | Still at HEAD | Untagged |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0–30 d | 14,175 | 5,907,452 | 4,551,595 | 77.0 % | 21,120 | 11,269,058 | 9,215,649 | 81.8 % |
| 30–90 d | 11,883 | 9,003,398 | 3,735,895 | 41.5 % | 36,894 | 27,199,771 | 14,689,006 | 54.0 % |
| 90–180 d | 15,441 | 9,874,364 | 4,368,766 | 44.2 % | 48,473 | 46,476,726 | 38,945,166 | 83.8 % |
| 180–365 d | 13,391 | 5,863,795 | 3,490,477 | 59.5 % | 91,874 | 22,514,641 | 11,769,773 | 52.3 % |
| 365–730 d | 10,293 | 2,011,955 | 1,332,958 | 66.3 % | 131,840 | 20,738,652 | 9,942,489 | 47.9 % |
| 730+ d | 3,091 | 70,018 | 33,187 | 47.4 % | 508,891 | 83,641,899 | 19,236,089 | 23.0 % |

## Repositories (alphabetical)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Line-weighted | Capped | Median per commit | Largest commit | Untagged, same age | Gap | Reproduce |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Aider-AI/aider | 12,461 | 11,156 | 374,687 | 235,782 | 62.9 % | 41.9 % | 33.3 % | 27 % | 52.7 % | +10.3 pts | `re audit Aider-AI/aider --json` |
| airbytehq/airbyte | 53,392 | 2,277 | 1,090,411 | 931,149 | 85.4 % | 75.2 % | 85.7 % | 46 % | 62.1 % | +23.3 pts | `re audit airbytehq/airbyte --json` |
| anthropics/anthropic-sdk-python | 1,465 | 45 | 5,369 | 4,424 | 82.4 % | 78.0 % | 82.2 % | 30 % | 88.6 % | -5.8 pts | `re audit anthropics/anthropic-sdk-python --json` |
| anthropics/anthropic-sdk-typescript | 1,395 | 60 | 14,871 | 12,340 | 83.0 % | 84.8 % | 94.7 % | 37 % | 89.8 % | -2.0 pts | `re audit anthropics/anthropic-sdk-typescript --json` |
| anthropics/claude-code | 763 | 78 | 31,739 | 29,673 | 93.5 % | 75.6 % | 58.1 % | 69 % | 79.8 % | +13.7 pts | `re audit anthropics/claude-code --json` |
| apache/fluss | 2,207 | 34 | 12,656 | 10,100 | 79.8 % | 79.7 % | 92.3 % | 12 % | 82.6 % | -3.4 pts | `re audit apache/fluss --json` |
| apache/spark | 48,772 | 21 | 1,634 | 1,412 | 86.4 % | 86.6 % | 94.1 % | 16 % | 85.3 % | +0.1 pts | `re audit apache/spark --json` |
| apache/texera | 7,238 | 535 | 206,294 | 164,168 | 79.6 % | 82.8 % | 90.9 % | 8 % | 82.4 % | -2.8 pts | `re audit apache/texera --json` |
| BerriAI/litellm | 43,948 | 5,193 | 1,774,352 | 1,157,040 | 65.2 % | 68.8 % | 68.7 % | 4 % | 58.3 % | +6.9 pts | `re audit BerriAI/litellm --json` |
| browser-use/browser-use | 7,031 | 263 | 19,774 | 4,278 | 21.6 % | 28.9 % | 14.3 % | 19 % | 44.8 % | -23.2 pts | `re audit browser-use/browser-use --json` |
| cfug/flutter.cn | 10,588 | 149 | 68,543 | 59,214 | 86.4 % | 75.1 % | 87.9 % | 48 % | 66.8 % | +19.6 pts | `re audit cfug/flutter.cn --json` |
| cline/cline | 7,426 | 156 | 97,936 | 65,473 | 66.9 % | 68.5 % | 71.3 % | 12 % | 55.2 % | +11.7 pts | `re audit cline/cline --json` |
| cloudflare/workers-sdk | 8,528 | 199 | 91,807 | 72,882 | 79.4 % | 78.1 % | 72.1 % | 8 % | 68.5 % | +10.9 pts | `re audit cloudflare/workers-sdk --json` |
| continuedev/continue | 16,266 | 228 | 27,756 | 18,063 | 65.1 % | 63.5 % | 68.1 % | 11 % | 58.4 % | +6.7 pts | `re audit continuedev/continue --json` |
| cozystack/cozystack | 5,305 | 2,555 | 573,125 | 443,806 | 77.4 % | 77.8 % | 85.0 % | 6 % | 82.6 % | -5.1 pts | `re audit cozystack/cozystack --json` |
| crewAIInc/crewAI | 2,831 | 199 | 3,348,346 | 153,591 | 4.6 % | 76.2 % | 89.9 % | 97 % | 63.9 % | -59.3 pts | `re audit crewAIInc/crewAI --json` |
| croviatrust/causari | 252 | 14 | 6,267 | 5,160 | 82.3 % | 82.3 % | 92.9 % | 25 % | 84.7 % | +0.3 pts | `re audit croviatrust/causari --json` |
| danny-avila/LibreChat | 5,741 | 88 | 84,488 | 78,508 | 92.9 % | 90.4 % | 88.2 % | 24 % | 87.7 % | +5.3 pts | `re audit danny-avila/LibreChat --json` |
| fastrepl/anarlog | 9,748 | 1,501 | 825,505 | 523,988 | 63.5 % | 47.6 % | 12.4 % | 15 % | 53.8 % | +9.7 pts | `re audit fastrepl/anarlog --json` |
| flutter/flutter | 89,327 | 155 | 85,979 | 17,871 | 20.8 % | 65.5 % | 85.4 % | 39 % | 38.3 % | -17.6 pts | `re audit flutter/flutter --json` |
| flutter/website | 8,554 | 150 | 50,112 | 47,582 | 95.0 % | 81.6 % | 93.0 % | 65 % | 66.7 % | +28.2 pts | `re audit flutter/website --json` |
| ghostty-org/ghostty | 13,580 | 83 | 4,330 | 3,127 | 72.2 % | 74.7 % | 88.2 % | 11 % | 48.1 % | +24.1 pts | `re audit ghostty-org/ghostty --json` |
| github/gh-aw-mcpg | 5,675 | 5,220 | 444,689 | 247,618 | 55.7 % | 62.8 % | 70.7 % | 3 % | 44.0 % | +11.7 pts | `re audit github/gh-aw-mcpg --json` |
| google-gemini/gemini-cli | 6,460 | 350 | 74,449 | 47,537 | 63.9 % | 62.4 % | 64.3 % | 4 % | 66.1 % | -3.7 pts | `re audit google-gemini/gemini-cli --json` |
| google/sam | 1,511 | 35 | 247 | 96 | 38.9 % | 42.2 % | 0.0 % | 71 % | 36.9 % | +2.0 pts | `re audit google/sam --json` |
| GoogleCloudPlatform/scion | 4,979 | 85 | 23,869 | 19,896 | 83.4 % | 76.4 % | 84.6 % | 36 % | 84.9 % | -1.6 pts | `re audit GoogleCloudPlatform/scion --json` |
| jdubois/boot-ui | 1,530 | 1,142 | 569,829 | 457,194 | 80.2 % | 79.9 % | 81.4 % | 9 % | 67.9 % | +12.3 pts | `re audit jdubois/boot-ui --json` |
| langchain-ai/langchain | 16,801 | 43 | 7,289 | 5,388 | 73.9 % | 69.8 % | 87.4 % | 24 % | 55.6 % | +15.7 pts | `re audit langchain-ai/langchain --json` |
| langgenius/dify | 13,854 | 573 | 646,492 | 354,786 | 54.9 % | 54.8 % | 55.9 % | 16 % | 45.2 % | +9.6 pts | `re audit langgenius/dify --json` |
| lidge-ai/cli-jaw | 5,702 | 1,361 | 445,819 | 197,939 | 44.4 % | 75.2 % | 68.2 % | 47 % | 54.5 % | -10.1 pts | `re audit lidge-ai/cli-jaw --json` |
| lidge-ai/ima2-gen | 2,059 | 455 | 127,661 | 46,829 | 36.7 % | 53.0 % | 60.0 % | 19 % | 57.4 % | -20.7 pts | `re audit lidge-ai/ima2-gen --json` |
| lobehub/lobe-chat | 14,118 | 2,031 | 2,398,361 | 2,092,529 | 87.2 % | 79.8 % | 81.7 % | 2 % | 81.9 % | +5.4 pts | `re audit lobehub/lobe-chat --json` |
| managarm/managarm | 6,439 | 660 | 52,317 | 40,956 | 78.3 % | 78.3 % | 90.1 % | 6 % | 79.7 % | -1.4 pts | `re audit managarm/managarm --json` |
| mem0ai/mem0 | 2,642 | 94 | 37,024 | 23,948 | 64.7 % | 64.9 % | 82.8 % | 9 % | 65.3 % | -4.8 pts | `re audit mem0ai/mem0 --json` |
| microsoft/BCApps | 4,212 | 675 | 378,503 | 330,733 | 87.4 % | 82.0 % | 86.4 % | 29 % | 91.7 % | -4.5 pts | `re audit microsoft/BCApps --json` |
| microsoft/vscode | 149,098 | 6,999 | 2,681,466 | 2,128,434 | 79.4 % | 72.1 % | 75.7 % | 33 % | 63.0 % | +16.4 pts | `re audit microsoft/vscode --json` |
| microsoft/vscode-copilot-chat | 3,713 | 336 | 995,871 | 939,159 | 94.3 % | 69.0 % | 76.3 % | 88 % | 55.7 % | +38.6 pts | `re audit microsoft/vscode-copilot-chat --json` |
| NovaSky-AI/SkyRL | 1,339 | 397 | 135,259 | 107,374 | 79.4 % | 76.2 % | 80.5 % | 4 % | 62.7 % | +16.7 pts | `re audit NovaSky-AI/SkyRL --json` |
| NVIDIA-NeMo/Switchyard | 520 | 25 | 15,168 | 10,046 | 66.2 % | 66.9 % | 78.7 % | 21 % | 77.2 % | -11.0 pts | `re audit NVIDIA-NeMo/Switchyard --json` |
| openai/codex | 11,843 | 364 | 183,908 | 109,054 | 59.3 % | 58.7 % | 58.3 % | 6 % | 54.4 % | +5.0 pts | `re audit openai/codex --json` |
| openai/openai-agents-python | 2,348 | 6 | 97,483 | 97,002 | 99.5 % | 96.4 % | 89.4 % | 97 % | no shared window | — | `re audit openai/openai-agents-python --json` |
| OpenHands/extensions | 390 | 313 | 87,715 | 69,897 | 79.7 % | 79.2 % | 86.5 % | 4 % | 68.5 % | +11.2 pts | `re audit OpenHands/extensions --json` |
| OpenHands/OpenHands · rewritten | 8,385 | 2,671 | 953,450 | 293,622 | 30.8 % | 29.0 % | 0.0 % | 14 % | 38.7 % | -7.9 pts | `re audit OpenHands/OpenHands --json` |
| OpenHands/software-agent-sdk | 2,501 | 1,734 | 494,896 | 393,242 | 79.5 % | 76.4 % | 77.8 % | 11 % | 77.6 % | +1.9 pts | `re audit OpenHands/software-agent-sdk --json` |
| PrefectHQ/prefect | 18,435 | 1,664 | 459,450 | 375,141 | 81.7 % | 80.5 % | 89.7 % | 3 % | 69.5 % | +12.1 pts | `re audit PrefectHQ/prefect --json` |
| pydantic/pydantic-ai | 3,889 | 347 | 684,751 | 581,635 | 84.9 % | 76.4 % | 76.1 % | 63 % | 82.3 % | +2.7 pts | `re audit pydantic/pydantic-ai --json` |
| QwenLM/qwen-code | 9,652 | 565 | 661,284 | 543,891 | 82.2 % | 76.7 % | 52.6 % | 21 % | 71.5 % | +10.8 pts | `re audit QwenLM/qwen-code --json` |
| ray-project/ray | 31,863 | 859 | 528,742 | 329,573 | 62.3 % | 76.8 % | 89.5 % | 27 % | 65.1 % | -2.8 pts | `re audit ray-project/ray --json` |
| RooCodeInc/Roo-Code | 6,210 | 43 | 88,016 | 39,395 | 44.8 % | 57.0 % | 62.6 % | 67 % | 45.6 % | -0.9 pts | `re audit RooCodeInc/Roo-Code --json` |
| run-llama/llama_index | 7,953 | 13 | 8,542 | 7,872 | 92.2 % | 92.2 % | 90.4 % | 73 % | 80.1 % | +12.0 pts | `re audit run-llama/llama_index --json` |
| secdev/scapy | 5,730 | 64 | 8,555 | 8,180 | 95.6 % | 91.6 % | 100.0 % | 29 % | 89.0 % | -0.4 pts | `re audit secdev/scapy --json` |
| smithersai/smithers | 16,490 | 10,828 | 7,993,375 | 1,707,036 | 21.4 % | 41.9 % | 24.4 % | 42 % | 20.0 % | +1.3 pts | `re audit smithersai/smithers --json` |
| sst/opencode | 15,762 | 50 | 25,998 | 9,466 | 36.4 % | 35.4 % | 39.5 % | 43 % | 40.5 % | -14.5 pts | `re audit sst/opencode --json` |
| TracecatHQ/tracecat | 5,901 | 555 | 367,704 | 249,762 | 67.9 % | 66.4 % | 65.4 % | 4 % | 68.1 % | -0.2 pts | `re audit TracecatHQ/tracecat --json` |
| vercel/next.js | 35,834 | 247 | 62,707 | 48,146 | 76.8 % | 74.0 % | 82.5 % | 7 % | 61.7 % | +15.1 pts | `re audit vercel/next.js --json` |
| vllm-project/llm-compressor | 3,245 | 158 | 17,400 | 13,935 | 80.1 % | 74.3 % | 85.7 % | 30 % | 75.5 % | +4.6 pts | `re audit vllm-project/llm-compressor --json` |
| xing-shuyin/pi-web-ui | 952 | 31 | 2,459 | 2,181 | 88.7 % | 89.5 % | 96.4 % | 8 % | 84.7 % | +4.7 pts | `re audit xing-shuyin/pi-web-ui --json` |
| youknowone/pyre | 85,501 | 2,021 | 2,126,278 | 1,518,879 | 71.4 % | 68.9 % | 71.8 % | 15 % | 65.0 % | +6.4 pts | `re audit youknowone/pyre --json` |
| zed-industries/zed | 37,104 | 121 | 47,975 | 24,876 | 51.9 % | 58.1 % | 69.4 % | 16 % | 66.2 % | -14.7 pts | `re audit zed-industries/zed --json` |

## By agent, across aggregated repositories (alphabetical)

| Agent | Repositories | Commits | Introduced | Still at HEAD | Line-weighted |
|---|---:|---:|---:|---:|---:|
| ai | 4 | 61 | 8,301 | 7,047 | 84.9 % |
| aider | 4 | 11,167 | 375,563 | 235,811 | 62.8 % |
| claude-code | 51 | 23,230 | 16,363,698 | 8,271,489 | 50.5 % |
| copilot | 4 | 4,006 | 252,865 | 148,908 | 58.9 % |
| cursor | 37 | 1,297 | 4,355,982 | 814,410 | 18.7 % |
| devin | 14 | 8,187 | 2,727,811 | 2,059,964 | 75.5 % |
| gemini | 14 | 1,559 | 476,688 | 292,368 | 61.3 % |
| github-copilot | 39 | 11,100 | 5,520,294 | 4,194,675 | 76.0 % |
| grok | 1 | 181 | 221,302 | 172,880 | 78.1 % |
| jules | 8 | 38 | 3,534 | 2,070 | 58.6 % |
| llm | 1 | 740 | 145,514 | 113,512 | 78.0 % |
| openai-codex | 22 | 2,177 | 848,705 | 510,930 | 60.2 % |
| opencode | 2 | 6 | 3,249 | 2,880 | 88.6 % |
| openhands | 5 | 4,493 | 1,425,137 | 683,871 | 48.0 % |
| pi | 2 | 32 | 2,339 | 2,063 | 88.2 % |

## Measured but not aggregated (fewer than 5 AI-tagged commits)

| Repository | Commits | AI-tagged | Introduced | Still at HEAD | Reproduce |
|---|---:|---:|---:|---:|---|
| openai/openai-python | 1,686 | 3 | 63 | 61 | `re audit openai/openai-python --json` |
| stackblitz/bolt.new | 101 | 0 | 0 | 0 | `re audit stackblitz/bolt.new --json` |

## Excluded from this report

- Shallow clones (history truncated; method v3 refuses them): none
- Audits that failed in this run: fern-api/fern, llvm/llvm-project, richlander/dotnet-inspect
- Opted out by their maintainers (https://github.com/croviatrust/causari/blob/main/.github/survival-optout.txt): 0

## Where this measurement connected

The witness recorded, in 20 of 20 uploaded shard logs, 1 destination(s) over 128 connection(s): 1 allowed, 0 blocked, 0 failed, under an allowlist of 1 rule(s) in enforce mode. Every destination the witness saw is one the policy allows.

| Destination | Outcome | Connections | Bytes out | Bytes in |
|---|---|---:|---:|---:|
| github.com:443 | allowed | 128 | 515,039 | 30,149,527,997 |

Signed run sheet: https://causari.dev/reports/survival/2026/04/reach.sheet.json (profile crovia.pnx.v1, witness `urn:causari:survival-report:witness:run-37308519983`, capture `proxy-connect`, disclosure `clear`).  
Policy: https://causari.dev/reports/survival/2026/04/egress-policy.json (source https://github.com/croviatrust/causari/blob/main/.github/egress-policy.json), bound in the sheet as `sha256:7a501cbe66356070f9c6c6a5372a8dd425be059bb2315a71bffcccff64692e21`.  
Verify: `tacet-pnx verify reach.sheet.json --policy egress-policy.json` or `re pnx verify reach.sheet.json --policy egress-policy.json`, or paste the sheet and the policy at https://croviatrust.com/registry/seal/verify/.  

Covered: the connection attempts in the shard logs that were uploaded and concatenated into this sheet. Not covered: any shard that uploaded no log; what the runner does outside the measurement step (checking out this repository, installing the tool, uploading the shard); and any connection that did not go through the witness. The record says where the measurement connected through the witness and how many bytes crossed, nothing about their content.

## Method

Method v3, causari 0.3.0. Detection from commit metadata only; survival from `git blame -w -M -C` at HEAD. Per-commit cap: a commit weighs at most the 95th percentile of per-commit introduced line counts in its repository, and never more than 10,000 lines. Sample floor: 5 VERIFIED commits. VERIFIED only; PROBABLE is listed but never summed. Full clones only. Baseline (method v3): the untagged lines of the same repository, by age; the gap is defined in the Baseline section. Details, limits and how to contest a number: https://causari.dev/method.

## What this report is, and is not

This report counts lines. For each repository it states how many lines were introduced by commits that carry machine-readable AI authorship metadata (trailers such as Co-Authored-By naming an agent, bot author identities, aider markers, git-ai notes), and how many of those lines git blame still attributes to those commits at HEAD, under the method version stated on the page (blame with -w -M -C, a per-commit weight cap, a sample floor, full clones only; from method v3 the untagged lines of the same repository, at the same age, stand next to the AI-tagged ones). Every row is reproducible with one command.

It is not a quality judgement: deleted lines include removed features and rewritten prototypes; surviving lines include dead code. It is not a sample of all AI-assisted code: inline completions leave no trace in git, untagged agent commits are invisible, and the repositories were selected, not drawn at random: 30 hand-picked and 34 found by GitHub commit search as public repositories with at least 5 commits carrying the same AI authorship metadata and at least 100 stars, most-starred first (discovered 2026-09-27); the selection rule and the counts behind it are public. The intervals describe the sampled repositories only.

Prior measurement work asks related questions with different instruments. GitClear publishes churn reports built from code-change patterns across the repositories it analyses; arXiv 2601.16809 ("Will It Survive?") follows the modification of agent-authored code in 201 projects with its own detector and finds that such code is modified less often than human-written code. This report does not reproduce either method and does not adjudicate between them: it publishes counts from git metadata alone, with the method version, the tool version and the exact bytes behind every number, so that the three can be read side by side.

## Cite

Crovia Trust. Survival Report #4 (2026-10-05). https://causari.dev/reports/survival/2026/04/ DOI 10.5281/zenodo.23196011
