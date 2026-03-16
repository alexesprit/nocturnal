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
# Auto-select: check proposals → review → implement
cd /path/to/repo
nocturnal run

# Or specify the project explicitly
nocturnal --project /path/to/repo run

# Run a specific phase
nocturnal implement        # Pick and implement the next open task
nocturnal review           # Pick and review the next reviewable task
nocturnal proposal-review  # Address comments on open MR/PR
```

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
# Round-robin: process one project per invocation
nocturnal rotate

# All at once: process every project in a single invocation
nocturnal foreach
```

### Recommended Schedule

Use `rotate` nightly to pick up and implement/review tasks, and `proposal-review` frequently (e.g. every 30 minutes) to address MR/PR comments promptly:

#### cron

```cron
# Rotate through projects nightly at 2 AM
0 2 * * * nocturnal rotate

# Check proposals for review comments every 30 minutes
*/30 * * * * nocturnal --project /path/to/repo proposal-review
```

#### launchd

Nightly rotation (`~/Library/LaunchAgents/com.nocturnal.rotate.plist`):

```xml
<key>ProgramArguments</key>
<array>
  <string>/path/to/nocturnal</string>
  <string>rotate</string>
</array>
<key>StartCalendarInterval</key>
<dict>
  <key>Hour</key>
  <integer>2</integer>
  <key>Minute</key>
  <integer>0</integer>
</dict>
```

Proposal review every 30 minutes (`~/Library/LaunchAgents/com.nocturnal.proposal-review.plist`):

```xml
<key>ProgramArguments</key>
<array>
  <string>/path/to/nocturnal</string>
  <string>--project</string>
  <string>/path/to/repo</string>
  <string>proposal-review</string>
</array>
<key>StartInterval</key>
<integer>1800</integer>
```

## Configuration

All configuration is via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `NOCTURNAL_MAX_REVIEWS` | `3` | Max review cycles before blocking a task |
| `NOCTURNAL_MAX_BUDGET` | `5` | Max USD per Claude run |
| `NOCTURNAL_MODEL` | `sonnet` | Claude model to use |
| `NOCTURNAL_LOG_DIR` | `$TMPDIR/nocturnal-logs` | Log output directory |
| `NOCTURNAL_LOCK_DIR` | `$TMPDIR` | Lock file directory |
| `NOCTURNAL_PROJECTS` | — | Colon-separated project paths (alternative to projects file) |
| `NOCTURNAL_PROJECTS_FILE` | `~/.config/nocturnal/projects` | Project list file |
| `NOCTURNAL_ROTATION_STATE` | `~/.config/nocturnal/rotation-state` | Rotation index persistence |

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

VCS integration is configured per project via `.nocturnal.toml` in the project root:

```toml
# "auto"   — detect GitLab/GitHub from origin remote URL
# "github" — force GitHub
# "gitlab" — force GitLab
# "off"    — no proposals (default if file is missing)
vcs = "auto"
```

When VCS is enabled and a task passes internal review:

1. Branch is pushed to origin
2. MR (GitLab) or PR (GitHub) is created with the task title and description
3. Auto-merge is enabled if the platform supports it
4. On subsequent runs, nocturnal checks for unresolved comments and runs Claude to address them
5. Once the proposal is merged, the task is closed

## Logs

```bash
ls -lt ${TMPDIR}/nocturnal-logs/
```

Log files are named `<phase>-<task-id>-<timestamp>.log` and contain the full Claude output for each run.

## License

MIT
