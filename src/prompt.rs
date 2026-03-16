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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_base_replaces_all_base_placeholders() {
        let result = render_base(Template::Implement, "task-42", "/home/project", 5);
        assert!(!result.contains("{{TASK_ID}}"));
        assert!(!result.contains("{{PROJECT_ROOT}}"));
        assert!(!result.contains("{{MAX_REVIEWS}}"));
        assert!(result.contains("task-42"));
        assert!(result.contains("/home/project"));
        assert!(result.contains("5"));
    }

    #[test]
    fn render_with_review_cycle_replaces_cycle_placeholder() {
        let result = render_with_review_cycle(Template::Review, "task-1", "/root", 3, Some(2));
        assert!(!result.contains("{{REVIEW_CYCLE}}"));
        assert!(result.contains("2"));
    }

    #[test]
    fn render_with_review_cycle_none_leaves_placeholder() {
        let result = render_with_review_cycle(Template::Review, "task-1", "/root", 3, None);
        assert!(result.contains("{{REVIEW_CYCLE}}"));
    }

    #[test]
    fn render_with_vcs_replaces_vcs_placeholder() {
        let result = render_with_vcs(
            Template::ProposalReview,
            "task-1",
            "/root",
            3,
            "glab mr note 42",
        );
        assert!(!result.contains("{{VCS_REPLY_CMD}}"));
        assert!(result.contains("glab mr note 42"));
    }

    #[test]
    fn render_base_does_not_leave_base_placeholders_in_any_template() {
        for (template, name) in [
            (Template::Implement, "implement"),
            (Template::Review, "review"),
            (Template::ProposalReview, "proposal-review"),
        ] {
            let result = render_base(template, "id", "/proj", 3);
            assert!(
                !result.contains("{{TASK_ID}}"),
                "{name} still contains {{{{TASK_ID}}}}"
            );
            assert!(
                !result.contains("{{PROJECT_ROOT}}"),
                "{name} still contains {{{{PROJECT_ROOT}}}}"
            );
            assert!(
                !result.contains("{{MAX_REVIEWS}}"),
                "{name} still contains {{{{MAX_REVIEWS}}}}"
            );
        }
    }
}
