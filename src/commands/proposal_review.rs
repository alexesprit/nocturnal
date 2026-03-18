use anyhow::{Result, bail};
use tracing::{error, info};

use crate::config::ProjectContext;
use crate::{claude, git, lock, prompt, td, vcs};

pub fn run(ctx: &ProjectContext) -> Result<()> {
    let slug = ctx.project_slug();
    let _lock = lock::Lock::acquire(&ctx.cfg.lock_dir, &format!("proposal-review-{slug}"))?;

    run_unlocked(ctx)?;
    Ok(())
}

/// Returns Ok(true) if there were proposal tasks to process, Ok(false) if nothing to do.
pub fn run_unlocked(ctx: &ProjectContext) -> Result<bool> {
    let td_client = td::Td::new(&ctx.project_root);

    let task_ids = td_client.get_proposal_task_ids()?;
    if task_ids.is_empty() {
        info!("No tasks with open proposals");
        return Ok(false);
    }

    let platform = vcs::detect_platform(&ctx.project_root, ctx.vcs_mode)
        .ok_or_else(|| anyhow::anyhow!("No VCS platform detected"))?;

    for task_id in task_ids {
        info!("=== Checking proposal for task: {task_id} ===");

        if ctx.cfg.dry_run {
            info!("dry-run: would invoke Claude for proposal-review of task {task_id}");
            continue;
        }

        let task = td_client.show(&task_id)?;
        let proposal_id = task
            .labels
            .iter()
            .find_map(|l| l.strip_prefix("noc-proposal:"))
            .ok_or_else(|| anyhow::anyhow!("Could not extract proposal ID for {task_id}"))?
            .to_string();

        let wt_path = git::worktree_path(&ctx.project_root, &task_id)?
            .ok_or_else(|| anyhow::anyhow!("No worktree found for {task_id}"))?;

        let state = vcs::get_proposal_state(platform, &wt_path, &proposal_id)?;

        match state {
            vcs::ProposalState::Merged => {
                info!("Proposal #{proposal_id} merged — approving task");
                td_client.approve(&task_id)?;
                continue;
            }
            vcs::ProposalState::Closed => {
                info!("Proposal #{proposal_id} closed without merge — rejecting task");
                let new_count = td::get_review_count(&task) + 1;
                let labels = td::swap_labels(
                    &task,
                    &["noc-proposal:", "noc-reviews:"],
                    Some(&format!("noc-reviews:{new_count}")),
                );
                td_client.update_labels(&task_id, &labels)?;
                td_client
                    .reject(&task_id, "Proposal closed without merging")
                    .ok();
                continue;
            }
            vcs::ProposalState::Open => {}
        }

        // Proposal is open — check for unresolved comments
        let comments_json = vcs::fetch_unresolved_comments(platform, &wt_path, &proposal_id)?;
        let comments: Vec<serde_json::Value> =
            serde_json::from_str(&comments_json).unwrap_or_default();

        if comments.is_empty() {
            info!("No unresolved comments on proposal #{proposal_id}");
            continue;
        }

        info!(
            "Found {} unresolved comments — running Claude to address them",
            comments.len()
        );

        let vcs_reply_cmd = match platform {
            vcs::Platform::GitLab => format!("glab mr note {proposal_id} --message"),
            vcs::Platform::GitHub => format!("gh pr comment {proposal_id} --body"),
        };
        let mut rendered = prompt::render_with_vcs(
            prompt::Template::ProposalReview,
            &task_id,
            &ctx.project_root,
            ctx.max_reviews,
            &vcs_reply_cmd,
        );
        rendered.push_str(&format!(
            "\n## Unresolved Comments\n\n```json\n{comments_json}\n```\n"
        ));

        let slug = ctx.project_slug();
        let log_file = claude::log_path(&ctx.cfg.log_dir, "proposal-review", &task_id);

        if claude::run(
            ctx,
            &wt_path,
            &rendered,
            &log_file,
            "proposal-review",
            &slug,
            &task_id,
        )? {
            info!("Proposal review completed");
        } else {
            error!("Proposal review failed");
        }

        // Limit to one Claude invocation per tick
        return Ok(true);
    }

    Ok(true)
}

pub fn create_proposal(ctx: &ProjectContext, task_id: &str) -> Result<()> {
    let platform = vcs::detect_platform(&ctx.project_root, ctx.vcs_mode)
        .ok_or_else(|| anyhow::anyhow!("No VCS platform detected — cannot create proposal"))?;

    let wt_path = git::worktree_path(&ctx.project_root, task_id)?
        .ok_or_else(|| anyhow::anyhow!("No worktree found for {task_id}"))?;

    let branch = git::worktree_branch(task_id);

    if !git::remote_reachable(&wt_path) {
        bail!("Remote 'origin' is not reachable — skipping proposal creation");
    }

    info!("Pushing branch {branch}");
    git::push_branch(&wt_path, &branch)?;

    let td_client = td::Td::new(&ctx.project_root);
    let task = td_client.show(task_id)?;
    let title = if task.title.is_empty() {
        "No title".to_string()
    } else {
        task.title.replace('\n', " ")
    };
    let desc = &task.description;

    let proposal = vcs::create_proposal(platform, &wt_path, &title, desc)?;
    info!("Proposal created: {platform} #{}", proposal.id);

    // Enable auto-merge (best-effort)
    if ctx.auto_merge {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if vcs::enable_auto_merge(platform, &wt_path, &proposal.id) {
            info!("Auto-merge enabled for #{}", proposal.id);
        } else {
            info!("Auto-merge not available for #{}", proposal.id);
        }
    } else {
        info!("Auto-merge disabled by config");
    }

    // Swap labels
    let labels = td::swap_label(
        &task,
        "noc-proposal-ready",
        Some(&format!("noc-proposal:{}", proposal.id)),
    );
    td_client.update_labels(task_id, &labels)?;

    let comment_url = if proposal.url.is_empty() {
        format!("{platform} #{}", proposal.id)
    } else {
        proposal.url
    };
    td_client
        .comment(task_id, &format!("Proposal opened: {comment_url}"))
        .ok();

    Ok(())
}
