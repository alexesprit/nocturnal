use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;
use tracing::{error, warn};

use super::AppState;
use super::helpers::{
    ALLOWED_PRIORITIES, ALLOWED_SORTS, ALLOWED_STATUSES, ALLOWED_TYPES, ALLOWED_VIEWS,
    FEEDBACK_HTML_FAILED_TO_START, MAX_QUERY_LEN, build_project_template, check_lock_status,
    derive_noc_state, feedback_html, fetch_orchestrator_status, fetch_project_status,
    group_by_status, is_valid_issue_id, priority_select_html, sanitize_param,
};
use super::models::{ListOpts, LockStatus, OrchestratorStatus};
use super::templates::{
    Breadcrumb, DashboardTemplate, IssueTableErrorTemplate, IssueTemplate, KanbanBoardTemplate,
    ProjectErrorTemplate, TableWrapperTemplate, into_html_response,
};
use crate::td::Td;

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

    into_html_response(DashboardTemplate {
        title: "Dashboard".to_string(),
        breadcrumbs: vec![],
        projects,
        orchestrator,
    })
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
            into_html_response(build_project_template(name, issues, recently_closed, view))
        }
        Ok(Err(e)) => {
            warn!("td list failed for {project_name}: {e}");
            into_html_response(ProjectErrorTemplate {
                title: project_name.clone(),
                breadcrumbs: vec![Breadcrumb {
                    label: project_name.clone(),
                    url: None,
                }],
                error_msg: e.to_string(),
            })
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
                        into_html_response(KanbanBoardTemplate {
                            name,
                            open,
                            in_progress,
                            blocked,
                            in_review,
                        })
                    } else {
                        into_html_response(TableWrapperTemplate { name, issues })
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

                    into_html_response(build_project_template(name, issues, recently_closed, view))
                }
            }
            Ok(Err(e)) => {
                warn!("td list failed for {project_name}: {e}");
                if is_htmx {
                    into_html_response(IssueTableErrorTemplate {
                        error_msg: e.to_string(),
                    })
                } else {
                    into_html_response(ProjectErrorTemplate {
                        title: project_name.clone(),
                        breadcrumbs: vec![Breadcrumb {
                            label: project_name.clone(),
                            url: None,
                        }],
                        error_msg: e.to_string(),
                    })
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
        let files = td.files(&issue_id);
        let noc_state = derive_noc_state(
            &detail.labels,
            &detail.status,
            max_reviews,
            &path,
            &issue_id,
        );
        let project_path = path.to_string_lossy().into_owned();
        Ok::<_, anyhow::Error>((detail, noc_state, files, project_path))
    })
    .await;

    match result {
        Ok(Ok((detail, noc_state, files, project_path))) => into_html_response(IssueTemplate {
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
            files,
            project_path,
        }),
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
        return Html(feedback_html("Already running", "running")).into_response();
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            error!("failed to get current exe: {e}");
            return Html(FEEDBACK_HTML_FAILED_TO_START).into_response();
        }
    };

    match std::process::Command::new(&exe)
        .args(["develop", "--all"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_child) => Html(feedback_html("Rotation triggered", "ok")).into_response(),
        Err(e) => {
            error!("failed to spawn rotate: {e}");
            Html(FEEDBACK_HTML_FAILED_TO_START).into_response()
        }
    }
}

async fn spawn_develop(
    state: &Arc<AppState>,
    project_name: &str,
    task_id: Option<&str>,
) -> Response {
    let Some(entry) = state.find_project(project_name) else {
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
        return Html(feedback_html("Already running", "running")).into_response();
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            error!("failed to get current exe: {e}");
            return Html(FEEDBACK_HTML_FAILED_TO_START).into_response();
        }
    };

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["develop", "--project"])
        .arg(&project_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    if let Some(id) = task_id {
        cmd.args(["--task", id]);
    }

    match cmd.spawn() {
        Ok(_child) => Html(feedback_html("Develop triggered", "ok")).into_response(),
        Err(e) => {
            if let Some(id) = task_id {
                error!("failed to spawn run for {project_name}/{id}: {e}");
            } else {
                error!("failed to spawn run for {project_name}: {e}");
            }
            Html(FEEDBACK_HTML_FAILED_TO_START).into_response()
        }
    }
}

pub async fn develop_now(State(state): State<Arc<AppState>>, Path(name): Path<String>) -> Response {
    spawn_develop(&state, &name, None).await
}

pub async fn develop_task_now(
    State(state): State<Arc<AppState>>,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    if !is_valid_issue_id(&id) {
        return (StatusCode::BAD_REQUEST, "invalid issue id").into_response();
    }
    spawn_develop(&state, &name, Some(&id)).await
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

    let status_result = tokio::task::spawn_blocking(move || {
        let td = Td::new(&path);
        td.show(&issue_id).map(|t| t.status)
    })
    .await;

    match status_result {
        Ok(Ok(ref status)) if *status == crate::td::TaskStatus::Closed => {
            return (
                StatusCode::FORBIDDEN,
                "cannot update priority of a closed task",
            )
                .into_response();
        }
        Ok(Err(e)) => {
            error!("show failed for {name}/{id}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "failed to fetch task").into_response();
        }
        Err(e) => {
            error!("task join error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response();
        }
        Ok(Ok(_)) => {}
    }

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
