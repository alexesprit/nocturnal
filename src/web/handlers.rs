use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;
use tracing::{error, warn};

use super::AppState;
use super::models::{IssueDetail, ListOpts, ProjectStatus, StatusCounts};
use crate::td::Task;

// --- Validation allowlists ---

const ALLOWED_STATUSES: &[&str] = &[
    "all",
    "open",
    "closed",
    "in_progress",
    "blocked",
    "in_review",
];
const ALLOWED_PRIORITIES: &[&str] = &["all", "P0", "P1", "P2", "P3", "P4"];
const ALLOWED_TYPES: &[&str] = &["all", "bug", "feature", "task", "epic", "chore"];
const ALLOWED_SORTS: &[&str] = &[
    "priority", "created", "modified", "status", "title", "updated",
];

fn sanitize_param(value: &str, allowed: &[&str]) -> Option<String> {
    if allowed.contains(&value) {
        Some(value.to_string())
    } else {
        None
    }
}

fn is_valid_issue_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// --- Askama templates ---

struct Breadcrumb {
    label: String,
    url: Option<String>,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    title: String,
    breadcrumbs: Vec<Breadcrumb>,
    projects: Vec<ProjectStatus>,
}

#[derive(Template)]
#[template(path = "project.html")]
struct ProjectTemplate {
    title: String,
    breadcrumbs: Vec<Breadcrumb>,
    name: String,
    issues: Vec<Task>,
}

#[derive(Template)]
#[template(path = "project_error.html")]
struct ProjectErrorTemplate {
    title: String,
    breadcrumbs: Vec<Breadcrumb>,
    error_msg: String,
}

#[derive(Template)]
#[template(path = "issue.html")]
struct IssueTemplate {
    title: String,
    breadcrumbs: Vec<Breadcrumb>,
    project_name: String,
    issue: IssueDetail,
}

#[derive(Template)]
#[template(path = "partials/issue_table.html")]
struct IssueTableTemplate {
    name: String,
    issues: Vec<Task>,
}

#[derive(Template)]
#[template(path = "partials/issue_table_error.html")]
struct IssueTableErrorTemplate {
    error_msg: String,
}

mod filters {
    pub use crate::web::filters::{format_date, format_datetime};

    pub fn render_markdown(s: &str) -> askama::Result<String> {
        Ok(crate::web::markdown::render(s))
    }

    pub fn join_labels(labels: &[String], sep: &str) -> askama::Result<String> {
        Ok(labels.join(sep))
    }
}

// --- Query params ---

#[derive(Deserialize)]
pub struct IssueFilterParams {
    #[serde(default)]
    status: String,
    #[serde(default)]
    priority: String,
    #[serde(rename = "type", default)]
    issue_type: String,
    #[serde(default)]
    q: String,
    #[serde(default)]
    sort: String,
}

// --- Handlers ---

pub async fn dashboard(State(state): State<Arc<AppState>>) -> Response {
    let mut handles = Vec::new();

    for entry in &state.projects {
        let td_binary = state.td_binary.clone();
        let path = entry.path.clone();
        let name = entry.name.clone();

        handles.push(tokio::task::spawn_blocking(move || {
            fetch_project_status(&td_binary, &name, &path)
        }));
    }

    let mut projects = Vec::new();
    for handle in handles {
        match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
            Ok(Ok(status)) => projects.push(status),
            Ok(Err(e)) => {
                error!("task join error: {e}");
            }
            Err(_) => {
                error!("project status fetch timed out");
            }
        }
    }

    let tmpl = DashboardTemplate {
        title: "Dashboard".to_string(),
        breadcrumbs: vec![],
        projects,
    };

    into_html_response(tmpl)
}

