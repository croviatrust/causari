<!-- Thanks for contributing! Fill this in honestly. -->

## What does this PR do?

<!-- One paragraph. What changes for the user? -->

## Why?

<!-- Link the issue this fixes: "Closes #123". If there is no issue, justify here. -->

## How was it tested?

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] Demo scripts (`demo.{sh,ps1}`, `demo-trace`, `demo-bidir`, `demo-mcp`) still pass.
- [ ] Manually verified on: <!-- Linux / macOS / Windows -->

## Schema / protocol changes

- [ ] Event schema (`causari.event.vX.Y`) is unchanged, OR I bumped the version and added decode for old events.
- [ ] MCP tools list is unchanged, OR I updated the install snippet and the README MCP section.

## Checklist

- [ ] I read [`CONTRIBUTING.md`](../CONTRIBUTING.md).
- [ ] I signed the [CLA](../CLA.md).
- [ ] My change is in scope (causal provenance / agent observability / MCP).
- [ ] No new dependencies, OR I justified them in the PR description.
