use std::fmt::Write as _;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tracing::{error, info};

use crate::config::ProjectContext;
use crate::{claude, git, lock, prompt, td, vcs};

/// How long to wait after creating a PR/MR before enabling auto-merge.
///
/// CI pipelines need a moment to register on the newly-created proposal before
/// the platform will accept an auto-merge request. Without this delay the call
/// succeeds but auto-merge silently has no effect because no required checks
/// have been attached yet.
const AUTO_MERGE_DELAY: Duration = Duration::from_secs(5);

pub fn run(ctx: &ProjectContext) -> Result<()> {
    let slug = ctx.project_slug();
    let _lock = lock::Lock::acquire(&ctx.cfg.lock_dir, &format!("proposal-{slug}"))?;

    run_unlocked(ctx)?;
    Ok(())
}

/// Returns Ok(true) if there were proposal tasks to process, Ok(false) if nothing to do.
#[allow(clippy::too_many_lines)]
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
            info!("dry-run: would invoke Claude for proposal of task {task_id}");
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
                if ctx.delete_branch_on_merge {
                    let branch = git::worktree_branch(&task_id);
                    if vcs::delete_remote_branch(&wt_path, &branch) {
                        info!("Remote branch {branch} deleted");
                    } else {
                        info!("Remote branch deletion failed (best-effort)");
                    }
                }
                vcs::run_post_merge_hooks(&ctx.project_root, &ctx.post_merge_hooks);
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
        let comments: Vec<serde_json::Value> = serde_json::from_str(&comments_json)
            .context("failed to parse unresolved comments JSON")?;

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
        let vcs_inline_reply_instructions = match platform {
            vcs::Platform::GitHub => concat!(
                "   **Inline review comment** (`path` is not null) with a `thread_id`:\n",
                "   ```bash\n",
                "   # Reply to the specific comment thread (replace COMMENT_ID with the comment's `id`)\n",
                "   gh api repos/{owner}/{repo}/pulls/PROPOSAL_NUMBER/comments/COMMENT_ID/replies -f body=\"Addressed: <brief summary>\"\n",
                "\n",
                "   # Resolve the thread (replace THREAD_ID with the comment's `thread_id`)\n",
                "   gh api graphql -f query='mutation { resolveReviewThread(input: {threadId: \"THREAD_ID\"}) { thread { isResolved } } }'\n",
                "   ```\n",
            ),
            vcs::Platform::GitLab => concat!(
                "   **Discussion thread comment** (`thread_id` is not null):\n",
                "   ```bash\n",
                "   # Reply within the discussion thread (replace DISCUSSION_ID with the comment's `thread_id`)\n",
                "   glab api --method POST \"projects/:fullpath/merge_requests/PROPOSAL_NUMBER/discussions/DISCUSSION_ID/notes\" -f \"body=Addressed: <brief summary>\"\n",
                "\n",
                "   # Resolve the discussion thread\n",
                "   glab api --method PUT \"projects/:fullpath/merge_requests/PROPOSAL_NUMBER/discussions/DISCUSSION_ID\" -f \"resolved=true\"\n",
                "   ```\n",
            ),
        };
        let vcs_resolve_rule = match platform {
            vcs::Platform::GitHub => {
                "- Resolve inline review threads after addressing them (as described in step 3); do NOT dismiss threads without addressing them"
            }
            vcs::Platform::GitLab => {
                "- Resolve discussion threads after addressing them (as described in step 3); do NOT resolve threads without addressing them"
            }
        };
        let mut rendered = prompt::render_with_vcs(
            prompt::Template::ProposalReview,
            &task_id,
            &ctx.project_root,
            ctx.max_reviews,
            &prompt::VcsPrompt {
                reply_cmd: &vcs_reply_cmd,
                inline_reply_instructions: vcs_inline_reply_instructions,
                resolve_rule: vcs_resolve_rule,
            },
            &ctx.base_branch,
        );
        let _ = write!(
            rendered,
            "\n## Unresolved Comments\n\n```json\n{comments_json}\n```\n"
        );

        let slug = ctx.project_slug();
        let log_file = claude::log_path(&ctx.cfg.log_dir, "proposal", &task_id);

        if ctx.review_backend.run(
            &wt_path,
            &rendered,
            &log_file,
            "proposal",
            &slug,
            &task_id,
            &ctx.review_model,
        )? {
            info!("Proposal review completed");
        } else {
            error!("Proposal review failed");
        }

        // Limit to one Claude invocation per tick
        return Ok(true);
    }

    Ok(false)
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

    let proposal = vcs::create_proposal(platform, &wt_path, &title, desc, &ctx.target_branch)?;
    info!("Proposal created: {platform} #{}", proposal.id);

    // Enable auto-merge (best-effort)
    if ctx.auto_merge {
        // Wait for CI pipelines to register on the new proposal before
        // requesting auto-merge. See AUTO_MERGE_DELAY for full rationale.
        std::thread::sleep(AUTO_MERGE_DELAY);
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
    td_client
        .log(&format!(
            "Created {platform} proposal #{} for {task_id}",
            proposal.id
        ))
        .ok();

    Ok(())
}
