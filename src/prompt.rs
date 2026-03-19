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

    fn extra_filename(&self) -> &'static str {
        match self {
            Template::Implement => "prompt-implement.md",
            Template::Review => "prompt-review.md",
            Template::ProposalReview => "prompt-proposal-review.md",
        }
    }
}

fn append_extra(prompt: &mut String, project_root: &str, template: &Template) {
    let dir = format!("{}/.nocturnal", project_root);
    if let Ok(shared) = std::fs::read_to_string(format!("{}/prompt-extra.md", dir)) {
        prompt.push('\n');
        prompt.push_str(&shared);
    }
    if let Ok(specific) = std::fs::read_to_string(format!("{}/{}", dir, template.extra_filename()))
    {
        prompt.push('\n');
        prompt.push_str(&specific);
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
    vcs_inline_reply_instructions: &str,
    vcs_resolve_rule: &str,
) -> String {
    render_base(template, task_id, project_root, max_reviews)
        .replace("{{VCS_REPLY_CMD}}", vcs_reply_cmd)
        .replace("{{VCS_INLINE_REPLY_INSTRUCTIONS}}", vcs_inline_reply_instructions)
        .replace("{{VCS_RESOLVE_RULE}}", vcs_resolve_rule)
}

pub fn render_base(
    template: Template,
    task_id: &str,
    project_root: &str,
    max_reviews: u32,
) -> String {
    let mut result = template
        .content()
        .replace("{{TASK_ID}}", task_id)
        .replace("{{PROJECT_ROOT}}", project_root)
        .replace("{{MAX_REVIEWS}}", &max_reviews.to_string());
    append_extra(&mut result, project_root, &template);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn setup_nocturnal_dir(base: &PathBuf) -> PathBuf {
        let dir = base.join(".nocturnal");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

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
            "",
            "",
        );
        assert!(!result.contains("{{VCS_REPLY_CMD}}"));
        assert!(!result.contains("{{VCS_INLINE_REPLY_INSTRUCTIONS}}"));
        assert!(!result.contains("{{VCS_RESOLVE_RULE}}"));
        assert!(result.contains("glab mr note 42"));
    }

    #[test]
    fn append_extra_shared_only() {
        let tmp = tempdir();
        let dir = setup_nocturnal_dir(&tmp);
        fs::write(dir.join("prompt-extra.md"), "shared content").unwrap();
        let mut prompt = String::from("base");
        append_extra(&mut prompt, tmp.to_str().unwrap(), &Template::Implement);
        assert!(prompt.contains("base"));
        assert!(prompt.contains("shared content"));
    }

    #[test]
    fn append_extra_template_specific_only() {
        let tmp = tempdir();
        let dir = setup_nocturnal_dir(&tmp);
        fs::write(dir.join("prompt-implement.md"), "impl extra").unwrap();
        let mut prompt = String::from("base");
        append_extra(&mut prompt, tmp.to_str().unwrap(), &Template::Implement);
        assert!(prompt.contains("impl extra"));
    }

    #[test]
    fn append_extra_both_shared_and_specific() {
        let tmp = tempdir();
        let dir = setup_nocturnal_dir(&tmp);
        fs::write(dir.join("prompt-extra.md"), "shared").unwrap();
        fs::write(dir.join("prompt-review.md"), "review extra").unwrap();
        let mut prompt = String::from("base");
        append_extra(&mut prompt, tmp.to_str().unwrap(), &Template::Review);
        let shared_pos = prompt.find("shared").unwrap();
        let specific_pos = prompt.find("review extra").unwrap();
        assert!(
            shared_pos < specific_pos,
            "shared must come before specific"
        );
    }

    #[test]
    fn append_extra_no_files_no_change() {
        let tmp = tempdir();
        let mut prompt = String::from("base");
        append_extra(&mut prompt, tmp.to_str().unwrap(), &Template::Implement);
        assert_eq!(prompt, "base");
    }

    #[test]
    fn append_extra_template_specific_not_mixed() {
        let tmp = tempdir();
        let dir = setup_nocturnal_dir(&tmp);
        fs::write(dir.join("prompt-review.md"), "review extra").unwrap();
        let mut prompt = String::from("base");
        append_extra(&mut prompt, tmp.to_str().unwrap(), &Template::Implement);
        assert!(!prompt.contains("review extra"));
    }

    #[test]
    fn render_base_appends_extra_when_files_present() {
        let tmp = tempdir();
        let dir = setup_nocturnal_dir(&tmp);
        fs::write(dir.join("prompt-extra.md"), "SHARED_EXTRA").unwrap();
        let result = render_base(Template::Implement, "task-1", tmp.to_str().unwrap(), 3);
        assert!(result.contains("SHARED_EXTRA"));
    }

    fn tempdir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nocturnal-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
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
