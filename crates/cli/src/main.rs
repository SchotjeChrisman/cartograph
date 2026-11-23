//! The `cartograph` binary: argument parsing and dispatch. The work lives in the modules.

mod args;
mod ask;
mod auto_index;
mod commands;
mod hook;
mod install;
mod mcp;
mod orientation;
mod stale_components;
mod workspace;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;

use args::{Cli, Command};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let explicit = cli.root.as_deref();
    match &cli.command {
        Command::Query(command) => {
            let root = workspace::graph_root(explicit)?;
            let output = ask::answer(&root, &command.request())?;
            print(&ask::render(&output, cli.json)?)
        }
        Command::Index => print(&commands::index::run(
            &workspace::index_root(explicit)?,
            cli.json,
        )?),
        Command::Describe(args) => print(&commands::describe::run(
            &workspace::graph_root(explicit)?,
            args,
            cli.json,
        )?),
        Command::Cluster(args) => print(&commands::cluster::run(
            &workspace::graph_root(explicit)?,
            args,
            cli.json,
        )?),
        Command::Map(args) => commands::map::run(&workspace::graph_root(explicit)?, args.bind),
        Command::Mcp => mcp::serve(
            &workspace::graph_root(explicit)?,
            io::stdin().lock(),
            io::stdout().lock(),
        ),
        Command::Install(args) => print(&install::run(&workspace::index_root(explicit)?, args)?),
        Command::Hook => match hook::run(&workspace::graph_root(explicit)?, io::stdin().lock())? {
            Some(decision) => print(&decision),
            None => Ok(()),
        },
    }
}

fn print(text: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", text.trim_end())?;
    Ok(())
}