fn fetch_project_status(td_binary: &str, name: &str, path: &str) -> ProjectStatus {
    let result = std::process::Command::new(td_binary)
        .args(["-w", path, "list", "--json", "--all"])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            let json = String::from_utf8_lossy(&output.stdout);
            let tasks: Vec<Task> = serde_json::from_str(&json).unwrap_or_default();

            let mut counts = StatusCounts::default();
            for task in &tasks {
                match task.status.as_str() {
                    "open" => counts.open += 1,
                    "in_progress" => counts.in_progress += 1,
                    "blocked" => counts.blocked += 1,
                    "in_review" => counts.in_review += 1,
                    _ => {}
                }
            }
            let total = counts.open + counts.in_progress + counts.blocked + counts.in_review;

            ProjectStatus {
                name: name.to_string(),
                path: path.to_string(),
                counts,
                total,
                error: None,
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("td list failed for {name}: {stderr}");
            ProjectStatus {
                name: name.to_string(),
                path: path.to_string(),
                counts: StatusCounts::default(),
                total: 0,
                error: Some(stderr.trim().to_string()),
            }
        }
        Err(e) => {
            error!("failed to run td for {name}: {e}");
            ProjectStatus {
                name: name.to_string(),
                path: path.to_string(),
                counts: StatusCounts::default(),
                total: 0,
                error: Some(format!("failed to run td: {e}")),
            }
        }
    }
}

