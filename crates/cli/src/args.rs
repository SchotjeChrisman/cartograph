//! The command line. Query subcommands mirror the six tools and build the same
//! [`query::Request`] the MCP server builds.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use graph::ComponentId;
use query::{
    DEFAULT_BUDGET, DEFAULT_DEPTH, DEFAULT_IMPACT_DEPTH, DEFAULT_PATH_DEPTH, Page, Relation,
    Request,
};

/// Where `cartograph map` listens unless told otherwise.
const DEFAULT_MAP_ADDRESS: &str = "0.0.0.0:8787";
/// How many levels `cartograph describe` fills unless told otherwise: system and subsystem.
const DEFAULT_DESCRIBE_LEVELS: u32 = 2;

#[derive(Parser)]
#[command(name = "cartograph", version, about = "Queryable code graph and map")]
pub struct Cli {
    /// Directory that holds (or will hold) `.cartograph/`. Queries default to the nearest
    /// ancestor of the working directory that has one; `index` defaults to the working directory.
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    /// Print the full structured result as JSON instead of dense text.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(flatten)]
    Query(QueryCommand),
    /// Index the tree into its graph; only changed files are parsed again.
    Index,
    /// Write plain-English descriptions of components with a model backend.
    Describe(DescribeArgs),
    /// Group files into the system > subsystem > component > file tree.
    Cluster(ClusterArgs),
    /// Serve the interactive map until interrupted.
    Map(MapArgs),
    /// Serve the query tools to an MCP client over stdio.
    Mcp,
    /// Register the MCP server and agent instructions in a project.
    Install(InstallArgs),
    /// PreToolUse hook handler used by the strict hook; reads the hook event on stdin.
    Hook,
}

/// The commands that ask the graph a question, one per MCP tool plus two shorthands.
#[derive(Subcommand)]
pub enum QueryCommand {
    /// What was parsed, what was not and why, and how many calls resolved.
    Coverage(PageArgs),
    /// Components and the most connected symbols, top level or inside one component.
    Overview(OverviewArgs),
    /// Resolve a name or words to ranked candidates with stable ids.
    Find(FindArgs),
    /// The neighbourhood of a node.
    Explore(ExploreArgs),
    /// Shorthand for `explore --callers`.
    Callers(FollowArgs),
    /// Shorthand for `explore --callees`.
    Callees(FollowArgs),
    /// The shortest path between two nodes.
    Path(PathArgs),
    /// What reaches a node, transitively: what breaks if it changes.
    Impact(ImpactArgs),
}

#[derive(Args, Clone, Copy)]
pub struct BudgetArgs {
    /// Approximate size of the answer in tokens.
    #[arg(long, default_value_t = DEFAULT_BUDGET)]
    pub budget: usize,
}

#[derive(Args, Clone, Copy)]
pub struct PageArgs {
    #[command(flatten)]
    pub budget: BudgetArgs,
    /// Skip this many ranked results: the cursor a previous footer printed.
    #[arg(long, default_value_t = 0)]
    pub after: usize,
}

impl PageArgs {
    pub fn page(self) -> Page {
        Page {
            budget: self.budget.budget,
            after: self.after,
        }
    }
}

#[derive(Args)]
pub struct OverviewArgs {
    /// Show the children of this component id instead of the top level.
    #[arg(long)]
    pub component: Option<i64>,
    #[command(flatten)]
    pub page: PageArgs,
}

#[derive(Args)]
pub struct FindArgs {
    pub text: String,
    #[command(flatten)]
    pub page: PageArgs,
}

#[derive(Args)]
pub struct FollowArgs {
    /// A name, qualified name, `path:line` or `@id`.
    pub target: String,
    #[arg(long, default_value_t = DEFAULT_DEPTH)]
    pub depth: u32,
    /// Keep only nodes of this component id.
    #[arg(long)]
    pub component: Option<i64>,
    #[command(flatten)]
    pub page: PageArgs,
}

#[derive(Args)]
pub struct ExploreArgs {
    #[command(flatten)]
    pub follow: FollowArgs,
    #[arg(long, group = "relation")]
    pub callers: bool,
    #[arg(long, group = "relation")]
    pub callees: bool,
    #[arg(long, group = "relation")]
    pub contains: bool,
}

