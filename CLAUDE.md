# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

`nocturnal` is a Rust CLI that automates task implementation and code review using Claude Code (`claude -p`) and `td` (task management CLI). It runs unattended via launchd/cron.

## Architecture

**Key flow:** Tasks come from `td`. Each task gets an isolated git worktree (`git gtr new nocturnal/<task-id>`). Claude runs inside the worktree with `--dangerously-skip-permissions`. The orchestrator manages td lifecycle transitions (start → review → approve/reject/block).

**Review cycle protection:** Tracks rejection count via `noc-reviews:N` label. After `MAX_REVIEWS` (default 3) rejections, blocks the task for human attention.

**Worktree-to-task mapping:** Branch naming convention `nocturnal/<task-id>` links worktrees to td tasks. `worktree_path()` scans `git worktree list --porcelain` to resolve paths.

## External Dependencies

- `td` — task management CLI (github.com/marcus/td), stores data in `.todos/` at project root
- `git gtr` — git worktree runner (github.com/coderabbitai/git-worktree-runner), creates isolated worktrees
- `claude` — Claude Code CLI, invoked with `-p` for non-interactive mode

## Prompt Templates

Templates in `prompts/` use `{{PLACEHOLDER}}` syntax, replaced via `String::replace()` in `prompt.rs`:
- `{{TASK_ID}}` — td task identifier
- `{{PROJECT_ROOT}}` — absolute path to main repo (for `td -w`)
- `{{MAX_REVIEWS}}` — max review cycles
- `{{REVIEW_CYCLE}}` — current review cycle number (review prompt)
- `{{VCS_REPLY_CMD}}` — platform-specific reply command (proposal-review prompt)

Claude runs `cd`'d into the worktree. All `td` commands in prompts use `-w "{{PROJECT_ROOT}}"` to reach the `.todos/` database in the main repo.

## Testing

No automated tests. Test manually against a repo with `td init`:
```bash
cd /path/to/repo-with-td-init
/path/to/nocturnal implement    # or: review, run
```

Logs go to `$TMPDIR/nocturnal-logs/`. Check with:
```bash
ls -lt ${TMPDIR}/nocturnal-logs/
```

## Per-Project Configuration

Each project can have a `.nocturnal.toml` in its root. Currently supports:
- `vcs` — VCS integration mode: `"auto"`, `"github"`, `"gitlab"`, or `"off"` (default). Controls whether nocturnal creates MRs/PRs after internal review passes.

## Rust Conventions

- External commands (`td`, `git`, `claude`, `glab`/`gh`) are invoked via `std::process::Command`
- Atomic locking via `fs::create_dir` (directory-based, similar to `mkdir` atomicity)
- `$TMPDIR` for temp/log files, not hardcoded `/tmp`
