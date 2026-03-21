pub use crate::td::{IssueDetail, ListOpts};

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
    pub proposal_url: Option<String>,
    pub proposal_label: Option<String>,
}

pub struct OrchestratorStatus {
    pub current_project: Option<String>,
    pub next_project: Option<String>,
    pub next_task: Option<NextTask>,
    pub recent_logs: Vec<RecentLogEntry>,
}

pub struct NextTask {
    pub action: String,
    pub id: String,
    pub title: String,
    pub priority: String,
}

pub struct RecentLogEntry {
    pub command: String,
    pub project: String,
    pub task_id: String,
    pub started: String,
    pub duration: String,
}
