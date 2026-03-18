use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;
use tracing::{error, warn};

use super::AppState;
use super::models::{
    IssueDetail, ListOpts, LockStatus, NextTask, NocBadge, NocIssueState, NocProjectStatus,
    NocTaskCounts, OrchestratorStatus, ProjectStatus, RecentLogEntry, StatusCounts,
};
use crate::config;
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
    orchestrator: OrchestratorStatus,
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
    noc_state: Option<NocIssueState>,
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
    let lock_dir = state.lock_dir.clone();
    let log_dir = state.log_dir.clone();
    let rotation_state_file = state.rotation_state_file.clone();
    let project_paths: Vec<(String, String)> = state
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.path.clone()))
        .collect();

    let mut handles = Vec::new();

    for entry in &state.projects {
        let td_binary = state.td_binary.clone();
        let path = entry.path.clone();
        let name = entry.name.clone();
        let lock_dir = lock_dir.clone();
        let max_reviews = entry.max_reviews;
        handles.push(tokio::task::spawn_blocking(move || {
            fetch_project_status(&td_binary, &name, &path, &lock_dir, max_reviews)
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

    let td_binary_for_orch = state.td_binary.clone();
    let orchestrator = fetch_orchestrator_status(
        &rotation_state_file,
        &log_dir,
        &project_paths,
        &td_binary_for_orch,
    );

    let tmpl = DashboardTemplate {
        title: "Dashboard".to_string(),
        breadcrumbs: vec![],
        projects,
        orchestrator,
    };

    into_html_response(tmpl)
}

fn fetch_project_status(
    td_binary: &str,
    name: &str,
    path: &str,
    lock_dir: &str,
    _max_reviews: u32,
) -> ProjectStatus {
    let result = std::process::Command::new(td_binary)
        .args(["-w", path, "list", "--json", "--all"])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            let json = String::from_utf8_lossy(&output.stdout);
            let tasks: Vec<Task> = match serde_json::from_str(&json) {
                Ok(t) => t,
                Err(e) => {
                    warn!("failed to parse td output for {name}: {e}");
                    Vec::new()
                }
            };

            let mut counts = StatusCounts::default();
            let mut noc_counts = NocTaskCounts::default();

            for task in &tasks {
                match task.status.as_str() {
                    "open" => counts.open += 1,
                    "in_progress" => counts.in_progress += 1,
                    "blocked" => counts.blocked += 1,
                    "in_review" => counts.in_review += 1,
                    _ => {}
                }

                let has_noc_reviews = task.labels.iter().any(|l| l.starts_with("noc-reviews:"));
                let has_proposal = task.labels.iter().any(|l| l.starts_with("noc-proposal:"));
                let has_proposal_ready = task.labels.iter().any(|l| l == "noc-proposal-ready");

                if has_proposal_ready {
                    noc_counts.proposal_ready += 1;
                } else if has_proposal {
                    noc_counts.proposal_pending += 1;
                } else if has_noc_reviews {
                    match task.status.as_str() {
                        "in_progress" => noc_counts.implementing += 1,
                        "in_review" => noc_counts.reviewing += 1,
                        _ => {}
                    }
                }
            }

            let total = counts.open + counts.in_progress + counts.blocked + counts.in_review;

            let worktree_task_ids = collect_worktree_task_ids(path);
            let slug = config::project_slug(path);
            let lock_status = check_lock_status(lock_dir, &slug);

            let lock_status_label = lock_status.label().to_string();
            let lock_status_css = lock_status.css_class().to_string();
            let worktree_count = worktree_task_ids.len();

            let noc = Some(NocProjectStatus {
                worktree_task_ids,
                worktree_count,
                lock_status,
                lock_status_label,
                lock_status_css,
                counts: noc_counts,
            });

            ProjectStatus {
                name: name.to_string(),
                path: path.to_string(),
                counts,
                total,
                error: None,
                noc,
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
                noc: None,
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
                noc: None,
            }
        }
    }
}

fn collect_worktree_task_ids(project_path: &str) -> Vec<String> {
    let output = match std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_path)
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut task_ids = Vec::new();

    for line in stdout.lines() {
        if let Some(branch_ref) = line.strip_prefix("branch refs/heads/nocturnal/") {
            task_ids.push(branch_ref.to_string());
        }
    }

    task_ids
}

