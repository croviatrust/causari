# Adversarial cases

Public fixtures for the cases the audit and the ledger already encode.
Each one states the evidence that was present, the evidence that was
absent, and the conclusion the current source draws. A miss is not a
human. A metadata match is not authorship.

These are the regression tests. They are not a claim that Causari wins.

## Audit, method v4

| Case | Evidence present | Evidence absent | Conclusion | Limit |
|---|---|---|---|---|
| Schema-only git-ai note | `schema_version`, no tool | a named tool, a trailer | UNKNOWN | v3 counted this as agent `ai` |
| Human-only git-ai note | a `humans` entry | a named tool | UNKNOWN | the note is not a finding of human authorship either; this detector simply has no AI signal |
| Named tool | `agent_id.tool = mock_ai` on lines 20–30 | coverage of the other lines | the commit is tagged `mock_ai` | survival then counts the commit's introduced lines, not the 11-line range |
| Empty tool, then a named tool | `tool: ""` in sessions, `mock_ai` in prompts | — | `mock_ai` | an empty string is not a tool and must not hide one |
| Schema-only note plus `Co-Authored-By: Claude` | the trailer | a named tool in the note | `claude-code` from the trailer | the note does not outrank an independent signal |
| Author `Claude` with a personal email | the name | a bot address, a trailer, a named tool | UNKNOWN | the same for Devin, Jules, Gemini, Cursor |
| `Claude <noreply@anthropic.com>` | the bot address | — | `claude-code`, metadata | not a signature |
| `Co-Authored-By: Claude` on a hand-written commit | the trailer | — | VERIFIED metadata | a person can write the trailer |
| `AI-Assisted: no` | the negative value | — | UNKNOWN | `AI-Assisted: yes` is generic `ai` |
| Malformed note | the word `schema_version`, not JSON | a parseable tool | UNKNOWN | fail closed |
| Note added and removed | `refs/notes/ai` | a new commit object | SHA unchanged; class follows the note | the note is not in the commit |
| Two tools, keys `a` and `z` | both tools | a conflict field | the tool on the lexicographically first key | not a proof of which tool wrote the lines; sessions are read before prompts |

Tests: `schema_only_git_ai_note_is_not_ai`, `human_only_git_ai_note_is_not_ai`, `named_tool_git_ai_note_is_ai_metadata`, `empty_or_blank_tool_is_not_ai_and_does_not_hide_a_later_tool`, `schema_only_note_does_not_hide_an_independent_trailer`, `malformed_or_unschematized_git_ai_note_is_not_ai`, `competing_git_ai_tools_follow_session_key_order`, `note_mutation_does_not_change_commit_sha`, `named_tool_range_classifies_the_whole_commit`, `human_authors_named_like_agents_are_unknown`, `fake_trailer_is_verified_metadata`, `negative_disclosure_trailers_are_not_ai`.

## Ledger

| Case | Evidence present | Evidence absent | Conclusion | Limit |
|---|---|---|---|---|
| Human edit, then a shell with no captured pre-state | the human bytes, the shell event | a PreToolUse snapshot | the human lines stay unassociated; the shell does not take them | declared evidence only |
| Shell with a PreToolUse snapshot | the snapshot and the diff after it | — | the shell owns that diff | not the lines that predate the snapshot |
| Whole-file declaration, file already in the tree, no PreToolUse | `old_string` empty, `new_string` the whole file | a snapshot of the previous bytes | the event does not take every pre-existing line | a first observation of a path that did not exist stays declared |
| Declared edit of a path that is not in the tree | the declaration | the path | it does not take lines of another file | |
| Agent A, human edit, rename, formatter, Agent B | each agent's own diff | a witness that can see through the formatter | A keeps the lines still attributable to A; the human edit stays unassociated; B gets its own change | ambiguous transformations stay unclaimed |

Tests: `shell_without_a_captured_pre_does_not_inherit_the_gap`, `shell_with_a_captured_pre_owns_only_the_change_after_the_snapshot`, `wholesale_declaration_does_not_take_lines_the_ledger_already_has`, `first_observation_of_a_path_stays_declared`, `declared_path_that_is_not_in_the_tree_does_not_steal_another_file`, `human_rename_formatter_between_agents_is_not_the_second_agents`.

Shallow clones are refused. A merge is excluded from the walk and blame can still follow a tagged parent. A rewrite that drops the trailer leaves the lines UNKNOWN. Tests: `shallow_clone_is_refused_and_marked`, `merge_commit_keeps_the_tagged_parent`, `history_rewrite_that_drops_the_trailer_is_unknown`.
