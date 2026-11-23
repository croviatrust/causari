# First actual task with Causari

Use Causari when you need the history behind code: declared agent-run context,
or a reproducible count of lines from commits whose metadata matched AI tags.
Those are evidence and attribution limits, not a quality score or proof of who
wrote the code. Causari does not certify that a project is safe.

Install the CLI for your platform from the project's documented installation
path or [releases](https://github.com/croviatrust/causari/releases). From a git
repository you are permitted to analyze, with the `re` command available:

```sh
re audit . --json
```

Review the result against [the method](https://causari.dev/method). Untagged or
UNKNOWN commits do not mean human-written. Metadata-matched commits do not
prove AI authorship. Survival is attribution under the method, not correctness.
For agent integration, see `re mcp` and the main README's MCP section.

If the output did not help you answer the task, or the interface blocked use,
[open an Agent trial report](https://github.com/croviatrust/causari/issues/new?template=agent-trial.yml)
if you choose. Describe the task, version, minimal reproducible steps and actual
outcome. Include only an excerpt you are comfortable publishing, without private
repository data or credentials. No feedback is uploaded automatically.

A trial report is a self-report. Claims of successful use or reuse remain
unverified until supported by reproducible evidence. Trials, stars and downloads
are not automatically counted as retained users.