fn check_lock_status(lock_dir: &str, slug: &str) -> LockStatus {
    let base = std::path::PathBuf::from(lock_dir);
    // Check both develop (run-{slug}) and proposal ({proposal-{slug}) locks.
    let lock_names = [
        format!("nocturnal.run-{slug}.lock"),
        format!("nocturnal.proposal-{slug}.lock"),
    ];
    let lock_path = match lock_names.iter().map(|n| base.join(n)).find(|p| p.is_dir()) {
        Some(p) => p,
        None => return LockStatus::Idle,
    };

    let pid_file = lock_path.join("pid");
    let pid_str = match std::fs::read_to_string(&pid_file) {
        Ok(s) => s,
        Err(_) => return LockStatus::Stale,
    };

    let pid: u32 = match pid_str.trim().parse() {
        Ok(p) => p,
        Err(_) => return LockStatus::Stale,
    };

    if is_process_alive(pid) {
        LockStatus::Running(pid)
    } else {
        LockStatus::Stale
    }
}

fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn fetch_orchestrator_status(
    rotation_state_file: &str,
    log_dir: &str,
    projects: &[(String, String)],
    td_binary: &str,
) -> OrchestratorStatus {
    let (current_project, next_project) = read_rotation_state(rotation_state_file, projects);

    let next_task = next_project.as_ref().and_then(|name| {
        let path = projects
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.as_str())?;
        fetch_next_task(td_binary, path)
    });

    let recent_logs = crate::activity::read_recent(log_dir, 5)
        .into_iter()
        .map(|e| RecentLogEntry {
            command: e.command,
            project: e.project,
            task_id: e.task_id,
            started: format_iso_datetime(&e.started_at),
            duration: format_duration_secs(e.duration_secs),
        })
        .collect();

    OrchestratorStatus {
        current_project,
        next_project,
        next_task,
        recent_logs,
    }
}

fn fetch_next_task(td_binary: &str, project_path: &str) -> Option<NextTask> {
    let td = crate::td::Td::new(project_path);
    let vcs_mode = crate::project_config::load_vcs_mode(project_path);
    let check_proposals = crate::vcs::detect_platform(project_path, vcs_mode).is_some();
    let action = td.get_next_action(check_proposals).ok()?;

    let task_id = action.task_id()?;
    let action_label = action.label().to_string();
    let detail = run_td_show(td_binary, project_path, task_id).ok()?;
    Some(NextTask {
        action: action_label,
        id: detail.id,
        title: detail.title,
        priority: detail.priority,
    })
}

fn read_rotation_state(
    rotation_state_file: &str,
    projects: &[(String, String)],
) -> (Option<String>, Option<String>) {
    if projects.is_empty() {
        return (None, None);
    }

    let last_idx: Option<usize> = std::fs::read_to_string(rotation_state_file)
        .ok()
        .and_then(|s| s.trim().parse().ok());

    match last_idx {
        Some(idx) if idx < projects.len() => {
            let current = projects[idx].0.clone();
            let next_idx = (idx + 1) % projects.len();
            let next = projects[next_idx].0.clone();
            (Some(current), Some(next))
        }
        _ => (None, Some(projects[0].0.clone())),
    }
}

fn format_iso_datetime(s: &str) -> String {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .map(|dt| dt.format("%b %-d, %Y %H:%M").to_string())
        .unwrap_or_else(|_| s.to_string())
}