#[derive(Args)]
pub struct PathArgs {
    pub from: String,
    pub to: String,
    #[arg(long, default_value_t = DEFAULT_PATH_DEPTH)]
    pub depth: u32,
    #[command(flatten)]
    pub budget: BudgetArgs,
}

#[derive(Args)]
pub struct ImpactArgs {
    pub target: String,
    #[arg(long, default_value_t = DEFAULT_IMPACT_DEPTH)]
    pub depth: u32,
    #[command(flatten)]
    pub page: PageArgs,
}

#[derive(Args)]
pub struct DescribeArgs {
    /// `claude`, `codex` or `ollama:<model>`.
    #[arg(long, default_value = "claude")]
    pub backend: String,
    /// Start at this component id instead of the top level.
    #[arg(long)]
    pub component: Option<i64>,
    /// How many levels to describe, counting the start.
    #[arg(long, default_value_t = DEFAULT_DESCRIBE_LEVELS, conflicts_with = "all")]
    pub depth: u32,
    /// Describe every level.
    #[arg(long)]
    pub all: bool,
}

#[derive(Args)]
pub struct ClusterArgs {
    /// Put vendored and generated files into components too.
    #[arg(long)]
    pub include_vendored: bool,
}

#[derive(Args)]
pub struct MapArgs {
    #[arg(long, default_value = DEFAULT_MAP_ADDRESS)]
    pub bind: SocketAddr,
}

#[derive(Args)]
pub struct InstallArgs {
    /// MCP client config to add the server to. Default: `.mcp.json` in the project.
    #[arg(long)]
    pub mcp_config: Option<PathBuf>,
    /// Instructions file that gets the usage snippet. Default: `CLAUDE.md` in the project.
    #[arg(long)]
    pub instructions: Option<PathBuf>,
    /// Also register the server in this Codex config file (for example `~/.codex/config.toml`).
    /// Off unless given; point `--instructions` at `AGENTS.md` for Codex's usage snippet.
    #[arg(long)]
    pub codex_config: Option<PathBuf>,
    /// Also install the strict PreToolUse hook into this Claude Code settings file
    /// (for example `.claude/settings.json`). Off unless given.
    #[arg(long)]
    pub strict_hook_settings: Option<PathBuf>,
}

fn component(id: Option<i64>) -> Option<ComponentId> {
    id.map(ComponentId)
}

impl FollowArgs {
    fn explore(&self, relation: Relation) -> Request {
        Request::Explore(query::Explore {
            target: self.target.clone(),
            relation,
            depth: self.depth,
            component: component(self.component),
            page: self.page.page(),
        })
    }
}

impl QueryCommand {
    pub fn request(&self) -> Request {
        match self {
            QueryCommand::Coverage(page) => {
                Request::Coverage(query::Coverage { page: page.page() })
            }
            QueryCommand::Overview(args) => Request::Overview(query::Overview {
                component: component(args.component),
                page: args.page.page(),
            }),
            QueryCommand::Find(args) => Request::Find(query::Find {
                text: args.text.clone(),
                page: args.page.page(),
            }),
            QueryCommand::Explore(args) => args.follow.explore(args.relation()),
            QueryCommand::Callers(args) => args.explore(Relation::Callers),
            QueryCommand::Callees(args) => args.explore(Relation::Callees),
            QueryCommand::Path(args) => Request::Path(query::PathRequest {
                from: args.from.clone(),
                to: args.to.clone(),
                max_depth: args.depth,
                budget: args.budget.budget,
            }),
            QueryCommand::Impact(args) => Request::Impact(query::Impact {
                target: args.target.clone(),
                depth: args.depth,
                page: args.page.page(),
            }),
        }
    }
}

impl ExploreArgs {
    fn relation(&self) -> Relation {
        match (self.callers, self.callees, self.contains) {
            (true, _, _) => Relation::Callers,
            (_, true, _) => Relation::Callees,
            (_, _, true) => Relation::Contains,
            _ => Relation::All,
        }
    }
}

impl DescribeArgs {
    pub fn levels(&self) -> u32 {
        if self.all { u32::MAX } else { self.depth }
    }
}
