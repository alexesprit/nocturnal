const IMPLEMENT: &str = include_str!("../prompts/implement.md");
const REVIEW: &str = include_str!("../prompts/review.md");
const PROPOSAL_REVIEW: &str = include_str!("../prompts/proposal-review.md");

pub enum Template {
    Implement,
    Review,
    ProposalReview,
}

impl Template {
    fn content(&self) -> &'static str {
        match self {
            Template::Implement => IMPLEMENT,
            Template::Review => REVIEW,
            Template::ProposalReview => PROPOSAL_REVIEW,
        }
    }
}

pub fn render_with_review_cycle(
    template: Template,
    task_id: &str,
    project_root: &str,
    max_reviews: u32,
    review_cycle: Option<u32>,
) -> String {
    let mut result = render_base(template, task_id, project_root, max_reviews);
    if let Some(cycle) = review_cycle {
        result = result.replace("{{REVIEW_CYCLE}}", &cycle.to_string());
    }
    result
}

pub fn render_with_vcs(
    template: Template,
    task_id: &str,
    project_root: &str,
    max_reviews: u32,
    vcs_reply_cmd: &str,
) -> String {
    render_base(template, task_id, project_root, max_reviews)
        .replace("{{VCS_REPLY_CMD}}", vcs_reply_cmd)
}

pub fn render_base(
    template: Template,
    task_id: &str,
    project_root: &str,
    max_reviews: u32,
) -> String {
    template
        .content()
        .replace("{{TASK_ID}}", task_id)
        .replace("{{PROJECT_ROOT}}", project_root)
        .replace("{{MAX_REVIEWS}}", &max_reviews.to_string())
}
