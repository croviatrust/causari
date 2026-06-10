use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "re",
    version,
    about = "Causari — intent-addressable code for AI agents",
    long_about = "Causari records every action an AI agent takes on your codebase \
                  and lets you inspect, diff, and revert them like git commits."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Initialize a new Causari repository in the current directory
    Init,

    /// Record an agent action (reads a JSON event from stdin or flags)
    Record(RecordArgs),

    /// Show the history of recorded agent actions
    Log(LogArgs),

    /// Show details of a specific event
    Show(ShowArgs),

    /// Revert the workspace to the state before a given event
    Revert(RevertArgs),

    /// Show the diff introduced by an event (or between two events)
    Diff(DiffArgs),

    /// Explain who/what created a specific line: `re why path/to/file:42`
    Why(WhyArgs),

    /// Auto-record every filesystem change as a Causari event (passive recorder)
    Watch(WatchArgs),

    /// Binary-search the event that broke a test command
    Bisect(BisectArgs),

    /// Create a new session branch and switch HEAD to it (multiverse fork)
    Fork(ForkArgs),

    /// List all session branches (the tips of the event DAG)
    Sessions,

    /// Switch HEAD to an existing session and sync the workspace to its tip
    Switch(SwitchArgs),

    /// Show the FULL causal cone of a line: every event that contributed,
    /// transitively, via the files it read or wrote.
    Trace(TraceArgs),

    /// Search events by free text in prompt, message, reasoning or tool.
    Find(FindArgs),

    /// Show the DOWNSTREAM causal cone of an event (what flowed from it).
    Impact(ImpactArgs),

    /// Render a file with per-line provenance annotations.
    Lens(LensArgs),

    /// Distill, inspect and verify signed skills (the experience layer)
    Skill(SkillArgs),

    /// Run Causari as an MCP server (Claude Code, Cursor, Cline, Windsurf, …)
    Mcp(McpArgs),

    /// Scan recent events for risky patterns (watchdog)
    Guard(GuardArgs),

    /// Measure how much AI-written code survived vs was rewritten (waste analysis)
    Churn(ChurnArgs),

    /// Generate a shareable HTML dashboard of AI code-survival and waste
    Report(ReportArgs),

    /// Run a local LLM capture proxy (OpenAI/Anthropic compatible).
    /// Every prompt, completion, token and dollar flows through Causari.
    Proxy(ProxyArgs),

    /// Install agent-side capture hooks (e.g. `re hook claude-code`)
    Hook(HookArgs),

    /// Internal: invoked by agent hooks with a JSON payload on stdin
    #[command(hide = true)]
    HookEvent(HookEventArgs),
}

#[derive(Args, Debug)]
pub struct RecordArgs {
    /// Short description of the action
    #[arg(short, long)]
    pub message: Option<String>,

    /// Tool used by the agent (e.g. "edit", "shell", "write_file")
    #[arg(short, long)]
    pub tool: Option<String>,

    /// Agent identifier (e.g. "claude-3.5-sonnet", "gpt-4o")
    #[arg(short, long)]
    pub agent: Option<String>,

    /// Read full event JSON from stdin instead of using flags
    #[arg(long)]
    pub stdin: bool,

    /// Record onto a named session instead of HEAD (one session per agent
    /// enables safe concurrent recording). Created on first use.
    #[arg(short = 's', long)]
    pub session: Option<String>,
}

#[derive(Args, Debug)]
pub struct LogArgs {
    /// Maximum number of events to display
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: usize,

    /// Show one line per event
    #[arg(long)]
    pub oneline: bool,

    /// Show the full DAG: events from ALL sessions, with tip and fork markers
    #[arg(long)]
    pub all: bool,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Event id (full or short prefix)
    pub id: String,
}

#[derive(Args, Debug)]
pub struct RevertArgs {
    /// Event id to revert TO (workspace will look like it did before this event)
    pub id: String,

    /// Skip confirmation prompt
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct DiffArgs {
    /// Event id (or RANGE like a..b)
    pub spec: String,
}

#[derive(Args, Debug)]
pub struct WhyArgs {
    /// Location to explain, in the form `path/to/file:line`
    pub spec: String,
}

#[derive(Args, Debug)]
pub struct WatchArgs {
    /// Tag every auto-recorded event with this agent identifier
    #[arg(short, long)]
    pub agent: Option<String>,

