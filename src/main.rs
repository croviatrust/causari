mod banner;
mod capture;
mod cli;
mod commands;
mod commit;
mod config;
mod dag;
mod index;
mod object;
mod proof;
mod repo;
mod skill;
mod snapshot;
mod store;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init => commands::init::run(),
        Command::Record(args) => commands::record::run(args),
        Command::Log(args) => commands::log::run(args),
        Command::Show(args) => commands::show::run(args),
        Command::Revert(args) => commands::revert::run(args),
        Command::Diff(args) => commands::diff::run(args),
        Command::Why(args) => commands::why::run(args),
        Command::Watch(args) => commands::watch::run(args),
        Command::Bisect(args) => commands::bisect::run(args),
        Command::Fork(args) => commands::fork::run(args),
        Command::Sessions => commands::sessions::run(),
        Command::Switch(args) => commands::switch::run(args),
        Command::Trace(args) => commands::trace::run(args),
        Command::Find(args) => commands::find::run(args),
        Command::Impact(args) => commands::impact::run(args),
        Command::Lens(args) => commands::lens::run(args),
        Command::Skill(args) => commands::skill::run(args),
        Command::Proof(args) => commands::proof::run(args),
        Command::Mcp(args) => commands::mcp::run(args),
        Command::Guard(args) => commands::guard::run(args),
        Command::Churn(args) => commands::churn::run(args),
        Command::Report(args) => commands::report::run(args),
        Command::Proxy(args) => commands::proxy::run(args),
        Command::Hook(args) => commands::hook::run(args),
        Command::HookEvent(args) => commands::hook::run_event(args),
    }
}
