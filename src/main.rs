mod activity;
mod claude;
mod commands;
mod config;
mod git;
mod lock;
mod project_config;
mod prompt;
mod td;
mod usage;
mod util;
mod vcs;
mod web;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::error;

#[derive(Parser)]
#[command(
    name = "nocturnal",
    about = "Automated task orchestrator for Claude Code + td"
)]
struct Cli {
    /// Override project root (default: current directory)
    #[arg(long = "project", global = true)]
    project: Option<String>,

    /// Show what would happen without invoking Claude or mutating task state
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Bootstrap a project directory for use with nocturnal
    Init,
    /// Implement and review the next open task [default]
    Develop,
    /// Pick and implement the highest-priority open task
    Implement,
    /// Pick and review the next reviewable task
    Review,
    /// Check open proposals for review comments and address them
    Proposal,
    /// Cycle through projects and run proposal for the first project with open proposals
    ProposalRotate,
    /// Process one project per tick, cycling through the project list (implement+review)
    DevelopRotate,
    /// Run 'develop' for every project in the project list (same tick)
    Foreach,
    /// Remove worktrees for completed/blocked tasks and clean stale locks
    Gc,
    /// Start a read-only web dashboard for td-managed projects
    Web {
        /// Server listen port
        #[arg(long, default_value = "8090")]
        port: u16,
        /// Bind address
        #[arg(long, default_value = "localhost")]
        addr: String,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        error!("{e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let mut cfg = config::Config::from_env();
    cfg.dry_run = cli.dry_run;
    let command = cli.command.unwrap_or(Command::Develop);

    let project_root = match cli.project {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::current_dir()?,
    };

    match command {
        Command::Init => commands::init::run(&project_root, cfg.dry_run),
        Command::DevelopRotate => commands::rotate::run(&cfg),
        Command::ProposalRotate => commands::proposal_review_rotate::run(&cfg),
        Command::Foreach => commands::foreach::run(&cfg),
        Command::Web { port, addr } => commands::web::run(&cfg, &addr, port),
        _ => {
            config::check_td_init(&project_root)?;

            let ctx = config::ProjectContext::new(cfg, project_root);
            match command {
                Command::Develop => commands::run::run(&ctx),
                Command::Implement => commands::implement::run(&ctx),
                Command::Review => commands::review::run(&ctx),
                Command::Proposal => commands::proposal_review::run(&ctx),
                Command::Gc => commands::gc::run(&ctx),
                Command::Init
                | Command::DevelopRotate
                | Command::ProposalRotate
                | Command::Foreach
                | Command::Web { .. } => unreachable!(),
            }
        }
    }
}
