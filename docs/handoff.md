# Commit handoff

Two agents working in Git worktrees can use a named slot to agree on what was
delivered. Use full local commit IDs and the exact previous delivery as the
precondition.

```sh
re handoff review "$first"
re handoff review "$candidate" --expect "$first"
```

The equivalent tool in `re mcp` is `causari_handoff`. Its input fields are
`slot`, `expected` and `next`. Use JSON `null` for `expected` on the first
delivery. The server's working directory selects the repository.

| Status | Meaning | Caller action |
| --- | --- | --- |
| `accepted` | This call advanced the slot | Save the returned receipt |
| `replayed` | The same request was already accepted | Use the original receipt |
| `stale` | The slot differed from `expected` | Inspect `current` before forming a new request |
| `not_fast_forward` | The candidate did not descend from `expected` | Reconcile the histories before proposing a new commit |

The CLI writes one JSON object. It exits zero for `accepted` and `replayed`.
On transport loss or a busy slot, retry the same tuple. `current` is the slot
value observed by that call and can change immediately. A replay receipt
identifies a historical acceptance even if `current` has advanced.

The slot is one direct ref under `refs/causari/handoff/`. Its journal commits
retain the delivered commits, so a replacement process can look up a receipt
by repeating the request. The compare-and-swap on that ref is the point of
publication. Branches, the index and the working tree remain untouched.
The private ref is shared by linked worktrees using the same Git directory.
Ordinary clones have separate slots. A bare repository also works.

This mechanism assumes trusted local access to the Git object and ref store.
Restrict who can write the reserved refs if the receipt matters to another
process. A receipt records delivery and carries no issuer signature or code
review verdict. It covers this local Git effect only. Keep the slot ref to
retain its history. Retrying a historical delivery may require a journal
walk. To retain receipts across machine loss, back up the repo including the
reserved refs.
