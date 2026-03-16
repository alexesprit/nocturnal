use serde::Deserialize;

use crate::td::{Task, null_as_default};

#[derive(Debug, Deserialize)]
pub struct IssueDetail {
    pub id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub title: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub status: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub priority: String,
    #[serde(rename = "type", default, deserialize_with = "null_as_default")]
    pub issue_type: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub labels: Vec<String>,
    #[serde(default)]
    pub points: Option<i32>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub sprint: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub acceptance: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub created_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub updated_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub closed_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub defer_date: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub due_date: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub parent_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub children: Vec<Task>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub depends_on: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub blocked_by: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub comments: Vec<Comment>,
    #[serde(rename = "logs", default, deserialize_with = "null_as_default")]
    pub activity: Vec<ActivityEntry>,
}

#[derive(Debug, Deserialize)]
pub struct Comment {
    #[serde(default, deserialize_with = "null_as_default")]
    pub author: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub body: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub created: String,
}

#[derive(Debug, Deserialize)]
pub struct ActivityEntry {
    #[serde(rename = "type", default, deserialize_with = "null_as_default")]
    pub action: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub timestamp: String,
    #[serde(rename = "message", default, deserialize_with = "null_as_default")]
    pub details: String,
}

pub struct ProjectStatus {
    pub name: String,
    pub path: String,
    pub counts: StatusCounts,
    pub total: i32,
    pub error: Option<String>,
    pub noc: Option<NocProjectStatus>,
}

#[derive(Default)]
pub struct StatusCounts {
    pub open: i32,
    pub in_progress: i32,
    pub blocked: i32,
    pub in_review: i32,
}

#[allow(dead_code)] // lock_status kept for programmatic access
pub struct NocProjectStatus {
    pub worktree_task_ids: Vec<String>,
    pub worktree_count: usize,
    pub lock_status: LockStatus,
    pub lock_status_label: String,
    pub lock_status_css: String,
    pub counts: NocTaskCounts,
}

#[allow(dead_code)] // Running(pid) kept for programmatic access
pub enum LockStatus {
    Idle,
    Running(u32),
    Stale,
}

impl LockStatus {
    pub fn label(&self) -> &str {
        match self {
            LockStatus::Idle => "Idle",
            LockStatus::Running(_) => "Running",
            LockStatus::Stale => "Stale",
        }
    }

    pub fn css_class(&self) -> &str {
        match self {
            LockStatus::Idle => "idle",
            LockStatus::Running(_) => "running",
            LockStatus::Stale => "stale",
        }
    }
}

#[derive(Default)]
pub struct NocTaskCounts {
    pub implementing: i32,
    pub reviewing: i32,
    pub proposal_pending: i32,
    pub proposal_ready: i32,
}

pub struct NocBadge {
    pub text: String,
    pub css_class: String,
}

pub struct NocIssueState {
    pub badge: NocBadge,
    pub review_cycle: Option<u32>,
    pub max_reviews: u32,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
}

pub struct OrchestratorStatus {
    pub current_project: Option<String>,
    pub next_project: Option<String>,
    pub recent_logs: Vec<RecentLogEntry>,
}

pub struct RecentLogEntry {
    pub command: String,
    pub task_id: String,
    pub started: String,
    pub duration: String,
}

pub struct ListOpts {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub task_type: Option<String>,
    pub query: Option<String>,
    pub sort: Option<String>,
    pub all: bool,
}

impl Default for ListOpts {
    fn default() -> Self {
        Self {
            status: None,
            priority: None,
            task_type: None,
            query: None,
            sort: Some("priority".to_string()),
            all: false,
        }
    }
}
