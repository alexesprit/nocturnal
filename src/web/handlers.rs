use std::fmt::Write as _;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use askama::Template;
use axum::extract::{Form, Path, Query, State};
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
use crate::lock::is_process_alive;
use crate::td::{Task, Td};

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
const ALLOWED_VIEWS: &[&str] = &["table", "kanban"];
const MAX_QUERY_LEN: usize = 200;

fn sanitize_param(value: &str, allowed: &[&str]) -> Option<String> {
    if allowed.contains(&value) {
        Some(value.to_string())
    } else {
        None
    }
}

fn is_valid_issue_id(id: &str) -> bool {
    crate::td::validate_task_id(id).is_ok()
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
    view: String,
    issues: Vec<Task>,
    open: Vec<Task>,
    in_progress: Vec<Task>,
    blocked: Vec<Task>,
    in_review: Vec<Task>,
    recently_closed: Vec<Task>,
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
#[template(path = "partials/table_wrapper.html")]
struct TableWrapperTemplate {
    name: String,
    issues: Vec<Task>,
}

#[derive(Template)]
#[template(path = "partials/kanban_board.html")]
struct KanbanBoardTemplate {
    name: String,
    open: Vec<Task>,
    in_progress: Vec<Task>,
    blocked: Vec<Task>,
    in_review: Vec<Task>,
}

#[derive(Template)]
#[template(path = "partials/issue_table_error.html")]
struct IssueTableErrorTemplate {
    error_msg: String,
}

mod filters {
    pub use crate::web::filters::{format_date, format_datetime};

    #[allow(clippy::unnecessary_wraps)]
    pub fn render_markdown(s: &str, _values: &dyn askama::Values) -> askama::Result<String> {
        Ok(crate::web::markdown::render(s))
    }

    #[allow(clippy::unnecessary_wraps)]
    pub fn join_labels(
        labels: &[String],
        _values: &dyn askama::Values,
        sep: &str,
    ) -> askama::Result<String> {
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
    #[serde(default)]
    view: String,
}

// --- Handlers ---

pub async fn dashboard(State(state): State<Arc<AppState>>) -> Response {
    let lock_dir = state.lock_dir.clone();
    let log_dir = state.log_dir.clone();
    let rotation_state_file = state.rotation_state_file.clone();
    let project_paths: Vec<(String, PathBuf)> = state
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.path.clone()))
        .collect();

    let mut handles = Vec::new();

    for entry in &state.projects {
        let path = entry.path.clone();
        let name = entry.name.clone();
        let lock_dir = lock_dir.clone();
        let max_reviews = entry.max_reviews;
        handles.push(tokio::task::spawn_blocking(move || {
            fetch_project_status(&name, &path, &lock_dir, max_reviews)
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

    let orchestrator = tokio::task::spawn_blocking(move || {
        fetch_orchestrator_status(&rotation_state_file, &log_dir, &project_paths)
    })
    .await
    .unwrap_or_else(|e| {
        error!("orchestrator status join error: {e}");
        OrchestratorStatus {
            current_project: None,
            next_project: None,
            next_task: None,
            recent_logs: Vec::new(),
        }
    });

    let tmpl = DashboardTemplate {
        title: "Dashboard".to_string(),
        breadcrumbs: vec![],
        projects,
        orchestrator,
    };

    into_html_response(tmpl)
}

fn fetch_project_status(
    name: &str,
    path: &FsPath,
    lock_dir: &FsPath,
    _max_reviews: u32,
) -> ProjectStatus {
    let path_str = path.to_string_lossy();
    let td = Td::new(path);
    let tasks = match td.list(&ListOpts::default()) {
        Ok(tasks) => tasks,
        Err(e) => {
            warn!("td list failed for {name}: {e}");
            return ProjectStatus {
                name: name.to_string(),
                path: path_str.into_owned(),
                counts: StatusCounts::default(),
                total: 0,
                error: Some(e.to_string()),
                noc: None,
            };
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

        if has_proposal_ready && task.status != "closed" {
            noc_counts.proposal_ready += 1;
        } else if has_proposal && task.status != "closed" {
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
        path: path_str.into_owned(),
        counts,
        total,
        error: None,
        noc,
    }
}

fn collect_worktree_task_ids(project_path: &FsPath) -> Vec<String> {
    crate::git::list_nocturnal_worktrees(project_path)
        .unwrap_or_default()
        .into_iter()
        .map(|(_, task_id)| task_id)
        .collect()
}

fn check_lock_status(lock_dir: &FsPath, slug: &str) -> LockStatus {
    // Check both develop (run-{slug}) and proposal ({proposal-{slug}) locks.
    let lock_names = [
        format!("nocturnal.run-{slug}.lock"),
        format!("nocturnal.proposal-{slug}.lock"),
    ];
    let Some(lock_path) = lock_names
        .iter()
        .map(|n| lock_dir.join(n))
        .find(|p| p.is_dir())
    else {
        return LockStatus::Idle;
    };

    let pid_file = lock_path.join("pid");
    let Ok(pid_str) = std::fs::read_to_string(&pid_file) else {
        return LockStatus::Stale;
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

fn fetch_orchestrator_status(
    rotation_state_file: &str,
    log_dir: &FsPath,
    projects: &[(String, PathBuf)],
) -> OrchestratorStatus {
    let (current_project, next_project) = read_rotation_state(rotation_state_file, projects);

    let next_task = next_project.as_ref().and_then(|name| {
        let path = projects
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.as_path())?;
        fetch_next_task(path)
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

fn fetch_next_task(project_path: &FsPath) -> Option<NextTask> {
    let td = Td::new(project_path);
    let vcs_mode = crate::project_config::load_vcs_mode(project_path);
    let check_proposals = crate::vcs::detect_platform(project_path, vcs_mode).is_some();
    let action = td.get_next_action(check_proposals).ok()?;

    let task_id = action.task_id()?;
    let action_label = action.label().to_string();
    let detail = td.show_detail(task_id).ok()?;
    Some(NextTask {
        action: action_label,
        id: detail.id,
        title: detail.title,
        priority: detail.priority,
    })
}

fn read_rotation_state(
    rotation_state_file: &str,
    projects: &[(String, PathBuf)],
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
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").map_or_else(
        |_| s.to_string(),
        |dt| dt.format("%b %-d, %Y %H:%M").to_string(),
    )
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

fn build_proposal_url(project_path: &FsPath, proposal_id: &str) -> Option<(String, String)> {
    let remote = crate::git::remote_url(project_path)?;
    let base_url = parse_remote_to_https_base(&remote)?;

    if remote.contains("github") {
        let url = format!("{base_url}/pull/{proposal_id}");
        let label = format!("PR #{proposal_id}");
        Some((url, label))
    } else if remote.contains("gitlab") {
        let url = format!("{base_url}/-/merge_requests/{proposal_id}");
        let label = format!("MR #{proposal_id}");
        Some((url, label))
    } else {
        None
    }
}

fn parse_remote_to_https_base(remote: &str) -> Option<String> {
    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = remote.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let path = path.trim_end_matches(".git");
        return Some(format!("https://{host}/{path}"));
    }
    // HTTPS: https://github.com/owner/repo.git
    if remote.starts_with("https://") || remote.starts_with("http://") {
        let url = remote.trim_end_matches(".git");
        return Some(url.to_string());
    }
    None
}

fn derive_noc_state(
    labels: &[String],
    status: &str,
    max_reviews: u32,
    project_path: &FsPath,
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
        if status == "closed" {
            return None;
        }
        review_cycle = None;
        badge = NocBadge {
            text: "proposal ready".to_string(),
            css_class: "proposal-ready".to_string(),
        };
    } else if labels.iter().any(|l| l.starts_with("noc-proposal:")) {
        if status == "closed" {
            return None;
        }
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

    // Build proposal URL if a proposal label exists
    let (proposal_url, proposal_label) = labels
        .iter()
        .find_map(|l| {
            l.strip_prefix("noc-proposal:")
                .map(std::string::ToString::to_string)
        })
        .and_then(|id| build_proposal_url(project_path, &id))
        .map_or((None, None), |(url, label)| (Some(url), Some(label)));

    Some(NocIssueState {
        badge,
        review_cycle,
        max_reviews,
        worktree_path,
        worktree_branch,
        proposal_url,
        proposal_label,
    })
}

fn find_worktree_for_task(
    project_path: &FsPath,
    task_id: &str,
) -> (Option<String>, Option<String>) {
    match crate::git::worktree_path(project_path, task_id) {
        Ok(Some(path)) => (
            Some(path.to_string_lossy().into_owned()),
            Some(format!("nocturnal/{task_id}")),
        ),
        _ => (None, None),
    }
}

pub async fn project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(params): Query<IssueFilterParams>,
) -> Response {
    let Some(entry) = state.find_project(&name) else {
        return (StatusCode::NOT_FOUND, "project not found").into_response();
    };

    let view = sanitize_param(&params.view, ALLOWED_VIEWS).unwrap_or_else(|| "table".to_string());

    let path = entry.path.clone();
    let path2 = entry.path.clone();
    let project_name = name.clone();

    let active_handle = tokio::task::spawn_blocking(move || {
        Td::new(&path).list(&ListOpts {
            sort: Some("priority".to_string()),
            ..Default::default()
        })
    });

    let closed_handle = tokio::task::spawn_blocking(move || {
        Td::new(&path2).list(&ListOpts {
            status: Some("closed".to_string()),
            sort: Some("closed_at".to_string()),
            reverse: true,
            limit: Some(10),
            all: true,
            ..Default::default()
        })
    });

    let (active_result, closed_result) = tokio::join!(active_handle, closed_handle);

    match active_result {
        Ok(Ok(issues)) => {
            let recently_closed = closed_result.ok().and_then(Result::ok).unwrap_or_default();

            let (table_issues, open, in_progress, blocked, in_review) = if view == "kanban" {
                let (o, ip, bl, ir) = group_by_status(issues);
                (Vec::new(), o, ip, bl, ir)
            } else {
                (issues, Vec::new(), Vec::new(), Vec::new(), Vec::new())
            };
            let tmpl = ProjectTemplate {
                title: name.clone(),
                breadcrumbs: vec![Breadcrumb {
                    label: name.clone(),
                    url: None,
                }],
                name,
                view,
                issues: table_issues,
                open,
                in_progress,
                blocked,
                in_review,
                recently_closed,
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

#[allow(clippy::too_many_lines)]
pub async fn project_issues(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Query(params): Query<IssueFilterParams>,
) -> Response {
    async fn inner(
        state: Arc<AppState>,
        name: String,
        headers: HeaderMap,
        params: IssueFilterParams,
    ) -> Response {
        let Some(entry) = state.find_project(&name) else {
            return (StatusCode::NOT_FOUND, "project not found").into_response();
        };

        let is_htmx = headers.get("HX-Request").is_some();

        let view =
            sanitize_param(&params.view, ALLOWED_VIEWS).unwrap_or_else(|| "table".to_string());

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
            ..Default::default()
        };

        let path = entry.path.clone();
        let path2 = entry.path.clone();
        let project_name = name.clone();

        let result = tokio::task::spawn_blocking(move || Td::new(&path).list(&opts)).await;

        match result {
            Ok(Ok(issues)) => {
                if is_htmx {
                    if view == "kanban" {
                        let (open, in_progress, blocked, in_review) = group_by_status(issues);
                        let tmpl = KanbanBoardTemplate {
                            name,
                            open,
                            in_progress,
                            blocked,
                            in_review,
                        };
                        into_html_response(tmpl)
                    } else {
                        let tmpl = TableWrapperTemplate { name, issues };
                        into_html_response(tmpl)
                    }
                } else {
                    let recently_closed = tokio::task::spawn_blocking(move || {
                        Td::new(&path2).list(&ListOpts {
                            status: Some("closed".to_string()),
                            sort: Some("closed_at".to_string()),
                            reverse: true,
                            limit: Some(10),
                            all: true,
                            ..Default::default()
                        })
                    })
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_default();

                    let (table_issues, open, in_progress, blocked, in_review) = if view == "kanban"
                    {
                        let (o, ip, bl, ir) = group_by_status(issues);
                        (Vec::new(), o, ip, bl, ir)
                    } else {
                        (issues, Vec::new(), Vec::new(), Vec::new(), Vec::new())
                    };
                    let tmpl = ProjectTemplate {
                        title: name.clone(),
                        breadcrumbs: vec![Breadcrumb {
                            label: name.clone(),
                            url: None,
                        }],
                        name,
                        view,
                        issues: table_issues,
                        open,
                        in_progress,
                        blocked,
                        in_review,
                        recently_closed,
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

    inner(state, name, headers, params).await
}

pub async fn issue(
    State(state): State<Arc<AppState>>,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    if !is_valid_issue_id(&id) {
        return (StatusCode::BAD_REQUEST, "invalid issue id").into_response();
    }

    let Some(entry) = state.find_project(&name) else {
        return (StatusCode::NOT_FOUND, "project not found").into_response();
    };

    let path = entry.path.clone();
    let project_name = name.clone();
    let issue_id = id.clone();
    let max_reviews = entry.max_reviews;

    let result = tokio::task::spawn_blocking(move || {
        let td = Td::new(&path);
        let mut detail = td.show_detail(&issue_id)?;
        detail.depends_on = td.depends_on(&issue_id);
        detail.blocked_by = td.blocked_by(&issue_id);
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

fn group_by_status(issues: Vec<Task>) -> (Vec<Task>, Vec<Task>, Vec<Task>, Vec<Task>) {
    let mut open = Vec::new();
    let mut in_progress = Vec::new();
    let mut blocked = Vec::new();
    let mut in_review = Vec::new();
    for task in issues {
        match task.status.as_str() {
            "open" => open.push(task),
            "in_progress" => in_progress.push(task),
            "blocked" => blocked.push(task),
            "in_review" => in_review.push(task),
            _ => {}
        }
    }
    (open, in_progress, blocked, in_review)
}

// --- API handlers ---

pub async fn rotate_now(State(state): State<Arc<AppState>>) -> Response {
    let lock_dir = state.lock_dir.clone();
    let project_paths_for_lock: Vec<PathBuf> =
        state.projects.iter().map(|p| p.path.clone()).collect();
    let is_running = tokio::task::spawn_blocking(move || {
        project_paths_for_lock.iter().any(|path| {
            let slug = crate::config::project_slug(path);
            matches!(check_lock_status(&lock_dir, &slug), LockStatus::Running(_))
        })
    })
    .await
    .unwrap_or(false);

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
    let Some(entry) = state.find_project(&name) else {
        return (StatusCode::NOT_FOUND, "project not found").into_response();
    };

    let slug = crate::config::project_slug(&entry.path);
    let lock_dir_for_check = state.lock_dir.clone();
    let lock_status =
        tokio::task::spawn_blocking(move || check_lock_status(&lock_dir_for_check, &slug))
            .await
            .unwrap_or(LockStatus::Idle);
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
        .args(["develop", "--project"])
        .arg(&project_path)
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

#[derive(Deserialize)]
pub struct PriorityForm {
    priority: String,
}

pub async fn update_priority(
    State(state): State<Arc<AppState>>,
    Path((name, id)): Path<(String, String)>,
    Form(form): Form<PriorityForm>,
) -> Response {
    if !is_valid_issue_id(&id) {
        return (StatusCode::BAD_REQUEST, "invalid issue id").into_response();
    }

    let valid_priorities = &ALLOWED_PRIORITIES[1..]; // exclude "all"
    let Some(priority) = sanitize_param(&form.priority, valid_priorities) else {
        return (StatusCode::BAD_REQUEST, "invalid priority").into_response();
    };

    let Some(entry) = state.find_project(&name) else {
        return (StatusCode::NOT_FOUND, "project not found").into_response();
    };

    let path = entry.path.clone();
    let issue_id = id.clone();
    let priority_clone = priority.clone();
    let result = tokio::task::spawn_blocking(move || {
        let td = Td::new(&path);
        td.update_priority(&issue_id, &priority_clone)
    })
    .await;

    match result {
        Ok(Ok(())) => {
            let html = priority_select_html(&name, &id, &priority);
            Html(html).into_response()
        }
        Ok(Err(e)) => {
            error!("update_priority failed for {name}/{id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to update priority",
            )
                .into_response()
        }
        Err(e) => {
            error!("task join error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
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

fn priority_select_html(project_name: &str, issue_id: &str, current_priority: &str) -> String {
    let priorities = &["P0", "P1", "P2", "P3", "P4"];
    let mut options = String::new();
    for p in priorities {
        let selected = if *p == current_priority {
            " selected"
        } else {
            ""
        };
        let _ = write!(options, r#"<option value="{p}"{selected}>{p}</option>"#);
    }
    format!(
        "<span class=\"priority-select-wrapper badge badge-priority badge-priority-{current_priority}\" id=\"priority-select-{issue_id}\"><select class=\"priority-select\" hx-post=\"/api/projects/{project_name}/issues/{issue_id}/priority\" hx-trigger=\"change\" hx-target=\"#priority-select-{issue_id}\" hx-swap=\"outerHTML\" name=\"priority\">{options}</select></span>"
    )
}
