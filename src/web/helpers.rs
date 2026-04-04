use std::fmt::Write as _;
use std::path::{Path as FsPath, PathBuf};

use tracing::warn;

use super::models::{
    ListOpts, LockStatus, NextTask, NocBadge, NocIssueState, NocProjectStatus, NocTaskCounts,
    OrchestratorStatus, ProjectStatus, RecentLogEntry, StatusCounts,
};
use crate::lock::is_process_alive;
use crate::td::{Task, Td};

// --- Validation allowlists ---

pub(super) const ALLOWED_STATUSES: &[&str] = &[
    "all",
    "open",
    "closed",
    "in_progress",
    "blocked",
    "in_review",
];
pub(super) const ALLOWED_PRIORITIES: &[&str] = &["all", "P0", "P1", "P2", "P3", "P4"];
pub(super) const ALLOWED_TYPES: &[&str] = &["all", "bug", "feature", "task", "epic", "chore"];
pub(super) const ALLOWED_SORTS: &[&str] = &[
    "priority", "created", "modified", "status", "title", "updated",
];
pub(super) const ALLOWED_VIEWS: &[&str] = &["table", "kanban"];
pub(super) const MAX_QUERY_LEN: usize = 200;

// --- Inline feedback HTML ---

pub(super) const FEEDBACK_HTML_ROTATE_RUNNING: &str = r#"<span class="action-feedback action-feedback-running">Already running</span><script>setTimeout(function(){var f=document.getElementById('action-feedback');if(f)f.innerHTML='';},4000);</script>"#;

pub(super) const FEEDBACK_HTML_ROTATE_TRIGGERED: &str = r#"<span class="action-feedback action-feedback-ok">Rotation triggered</span><script>setTimeout(function(){var f=document.getElementById('action-feedback');if(f)f.innerHTML='';},4000);</script>"#;

pub(super) const FEEDBACK_HTML_DEVELOP_RUNNING: &str = r#"<span class="action-feedback action-feedback-running">Already running</span><script>setTimeout(function(){var f=document.getElementById('action-feedback');if(f)f.innerHTML='';},4000);</script>"#;

pub(super) const FEEDBACK_HTML_DEVELOP_TRIGGERED: &str = r#"<span class="action-feedback action-feedback-ok">Develop triggered</span><script>setTimeout(function(){var f=document.getElementById('action-feedback');if(f)f.innerHTML='';},4000);</script>"#;

pub(super) const FEEDBACK_HTML_FAILED_TO_START: &str =
    r#"<span class="action-feedback action-feedback-error">Failed to start</span>"#;

// --- Validation helpers ---

pub(super) fn sanitize_param(value: &str, allowed: &[&str]) -> Option<String> {
    if allowed.contains(&value) {
        Some(value.to_string())
    } else {
        None
    }
}

pub(super) fn is_valid_issue_id(id: &str) -> bool {
    crate::td::validate_task_id(id).is_ok()
}

// --- Project status helpers ---

pub(super) fn fetch_project_status(
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
    let slug = crate::config::project_slug(path);
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

pub(super) fn collect_worktree_task_ids(project_path: &FsPath) -> Vec<String> {
    crate::git::list_nocturnal_worktrees(project_path)
        .unwrap_or_default()
        .into_iter()
        .map(|(_, task_id)| task_id)
        .collect()
}

pub(super) fn check_lock_status(lock_dir: &FsPath, slug: &str) -> LockStatus {
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

// --- Orchestrator status helpers ---

pub(super) fn fetch_orchestrator_status(
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

pub(super) fn fetch_next_task(project_path: &FsPath) -> Option<NextTask> {
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

pub(super) fn read_rotation_state(
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

// --- Formatting helpers ---

pub(super) fn format_iso_datetime(s: &str) -> String {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").map_or_else(
        |_| s.to_string(),
        |dt| dt.format("%b %-d, %Y %H:%M").to_string(),
    )
}

pub(super) fn format_duration_secs(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

// --- URL helpers ---

pub(super) fn build_proposal_url(
    project_path: &FsPath,
    proposal_id: &str,
) -> Option<(String, String)> {
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

pub(super) fn parse_remote_to_https_base(remote: &str) -> Option<String> {
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

// --- NocState helpers ---

pub(super) fn derive_noc_state(
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

pub(super) fn find_worktree_for_task(
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

// --- Task grouping helpers ---

pub(super) fn group_by_status(issues: Vec<Task>) -> (Vec<Task>, Vec<Task>, Vec<Task>, Vec<Task>) {
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

// --- HTML generation helpers ---

pub(super) fn priority_select_html(
    project_name: &str,
    issue_id: &str,
    current_priority: &str,
) -> String {
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