pub async fn project(State(state): State<Arc<AppState>>, Path(name): Path<String>) -> Response {
    let entry = match state.find_project(&name) {
        Some(e) => e,
        None => return (StatusCode::NOT_FOUND, "project not found").into_response(),
    };

    let td_binary = state.td_binary.clone();
    let path = entry.path.clone();
    let project_name = name.clone();

    let result = tokio::task::spawn_blocking(move || {
        run_td_list(
            &td_binary,
            &path,
            &ListOpts {
                sort: Some("priority".to_string()),
                ..Default::default()
            },
        )
    })
    .await;

    match result {
        Ok(Ok(issues)) => {
            let tmpl = ProjectTemplate {
                title: name.clone(),
                breadcrumbs: vec![Breadcrumb {
                    label: name.clone(),
                    url: None,
                }],
                name,
                issues,
            };
            into_html_response(tmpl)
        }
        Ok(Err(e)) => {
            warn!("td list failed for {project_name}: {e}");
            let tmpl = ProjectErrorTemplate {
                title: project_name.clone(),
                breadcrumbs: vec![Breadcrumb {
                    label: project_name.clone(),
                    url: None,
                }],
                error_msg: e.to_string(),
            };
            into_html_response(tmpl)
        }
        Err(e) => {
            error!("task join error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
        }
    }
}

pub async fn project_issues(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Query(params): Query<IssueFilterParams>,
) -> Response {
    let entry = match state.find_project(&name) {
        Some(e) => e,
        None => return (StatusCode::NOT_FOUND, "project not found").into_response(),
    };

    let is_htmx = headers.get("HX-Request").is_some();

    const MAX_QUERY_LEN: usize = 200;
    let query = if params.q.is_empty()
        || params.q.starts_with('-')
        || params.q.len() > MAX_QUERY_LEN
        || !params.q.chars().all(|c| c.is_ascii_graphic() || c == ' ')
    {
        None
    } else {
        Some(params.q)
    };

    let opts = ListOpts {
        status: sanitize_param(&params.status, ALLOWED_STATUSES),
        priority: sanitize_param(&params.priority, ALLOWED_PRIORITIES),
        task_type: sanitize_param(&params.issue_type, ALLOWED_TYPES),
        query,
        sort: sanitize_param(&params.sort, ALLOWED_SORTS).or(Some("priority".to_string())),
        all: false,
    };

    let td_binary = state.td_binary.clone();
    let path = entry.path.clone();
    let project_name = name.clone();

    let result = tokio::task::spawn_blocking(move || run_td_list(&td_binary, &path, &opts)).await;

    match result {
        Ok(Ok(issues)) => {
            if is_htmx {
                let tmpl = IssueTableTemplate { name, issues };
                into_html_response(tmpl)
            } else {
                let tmpl = ProjectTemplate {
                    title: name.clone(),
                    breadcrumbs: vec![Breadcrumb {
                        label: name.clone(),
                        url: None,
                    }],
                    name,
                    issues,
                };
                into_html_response(tmpl)
            }
        }
        Ok(Err(e)) => {
            warn!("td list failed for {project_name}: {e}");
            if is_htmx {
                let tmpl = IssueTableErrorTemplate {
                    error_msg: e.to_string(),
                };
                into_html_response(tmpl)
            } else {
                let tmpl = ProjectErrorTemplate {
                    title: project_name.clone(),
                    breadcrumbs: vec![Breadcrumb {
                        label: project_name.clone(),
                        url: None,
                    }],
                    error_msg: e.to_string(),
                };
                into_html_response(tmpl)
            }
        }
        Err(e) => {
            error!("task join error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
        }
    }
}

pub async fn issue(
    State(state): State<Arc<AppState>>,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    if !is_valid_issue_id(&id) {
        return (StatusCode::BAD_REQUEST, "invalid issue id").into_response();
    }

    let entry = match state.find_project(&name) {
        Some(e) => e,
        None => return (StatusCode::NOT_FOUND, "project not found").into_response(),
    };

    let td_binary = state.td_binary.clone();
    let path = entry.path.clone();
    let project_name = name.clone();
    let issue_id = id.clone();

    let result =
        tokio::task::spawn_blocking(move || run_td_show(&td_binary, &path, &issue_id)).await;

    match result {
        Ok(Ok(detail)) => {
            let tmpl = IssueTemplate {
                title: format!("{} — {}", detail.id, detail.title),
                breadcrumbs: vec![
                    Breadcrumb {
                        label: project_name.clone(),
                        url: Some(format!("/projects/{project_name}")),
                    },
                    Breadcrumb {
                        label: id,
                        url: None,
                    },
                ],
                project_name,
                issue: detail,
            };
            into_html_response(tmpl)
        }
        Ok(Err(e)) => {
            let err_str = e.to_string();
            if err_str.to_lowercase().contains("not found") {
                (StatusCode::NOT_FOUND, "issue not found").into_response()
            } else {
                error!("td show failed for {project_name}/{id}: {err_str}");
                (StatusCode::INTERNAL_SERVER_ERROR, "failed to load issue").into_response()
            }
        }
        Err(e) => {
            error!("task join error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
        }
    }
}

// --- td CLI wrappers ---

fn run_td_list(td_binary: &str, project_path: &str, opts: &ListOpts) -> anyhow::Result<Vec<Task>> {
    let mut args = vec!["list", "--json", "-w", project_path];

    if opts.all {
        args.push("--all");
    }

    // We need to own these strings for the lifetime of the command
    let status_val;
    if let Some(ref s) = opts.status {
        if s != "all" {
            status_val = s.clone();
            args.push("--status");
            args.push(&status_val);
        }
    }

    let priority_val;
    if let Some(ref p) = opts.priority {
        if p != "all" {
            priority_val = p.clone();
            args.push("--priority");
            args.push(&priority_val);
        }
    }

    let type_val;
    if let Some(ref t) = opts.task_type {
        if t != "all" {
            type_val = t.clone();
            args.push("--type");
            args.push(&type_val);
        }
    }

    let query_val;
    if let Some(ref q) = opts.query {
        query_val = q.clone();
        args.push("-q");
        args.push(&query_val);
    }

    let sort_val;
    if let Some(ref s) = opts.sort {
        sort_val = s.clone();
        args.push("--sort");
        args.push(&sort_val);
    }

    let output = std::process::Command::new(td_binary)
        .args(&args)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run td: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("td list failed: {}", stderr.trim());
    }

    let json = String::from_utf8_lossy(&output.stdout);
    let tasks: Vec<Task> = serde_json::from_str::<Option<Vec<Task>>>(&json)?.unwrap_or_default();
    Ok(tasks)
}

fn run_td_show(td_binary: &str, project_path: &str, issue_id: &str) -> anyhow::Result<IssueDetail> {
    let output = std::process::Command::new(td_binary)
        .args(["show", issue_id, "--json", "-w", project_path])
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run td: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("td show failed: {}", stderr.trim());
    }

    let json = String::from_utf8_lossy(&output.stdout);
    let detail: IssueDetail = serde_json::from_str(&json)?;
    Ok(detail)
}

// --- Helpers ---

fn into_html_response<T: Template>(tmpl: T) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            error!("template render error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
        }
    }
}