fn format_duration_secs(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn derive_noc_state(
    labels: &[String],
    status: &str,
    max_reviews: u32,
    project_path: &str,
    issue_id: &str,
) -> Option<NocIssueState> {
    let review_count = labels.iter().find_map(|l| {
        l.strip_prefix("noc-reviews:")
            .and_then(|n| n.parse::<u32>().ok())
    });

    let badge;
    let review_cycle;

    if let Some(n) = review_count {
        review_cycle = Some(n);
        if n >= max_reviews {
            badge = NocBadge {
                text: "blocked (max reviews)".to_string(),
                css_class: "blocked".to_string(),
            };
        } else {
            match status {
                "in_progress" => {
                    badge = NocBadge {
                        text: "implementing".to_string(),
                        css_class: "implementing".to_string(),
                    };
                }
                "in_review" => {
                    badge = NocBadge {
                        text: "noc reviewing".to_string(),
                        css_class: "reviewing".to_string(),
                    };
                }
                _ => return None,
            }
        }
    } else if labels.iter().any(|l| l == "noc-proposal-ready") {
        review_cycle = None;
        badge = NocBadge {
            text: "proposal ready".to_string(),
            css_class: "proposal-ready".to_string(),
        };
    } else if labels.iter().any(|l| l.starts_with("noc-proposal:")) {
        review_cycle = None;
        badge = NocBadge {
            text: "proposal pending".to_string(),
            css_class: "proposal-pending".to_string(),
        };
    } else {
        return None;
    }

    // Find worktree for this task
    let (worktree_path, worktree_branch) = find_worktree_for_task(project_path, issue_id);

    Some(NocIssueState {
        badge,
        review_cycle,
        max_reviews,
        worktree_path,
        worktree_branch,
    })
}

fn find_worktree_for_task(project_path: &str, task_id: &str) -> (Option<String>, Option<String>) {
    let output = match std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_path)
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return (None, None),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let target_ref = format!("refs/heads/nocturnal/{task_id}");

    let mut current_path: Option<String> = None;
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            if branch_ref == target_ref {
                let branch = format!("nocturnal/{task_id}");
                return (current_path, Some(branch));
            }
        }
    }

    (None, None)
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
    let max_reviews = entry.max_reviews;

    let result = tokio::task::spawn_blocking(move || {
        let mut detail = run_td_show(&td_binary, &path, &issue_id)?;
        detail.depends_on = run_td_depends_on(&td_binary, &path, &issue_id);
        detail.blocked_by = run_td_blocked_by(&td_binary, &path, &issue_id);
        let noc_state = derive_noc_state(
            &detail.labels,
            &detail.status,
            max_reviews,
            &path,
            &issue_id,
        );
        Ok::<_, anyhow::Error>((detail, noc_state))
    })
    .await;

    match result {
        Ok(Ok((detail, noc_state))) => {
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
                noc_state,
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

fn run_td_depends_on(td_binary: &str, project_path: &str, issue_id: &str) -> Vec<String> {
    let output = std::process::Command::new(td_binary)
        .args(["depends-on", issue_id, "--json", "-w", project_path])
        .output();
    let Ok(output) = output else { return vec![] };
    if !output.status.success() {
        return vec![];
    }
    let json = String::from_utf8_lossy(&output.stdout);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
        return vec![];
    };
    value["dependencies"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn run_td_blocked_by(td_binary: &str, project_path: &str, issue_id: &str) -> Vec<String> {
    let output = std::process::Command::new(td_binary)
        .args(["blocked-by", issue_id, "--json", "-w", project_path])
        .output();
    let Ok(output) = output else { return vec![] };
    if !output.status.success() {
        return vec![];
    }
    let json = String::from_utf8_lossy(&output.stdout);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
        return vec![];
    };
    value["direct"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
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

// --- API handlers ---

pub async fn rotate_now(State(state): State<Arc<AppState>>) -> Response {
    let is_running = state.projects.iter().any(|p| {
        let slug = crate::config::project_slug(&p.path);
        matches!(
            check_lock_status(&state.lock_dir, &slug),
            LockStatus::Running(_)
        )
    });

    if is_running {
        return Html(
            r#"<span class="rotate-feedback rotate-feedback-running">Already running</span><script>setTimeout(function(){var f=document.getElementById('rotate-feedback');if(f)f.innerHTML='';},4000);</script>"#,
        )
        .into_response();
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            error!("failed to get current exe: {e}");
            return Html(
                r#"<span class="rotate-feedback rotate-feedback-error">Failed to start</span>"#,
            )
            .into_response();
        }
    };

    match std::process::Command::new(&exe)
        .arg("develop-rotate")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_child) => Html(
            r#"<span class="rotate-feedback rotate-feedback-ok">Rotation triggered</span><script>setTimeout(function(){var f=document.getElementById('rotate-feedback');if(f)f.innerHTML='';},4000);</script>"#,
        )
        .into_response(),
        Err(e) => {
            error!("failed to spawn rotate: {e}");
            Html(
                r#"<span class="rotate-feedback rotate-feedback-error">Failed to start</span>"#,
            )
            .into_response()
        }
    }
}

pub async fn develop_now(State(state): State<Arc<AppState>>, Path(name): Path<String>) -> Response {
    let entry = match state.find_project(&name) {
        Some(e) => e,
        None => return (StatusCode::NOT_FOUND, "project not found").into_response(),
    };

    let slug = crate::config::project_slug(&entry.path);
    let lock_status = check_lock_status(&state.lock_dir, &slug);
    let project_path = entry.path.clone();

    if matches!(lock_status, LockStatus::Running(_)) {
        return Html(
            r#"<span class="rotate-feedback rotate-feedback-running">Already running</span><script>setTimeout(function(){var f=document.getElementById('develop-feedback');if(f)f.innerHTML='';},4000);</script>"#,
        )
        .into_response();
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            error!("failed to get current exe: {e}");
            return Html(
                r#"<span class="rotate-feedback rotate-feedback-error">Failed to start</span>"#,
            )
            .into_response();
        }
    };

    match std::process::Command::new(&exe)
        .args(["develop", "--project", &project_path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_child) => Html(
            r#"<span class="rotate-feedback rotate-feedback-ok">Develop triggered</span><script>setTimeout(function(){var f=document.getElementById('develop-feedback');if(f)f.innerHTML='';},4000);</script>"#,
        )
        .into_response(),
        Err(e) => {
            error!("failed to spawn run for {name}: {e}");
            Html(
                r#"<span class="rotate-feedback rotate-feedback-error">Failed to start</span>"#,
            )
            .into_response()
        }
    }
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
