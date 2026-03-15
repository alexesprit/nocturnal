# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

`nocturnal` is a bash orchestrator that automates task implementation and code review using Claude Code (`claude -p`) and `td` (task management CLI). It runs unattended via launchd/cron.

## Architecture

**Key flow:** Tasks come from `td`. Each task gets an isolated git worktree (`git gtr new nocturnal/<task-id>`). Claude runs inside the worktree with `--dangerously-skip-permissions`. The orchestrator manages td lifecycle transitions (start → review → approve/reject/block).

**Review cycle protection:** Tracks rejection count via `noc-reviews:N` label. After `MAX_REVIEWS` (default 3) rejections, blocks the task for human attention.

**Worktree-to-task mapping:** Branch naming convention `nocturnal/<task-id>` links worktrees to td tasks. `worktree_path()` scans `git worktree list --porcelain` to resolve paths.

## External Dependencies

- `td` — task management CLI (github.com/marcus/td), stores data in `.todos/` at project root
- `git gtr` — git worktree runner (github.com/coderabbitai/git-worktree-runner), creates isolated worktrees
- `claude` — Claude Code CLI, invoked with `-p` for non-interactive mode
- `jq` — JSON parsing for td output

## Prompt Templates

Templates in `prompts/` use `{{PLACEHOLDER}}` syntax, replaced by `sed` at runtime:
- `{{TASK_ID}}` — td task identifier
- `{{PROJECT_ROOT}}` — absolute path to main repo (for `td -w`)
- `{{MAX_REVIEWS}}` — max review cycles

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

## Shell Conventions

- `set -euo pipefail` — strict mode
- No `git -C` — use `(cd "$dir" && git ...)` subshells
- `readlink -f` for resolving symlinks (requires GNU coreutils on macOS)
- Atomic locking via `mkdir` (no `flock` on macOS)
- `$TMPDIR` for temp files, not hardcoded `/tmp`
