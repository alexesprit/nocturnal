use std::sync::Arc;

use anyhow::Result;
use tokio::signal;
use tracing::{info, warn};

use crate::config::Config;
use crate::project_config;
use crate::web::{self, AppState, ProjectEntry};

pub fn run(cfg: &Config, addr: &str, port: u16) -> Result<()> {
    let projects: Vec<ProjectEntry> = cfg
        .projects_list()
        .into_iter()
        .map(|path| {
            let name = path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let settings = project_config::load_project_settings(&path);
            ProjectEntry {
                name,
                path,
                max_reviews: settings.max_reviews,
            }
        })
        .collect();

    if projects.is_empty() {
        anyhow::bail!("no projects configured (set NOCTURNAL_PROJECTS or projects file)");
    }

    let state = Arc::new(AppState {
        projects,
        td_binary: "td".to_string(),
        lock_dir: cfg.lock_dir.clone(),
        log_dir: cfg.log_dir.clone(),
        rotation_state_file: cfg.rotation_state_file.clone(),
    });

    if addr != "localhost" && addr != "127.0.0.1" {
        warn!("binding to non-loopback address {addr} -- dashboard will be network-accessible");
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let app = web::router(state);
        let bind_addr = format!("{addr}:{port}");
        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        info!("listening on http://{bind_addr}");

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;

        Ok(())
    })
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to listen for ctrl+c");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received, draining connections...");
}
