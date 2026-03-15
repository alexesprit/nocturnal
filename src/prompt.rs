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

pub fn render(template: Template, task_id: &str, project_root: &str, max_reviews: u32) -> String {
    template
        .content()
        .replace("{{TASK_ID}}", task_id)
        .replace("{{PROJECT_ROOT}}", project_root)
        .replace("{{MAX_REVIEWS}}", &max_reviews.to_string())
}
