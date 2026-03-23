# nocturnal

Unattended task orchestrator that automates implementation and code review using [Claude Code](https://docs.anthropic.com/en/docs/claude-code) and [td](https://github.com/marcus/td). Designed to run on a schedule via launchd or cron — pick up tasks, implement them in isolated worktrees, self-review, open proposals (MR/PR), and address review comments, all without human intervention.

## How It Works

```
td task queue ──► nocturnal picks highest-priority task
                      │
                      ▼
               create git worktree
              (nocturnal/<task-id>)
                      │
                      ▼
              Claude implements task
              (claude -p in worktree)
                      │
                      ▼
              Claude self-reviews diff
                      │
                ┌─────┴─────┐
                ▼           ▼
            rejected     approved
            (td reject)     │
                │           ▼
                │     push & open MR/PR
                │           │
                ▼           ▼
           re-implement  human reviews
                        proposal on
                        GitLab/GitHub
                            │
                      ┌─────┴─────┐
                      ▼           ▼
                  comments     merged
                      │           │
                      ▼           ▼
              Claude addresses  td approve
              review comments   (task closed)
```

Tasks that fail review after multiple cycles are blocked for human attention.

## Requirements

- [td](https://github.com/marcus/td) — task management CLI
- [git-worktree-runner](https://github.com/coderabbitai/git-worktree-runner) (`git gtr`) — isolated worktree creation
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (`claude`) — AI coding agent
- [glab](https://gitlab.com/gitlab-org/cli) (GitLab) or [gh](https://cli.github.com/) (GitHub) — optional, for proposal creation

## Installation

```bash
cargo install --path .
```

The target repository must have `td` initialized (`td init`).

## Usage

### Single Project

```bash
# Auto-select: review → implement (default command)
cd /path/to/repo
nocturnal develop

# Or specify the project explicitly
nocturnal --project /path/to/repo develop

# Run a specific phase
nocturnal implement  # Pick and implement the next open task
nocturnal review     # Pick and review the next reviewable task
nocturnal proposal   # Address comments on open MR/PR
```

### Web Dashboard

Start a read-only dashboard to browse tasks across all configured projects:

```bash
nocturnal web                        # http://localhost:8090
nocturnal web --port 3000            # custom port
nocturnal web --addr 0.0.0.0         # listen on all interfaces
```

Requires projects to be configured (via `NOCTURNAL_PROJECTS` env var or the projects file). The dashboard shows task lists per project and individual task detail pages.

### Multiple Projects

Create a project list at `~/.config/nocturnal/projects`:

```
# One path per line
/path/to/project-a
/path/to/project-b
```

Or pass via environment:

```bash
export NOCTURNAL_PROJECTS=/path/to/project-a:/path/to/project-b
```

Then choose a scheduling strategy:

```bash
# Round-robin: implement/review one project per invocation
nocturnal develop-rotate

# All at once: process every project in a single invocation
nocturnal foreach

# Round-robin: check proposals for one project per invocation
nocturnal proposal-rotate
```

### Recommended Schedule

Run `develop-rotate` nightly to implement/review tasks, and `proposal-rotate` frequently (e.g. every hour) to address MR/PR comments promptly. The two commands use separate locks and operate on disjoint task states, so they can run concurrently without conflict.

#### cron

```cron
# Rotate through projects nightly at 2 AM
0 2 * * * nocturnal develop-rotate

# Check proposals for review comments every hour
0 * * * * nocturnal proposal-rotate
```

#### launchd

Nightly rotation (`~/Library/LaunchAgents/com.nocturnal.rotate.plist`):

```xml
<key>ProgramArguments</key>
<array>
  <string>/path/to/nocturnal</string>
  <string>develop-rotate</string>
</array>
<key>StartCalendarInterval</key>
<dict>
  <key>Hour</key>
  <integer>2</integer>
  <key>Minute</key>
  <integer>0</integer>
</dict>
```

Proposal review every hour (`~/Library/LaunchAgents/com.nocturnal.proposal-review.plist`):

```xml
<key>ProgramArguments</key>
<array>
  <string>/path/to/nocturnal</string>
  <string>proposal-rotate</string>
</array>
<key>StartInterval</key>
<integer>3600</integer>
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NOCTURNAL_LOG_DIR` | `$TMPDIR/nocturnal-logs` | Log output directory |
| `NOCTURNAL_LOCK_DIR` | `$TMPDIR` | Lock file directory |
| `NOCTURNAL_PROJECTS` | — | Colon-separated project paths (alternative to projects file) |
| `NOCTURNAL_PROJECTS_FILE` | `~/.config/nocturnal/projects` | Project list file |
| `NOCTURNAL_ROTATION_STATE` | `~/.config/nocturnal/rotation-state` | Rotation index persistence |

### Per-Project Configuration

Each project can have a `.nocturnal.toml` in its root:

```toml
# Max review cycles before blocking a task for human attention (default: 3)
max_reviews = 3

# Max USD budget per Claude run; omit for no limit (default: unlimited)
max_budget = 10

[claude]
# Default model for all operations (default: "sonnet")
model = "sonnet"

# Override model for implement/develop operations (optional, falls back to model)
implement_model = "opus"

# Override model for review/proposal-review operations (optional, falls back to model)
review_model = "haiku"

[vcs]
# "auto"   — detect GitLab/GitHub from origin remote URL
# "github" — force GitHub
# "gitlab" — force GitLab
# "local"  — merge directly into the local target branch after approval
# "off"    — no proposals (default if section is missing)
mode = "auto"

# Target branch for proposals or local merges (default: "main")
target_branch = "main"

# Local merge strategy: "ff", "no-ff", or "rebase"
# Defaults to "rebase" in local mode and "ff" otherwise.
merge_strategy = "rebase"

# Enable auto-merge on created proposals (default: true)
# Set to false if your repo has no branch protection rules,
# otherwise the PR/MR will be merged immediately on creation.
auto_merge = false
```

## Task Lifecycle

nocturnal tracks state through `td` statuses and labels:

| Status | Meaning |
|--------|---------|
| `open` | Available for implementation |
| `in_progress` | Being implemented |
| `in_review` | Awaiting review |
| `closed` | Proposal merged, task complete |
| `blocked` | Exceeded max review cycles, needs human |

| Label | Meaning |
|-------|---------|
| `noc-reviews:N` | Number of review cycles consumed |
| `noc-proposal-ready` | Internal review passed, proposal not yet created |
| `noc-proposal:<id>` | Proposal open on VCS platform |

## VCS Integration

VCS integration is configured per project via the `[vcs]` section in `.nocturnal.toml` (see [Per-Project Configuration](#per-project-configuration) above).

When VCS is enabled and a task passes internal review:

1. Branch is pushed to origin
2. MR (GitLab) or PR (GitHub) is created with the task title and description
3. Auto-merge is enabled (if `auto_merge = true` and the platform supports it)
4. On subsequent runs, nocturnal checks for unresolved comments and runs Claude to address them
5. Once the proposal is merged, the task is closed

## Logs

```bash
ls -lt ${TMPDIR}/nocturnal-logs/
```

Log files are named `<phase>-<task-id>-<timestamp>.log` and contain the full Claude output for each run.

## License

MIT