    /// Tag every auto-recorded event with this model identifier
    #[arg(short, long)]
    pub model: Option<String>,

    /// Debounce window in milliseconds (default: 800)
    #[arg(long)]
    pub debounce: Option<u64>,

    /// Correlation window in seconds: how far back to search captured LLM
    /// exchanges when attributing a file change (default: 300)
    #[arg(long)]
    pub window: Option<u64>,

    /// Record onto a named session instead of HEAD (one session per agent
    /// enables safe concurrent recording). Created on first use.
    #[arg(short = 's', long)]
    pub session: Option<String>,
}

#[derive(Args, Debug)]
pub struct BisectArgs {
    /// Known-good event id
    #[arg(long)]
    pub good: String,

    /// Known-bad event id
    #[arg(long)]
    pub bad: String,

    /// Shell command whose success defines "good"
    #[arg(long)]
    pub test: String,
}

#[derive(Args, Debug)]
pub struct ForkArgs {
    /// New branch name
    pub name: String,

    /// Event id to fork from (default: HEAD)
    #[arg(long)]
    pub from: Option<String>,
}

#[derive(Args, Debug)]
pub struct SwitchArgs {
    /// Session name to switch to
    pub name: String,

    /// Keep the working tree as-is (only move HEAD)
    #[arg(long)]
    pub no_sync: bool,
}

#[derive(Args, Debug)]
pub struct TraceArgs {
    /// Location whose causal cone you want, in the form `path/to/file:line`
    pub spec: String,
}

#[derive(Args, Debug)]
pub struct FindArgs {
    /// Free-text query (matched against prompt, message, reasoning, tool)
    pub query: String,

    /// Maximum number of results to display
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,
}

#[derive(Args, Debug)]
pub struct ImpactArgs {
    /// Event id whose downstream cone you want to inspect
    pub event: String,
}

#[derive(Args, Debug)]
pub struct LensArgs {
    /// Path to the file you want annotated with per-line provenance
    pub file: String,
}

#[derive(Args, Debug)]
pub struct ChurnArgs {
    /// Emit a Markdown summary (for CI / PR comments)
    #[arg(long)]
    pub summary: bool,
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// Output file path (default: causari-report.html)
    #[arg(short, long)]
    pub output: Option<String>,

    /// Open the report in the default browser after writing it
    #[arg(long)]
    pub open: bool,
}

#[derive(Args, Debug)]
pub struct GuardArgs {
    /// Number of recent events to scan (default: 20)
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,

    /// Generate an SVG badge at .causari/guard-badge.svg
    #[arg(long)]
    pub badge: bool,

    /// Emit Markdown summary to stdout (for CI / PR comments)
    #[arg(long)]
    pub summary: bool,
}

#[derive(Args, Debug)]
pub struct ProxyArgs {
    /// Port to listen on (default: 4242)
    #[arg(short, long)]
    pub port: Option<u16>,

    /// Upstream base URL for OpenAI-style requests (default: https://api.openai.com)
    #[arg(long)]
    pub openai_upstream: Option<String>,

    /// Upstream base URL for Anthropic-style requests (default: https://api.anthropic.com)
    #[arg(long)]
    pub anthropic_upstream: Option<String>,
}

#[derive(Args, Debug)]
pub struct HookArgs {
    /// Agent runtime to hook into (supported: claude-code)
    pub target: String,
}

#[derive(Args, Debug)]
pub struct HookEventArgs {
    /// Hook kind: user-prompt | post-tool
    pub kind: String,
}

#[derive(Args, Debug)]
pub struct SkillArgs {
    #[command(subcommand)]
    pub command: SkillCommand,
}

#[derive(Subcommand, Debug)]
pub enum SkillCommand {
    /// Distill new skills from the event ledger (idempotent)
    Distill,

    /// List all skills with their trust level
    List,

    /// Show one skill in full detail
    Show {
        /// Skill id (full or prefix, min 4 chars)
        id: String,
    },

    /// Verify the Ed25519 signature of one skill, or of all skills
    Verify {
        /// Skill id (omit to verify every skill)
        id: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct McpArgs {
    /// Print the JSON snippet to register Causari in Claude/Cursor/Cline,
    /// then exit. Without this flag, Causari runs as an MCP server on stdio.
    #[arg(long)]
    pub install: bool,
}
