use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use tracing::error;

use super::models::{IssueDetail, NocIssueState, OrchestratorStatus, ProjectStatus};
use crate::td::Task;

pub(super) struct Breadcrumb {
    pub(super) label: String,
    pub(super) url: Option<String>,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
pub(super) struct DashboardTemplate {
    pub(super) title: String,
    pub(super) breadcrumbs: Vec<Breadcrumb>,
    pub(super) projects: Vec<ProjectStatus>,
    pub(super) orchestrator: OrchestratorStatus,
}

#[derive(Template)]
#[template(path = "project.html")]
pub(super) struct ProjectTemplate {
    pub(super) title: String,
    pub(super) breadcrumbs: Vec<Breadcrumb>,
    pub(super) name: String,
    pub(super) view: String,
    pub(super) issues: Vec<Task>,
    pub(super) open: Vec<Task>,
    pub(super) in_progress: Vec<Task>,
    pub(super) blocked: Vec<Task>,
    pub(super) in_review: Vec<Task>,
    pub(super) recently_closed: Vec<Task>,
}

#[derive(Template)]
#[template(path = "project_error.html")]
pub(super) struct ProjectErrorTemplate {
    pub(super) title: String,
    pub(super) breadcrumbs: Vec<Breadcrumb>,
    pub(super) error_msg: String,
}

#[derive(Template)]
#[template(path = "issue.html")]
pub(super) struct IssueTemplate {
    pub(super) title: String,
    pub(super) breadcrumbs: Vec<Breadcrumb>,
    pub(super) project_name: String,
    pub(super) issue: IssueDetail,
    pub(super) noc_state: Option<NocIssueState>,
}

#[derive(Template)]
#[template(path = "partials/table_wrapper.html")]
pub(super) struct TableWrapperTemplate {
    pub(super) name: String,
    pub(super) issues: Vec<Task>,
}

#[derive(Template)]
#[template(path = "partials/kanban_board.html")]
pub(super) struct KanbanBoardTemplate {
    pub(super) name: String,
    pub(super) open: Vec<Task>,
    pub(super) in_progress: Vec<Task>,
    pub(super) blocked: Vec<Task>,
    pub(super) in_review: Vec<Task>,
}

#[derive(Template)]
#[template(path = "partials/issue_table_error.html")]
pub(super) struct IssueTableErrorTemplate {
    pub(super) error_msg: String,
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

pub(super) fn into_html_response<T: Template>(tmpl: T) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            error!("template render error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
        }
    }
}
