# CLAUDE.md

## What This Is

`nocturnal` is a Rust CLI that automates task implementation and code review using Claude Code (`claude -p`) and `td` (task management CLI). It runs unattended via launchd/cron.

## Architecture

**Key flow:** Tasks come from `td`. Each task gets an isolated git worktree (`git gtr new nocturnal/<task-id>`). The selected AI backend runs inside the worktree. The orchestrator manages td lifecycle transitions (start → review → approve/reject/block).

**Multi-backend AI support:** The `AiBackend` trait in `src/backend.rs` abstracts over AI providers. `ClaudeBackend` invokes `claude -p --dangerously-skip-permissions`; `CodexBackend` invokes `codex exec --full-auto`. The active backend is selected per project via the `provider` field in `.nocturnal.toml`.

**Review cycle protection:** Tracks rejection count via `noc-reviews:N` label. After `MAX_REVIEWS` (default 3) rejections, blocks the task for human attention.

**Worktree-to-task mapping:** Branch naming convention `nocturnal/<task-id>` links worktrees to td tasks. `worktree_path()` scans `git worktree list --porcelain` to resolve paths.

## Web Dashboard

`nocturnal web` starts a read-only axum HTTP server (default `localhost:8090`) that renders td task lists and detail pages across all configured projects. Templates live in `src/web_templates/` (askama), static assets in `src/static/`. The `src/web/` module contains handlers, models, markdown rendering (pulldown-cmark), and template filters. Binds only to loopback by default; warns if a non-loopback address is given.

## External Dependencies

- `td` — task management CLI (github.com/marcus/td), stores data in `.todos/` at project root
- `git gtr` — git worktree runner (github.com/coderabbitai/git-worktree-runner), creates isolated worktrees
- `claude` — Claude Code CLI, invoked with `-p` for non-interactive mode
- `codex` — OpenAI Codex CLI (optional), used when `provider = "codex"` in `.nocturnal.toml`

## Prompt Templates

Templates in `prompts/` use `{{PLACEHOLDER}}` syntax, replaced via `String::replace()` in `prompt.rs`:
- `{{TASK_ID}}` — td task identifier
- `{{PROJECT_ROOT}}` — absolute path to main repo (for `td -w`)
- `{{MAX_REVIEWS}}` — max review cycles
- `{{REVIEW_CYCLE}}` — current review cycle number (review prompt)
- `{{VCS_REPLY_CMD}}` — platform-specific reply command (proposal prompt)

Claude runs `cd`'d into the worktree. All `td` commands in prompts use `-w "{{PROJECT_ROOT}}"` to reach the `.todos/` database in the main repo.

## Testing

Unit tests exist in `config.rs`, `project_config.rs`, `prompt.rs`, `td.rs`, and `claude.rs`. Run them with:
```bash
cargo test
```

For manual integration testing against a repo with `td init`:
```bash
cd /path/to/repo-with-td-init
/path/to/nocturnal develop    # or: proposal, loop, gc
```

Logs go to `$TMPDIR/nocturnal-logs/`. Check with:
```bash
ls -lt ${TMPDIR}/nocturnal-logs/
```

## Per-Project Configuration

Each project can have a `.nocturnal.toml` in its root. Top-level fields:
- `provider` — AI backend: `"claude"` (default) or `"codex"`
- `implement_provider` — override provider for implement/develop (falls back to `provider`)
- `review_provider` — override provider for review/proposal-review (falls back to `provider`)
- `max_reviews` — max review cycles before blocking a task (default `3`)
- `max_budget` — max USD per Claude run; omit for no budget limit (default: unlimited)
- `auto_develop` — boolean (default `true`). When set to `false`, `develop --all`, `proposal --all`, and `loop --all` skip the project. The web dashboard continues to show the project regardless.

`[claude]` section (per-operation model config):
- `model` — default Claude model for all operations (default `"sonnet"`)
- `implement_model` — override model for implement/develop (falls back to `model`)
- `review_model` — override model for review/proposal-review (falls back to `model`)

`[codex]` section (used when `provider = "codex"`):
- `model` — default Codex model (default `"gpt-5.4"`)
- `implement_model` — override model for implement/develop (falls back to `model`)
- `review_model` — override model for review/proposal-review (falls back to `model`)
- `reasoning_effort` — Codex reasoning effort level (default `"high"`)

`[vcs]` section:
- `mode` — VCS integration mode: `"auto"`, `"github"`, `"gitlab"`, `"local"`, or `"off"` (default). `"local"` merges directly into the configured target branch after internal review passes.
- `auto_merge` — boolean (default `true`). When `false`, nocturnal creates the PR/MR but does not enable auto-merge.
- `delete_branch_on_merge` — boolean (default `false`). When `true`, deletes the remote branch after a proposal is merged.
- `base_branch` — branch to create worktrees from (default `"main"`).
- `target_branch` — branch to open proposals against or merge into (default: same as `base_branch`).
- `merge_strategy` — local merge strategy: `"ff"`, `"no-ff"`, or `"rebase"`. Defaults to `"rebase"` for `mode = "local"`, otherwise `"ff"`.

`[hooks]` section:
- `pre_merge` — list of shell commands to run before merging (default: empty)
- `post_merge` — list of shell commands to run after merging (default: empty)

## Prompt Extras

Prompt content can be extended per project without modifying the built-in templates. Place files in `.nocturnal/` at the project root:
- `prompt-extra.md` — appended to **all** templates
- `prompt-implement.md` — appended to the implement template only
- `prompt-review.md` — appended to the review template only
- `prompt-proposal-review.md` — appended to the proposal-review template only

Shared content is appended before template-specific content.

## Security / Trust Model

nocturnal invokes Claude with `--dangerously-skip-permissions`, which grants the spawned process unrestricted filesystem and command execution access. This is required for unattended operation — Claude cannot prompt the user for permission approvals at runtime.

**Consequences operators must understand:**

1. **Task descriptions are untrusted code execution vectors.** Any text in a `td` task title, description, or acceptance criteria is passed as a prompt to Claude running with full permissions. A malicious or malformed task could instruct Claude to exfiltrate files, modify system configuration, or run arbitrary commands.

2. **Worktree isolation is the primary containment boundary — not a security boundary.** Each task runs in an isolated git worktree (`nocturnal/<task-id>`), which limits the blast radius for accidental file changes to that branch. It does not prevent reads across the filesystem, network access, or execution of system binaries.

3. **Operators accept full code-execution risk for any task text.** Only feed tasks into nocturnal that you would be comfortable running as a shell script under your user account. Treat the `td` task database as a trusted execution surface.

**Mitigation strategies:**

- Run nocturnal under a dedicated low-privilege OS user with restricted filesystem access
- Use `max_budget` in `.nocturnal.toml` to cap spend per run
- Review task descriptions before they enter the `open` queue
- Monitor `$TMPDIR/nocturnal-logs/` for unexpected command output

## Rust Conventions

- External commands (`td`, `git`, `claude`, `glab`/`gh`) are invoked via `std::process::Command`
- Atomic locking via `fs::create_dir` (directory-based, similar to `mkdir` atomicity)
- `$TMPDIR` for temp/log files, not hardcoded `/tmp`
