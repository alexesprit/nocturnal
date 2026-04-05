mod activity;
mod backend;
mod commands;
mod config;
mod git;
mod lock;
mod preflight;
mod project_config;
mod prompt;
mod td;
mod usage;
mod util;
mod vcs;
mod web;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

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
    Develop {
        /// Project directory (default: current directory)
        path: Option<String>,
        /// Run across all configured projects
        #[arg(long)]
        all: bool,
        /// Target a specific task ID instead of picking the next one automatically
        #[arg(long)]
        task: Option<String>,
    },
    /// Check open proposals for review comments and address them
    Proposal {
        /// Project directory (default: current directory)
        path: Option<String>,
        /// Run across all configured projects
        #[arg(long)]
        all: bool,
    },
    /// Run develop in a loop until nothing is left to do
    Loop {
        /// Project directory (default: current directory)
        path: Option<String>,
        /// Run across all configured projects
        #[arg(long)]
        all: bool,
        /// Maximum number of iterations (default: unlimited)
        #[arg(long, short = 'n')]
        max_iterations: Option<usize>,
    },
    /// Remove worktrees for completed/blocked tasks and clean stale locks
    Gc {
        /// Project directory (default: current directory)
        path: Option<String>,
        /// Run across all configured projects
        #[arg(long)]
        all: bool,
    },
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
    let make_filter =
        || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let tmpdir = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    let log_dir = std::path::PathBuf::from(
        std::env::var("NOCTURNAL_LOG_DIR").unwrap_or_else(|_| format!("{tmpdir}/nocturnal-logs")),
    );
    std::fs::create_dir_all(&log_dir).ok();

    let log_path = log_dir.join(format!(
        "nocturnal-{}.log",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));

    let stdout_layer = fmt::layer().with_target(false).with_filter(make_filter());

    match std::fs::File::create(&log_path) {
        Ok(log_file) => {
            let file_layer = fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(log_file)
                .with_filter(make_filter());
            tracing_subscriber::registry()
                .with(stdout_layer)
                .with(file_layer)
                .init();
            println!("Trace log: {}", log_path.display());
        }
        Err(e) => {
            tracing_subscriber::registry().with(stdout_layer).init();
            eprintln!(
                "Warning: could not create trace log at {}: {e}",
                log_path.display()
            );
        }
    }

    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        error!("{e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let mut cfg = config::Config::from_env();
    cfg.dry_run = cli.dry_run;
    let command = cli.command.unwrap_or(Command::Develop {
        path: None,
        all: false,
        task: None,
    });

    let default_root = match cli.project {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::current_dir()?,
    };

    match command {
        Command::Init => commands::init::run(&default_root, cfg.dry_run),
        Command::Web { port, addr } => commands::web::run(&cfg, &addr, port),
        Command::Develop { path, all, task } => {
            if all {
                info!(
                    "Mode: all projects ({} configured)",
                    cfg.projects_list().len()
                );
                commands::rotate::run(&cfg)
            } else {
                let root = path.map(std::path::PathBuf::from).unwrap_or(default_root);
                config::check_td_init(&root)?;
                info!("Project: {}", root.display());
                let ctx = config::ProjectContext::new(cfg, root);
                commands::run::run(&ctx, task.as_deref())
            }
        }
        Command::Proposal { path, all } => {
            if all {
                info!(
                    "Mode: all projects ({} configured)",
                    cfg.projects_list().len()
                );
                commands::proposal_review_rotate::run(&cfg)
            } else {
                let root = path.map(std::path::PathBuf::from).unwrap_or(default_root);
                config::check_td_init(&root)?;
                info!("Project: {}", root.display());
                let ctx = config::ProjectContext::new(cfg, root);
                commands::proposal_review::run(&ctx)
            }
        }
        Command::Loop {
            path,
            all,
            max_iterations,
        } => {
            if all {
                info!(
                    "Mode: all projects ({} configured)",
                    cfg.projects_list().len()
                );
                commands::loop_cmd::run(&cfg, max_iterations)
            } else {
                let root = path.map(std::path::PathBuf::from).unwrap_or(default_root);
                config::check_td_init(&root)?;
                info!("Project: {}", root.display());
                let ctx = config::ProjectContext::new(cfg, root);
                commands::loop_cmd::run_single(&ctx, max_iterations)
            }
        }
        Command::Gc { path, all } => {
            if all {
                info!(
                    "Mode: all projects ({} configured)",
                    cfg.projects_list().len()
                );
                use std::path::PathBuf;
                use tracing::warn;
                // Run lock cleanup once before the project loop
                let lock_ctx = config::ProjectContext::new(cfg.clone(), default_root.clone());
                let locks_removed = commands::gc::run_locks_only(&lock_ctx)?;
                let mut total_worktrees = 0usize;
                let mut project_count = 0usize;
                for project in cfg.projects_list() {
                    let root = PathBuf::from(&project);
                    if !root.join(".todos").is_dir() {
                        warn!("gc: skipping {project} (no .todos/ found)");
                        continue;
                    }
                    let ctx = config::ProjectContext::new(cfg.clone(), root);
                    let removed = commands::gc::run_worktrees_only(&ctx)?;
                    println!("gc [{project}]: {removed} worktree(s) removed");
                    total_worktrees += removed;
                    project_count += 1;
                }
                println!(
                    "gc: {total_worktrees} worktree(s) removed across {project_count} project(s), {locks_removed} stale lock(s) cleaned"
                );
                Ok(())
            } else {
                let root = path.map(std::path::PathBuf::from).unwrap_or(default_root);
                config::check_td_init(&root)?;
                info!("Project: {}", root.display());
                let ctx = config::ProjectContext::new(cfg, root);
                commands::gc::run(&ctx)
            }
        }
    }
}
