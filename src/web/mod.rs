pub mod filters;
pub mod handlers;
mod helpers;
pub mod markdown;
pub mod models;
mod templates;

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "src/static/"]
struct StaticAssets;

pub struct ProjectEntry {
    pub name: String,
    pub path: PathBuf,
    pub max_reviews: u32,
}

pub struct AppState {
    pub projects: Vec<ProjectEntry>,
    pub lock_dir: PathBuf,
    pub log_dir: PathBuf,
    pub rotation_state_file: String,
}

impl AppState {
    pub fn find_project(&self, name: &str) -> Option<&ProjectEntry> {
        self.projects.iter().find(|p| p.name == name)
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(handlers::dashboard))
        .route("/projects/{name}", get(handlers::project))
        .route("/projects/{name}/issues", get(handlers::project_issues))
        .route("/projects/{name}/issues/{id}", get(handlers::issue))
        .route("/api/rotate", post(handlers::rotate_now))
        .route("/api/projects/{name}/develop", post(handlers::develop_now))
        .route(
            "/api/projects/{name}/issues/{id}/priority",
            post(handlers::update_priority),
        )
        .route("/static/{*path}", get(static_handler))
        .with_state(state)
}

async fn static_handler(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    let extension = Path::new(&path).extension();
    let mime = if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("css")) {
        "text/css"
    } else if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("js")) {
        "application/javascript"
    } else {
        "application/octet-stream"
    };

    match StaticAssets::get(&path) {
        Some(content) => (
            axum::http::StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, mime)],
            content.data.to_vec(),
        )
            .into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
