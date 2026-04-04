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
- [Codex CLI](https://github.com/openai/codex) (`codex`) — optional, alternative AI backend

## Installation

```bash
cargo install --path .
```

Then bootstrap a project with `nocturnal init` (see [Usage](#usage)).

## Usage

### Initialize a Project

```bash
cd /path/to/repo
nocturnal init           # check tools, run td init, create .nocturnal.toml
nocturnal init --dry-run # preview without making changes
```

`init` checks for required tools (`td`, `git`, `claude`, `git-gtr`) and optional VCS tools (`gh`/`glab`) based on the detected git remote. It runs `td init` if needed, creates `.nocturnal.toml` with sensible defaults, and creates the `.nocturnal/` prompt extras directory.

### Single Project

```bash
# Auto-select: review → implement (default command)
cd /path/to/repo
nocturnal develop

# Or specify the project explicitly
nocturnal --project /path/to/repo develop
nocturnal develop /path/to/repo

# Target a specific task
nocturnal develop --task td-abc123

# Address comments on open MR/PR
nocturnal proposal

# Run in a loop until nothing is left to do
nocturnal loop
nocturnal loop --max-iterations 5
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

Then use the `--all` flag to run across all configured projects:

```bash
# Implement/review one task across all projects
nocturnal develop --all

# Check proposals for review comments across all projects
nocturnal proposal --all

# Loop until nothing is left to do across all projects
nocturnal loop --all
```

### Recommended Schedule

Run `develop --all` in overnight batches on weeknights to implement and review tasks. Run `proposal --all` every 15 minutes to respond promptly to MR/PR comments. The two commands use separate locks and operate on disjoint task states, so they can run concurrently without conflict.

#### cron

```cron
# Process all projects nightly at 2 AM
0 2 * * * nocturnal develop --all

# Check proposals for review comments every hour
0 * * * * nocturnal proposal --all
```

#### launchd

`~/Library/LaunchAgents/com.nocturnal.develop.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.nocturnal.develop</string>

  <!-- Use a login shell so PATH includes Homebrew, cargo, etc. -->
  <!-- caffeinate -s prevents macOS sleep during long Claude runs -->
  <key>ProgramArguments</key>
  <array>
    <string>/bin/zsh</string>
    <string>-l</string>
    <string>-c</string>
    <string>caffeinate -s nocturnal develop --all</string>
  </array>

  <!-- Overnight batch: 1 AM, 2 AM, 3 AM Monday; daytime: 2 PM Monday -->
  <!-- Add or remove entries to match your preferred schedule -->
  <key>StartCalendarInterval</key>
  <array>
    <dict>
      <key>Hour</key><integer>1</integer>
      <key>Minute</key><integer>0</integer>
      <key>Weekday</key><integer>1</integer>
    </dict>
    <dict>
      <key>Hour</key><integer>2</integer>
      <key>Minute</key><integer>0</integer>
      <key>Weekday</key><integer>1</integer>
    </dict>
    <dict>
      <key>Hour</key><integer>3</integer>
      <key>Minute</key><integer>0</integer>
      <key>Weekday</key><integer>1</integer>
    </dict>
    <dict>
      <key>Hour</key><integer>14</integer>
      <key>Minute</key><integer>0</integer>
      <key>Weekday</key><integer>1</integer>
    </dict>
  </array>

  <key>StandardOutPath</key>
  <string>/tmp/nocturnal-logs/launchd-develop.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/nocturnal-logs/launchd-develop.log</string>
</dict>
</plist>
```

`~/Library/LaunchAgents/com.nocturnal.proposal.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.nocturnal.proposal</string>

  <key>ProgramArguments</key>
  <array>
    <string>/bin/zsh</string>
    <string>-l</string>
    <string>-c</string>
    <string>nocturnal proposal --all</string>
  </array>

  <!-- Every 15 minutes -->
  <key>StartCalendarInterval</key>
  <array>
    <dict><key>Minute</key><integer>0</integer></dict>
    <dict><key>Minute</key><integer>15</integer></dict>
    <dict><key>Minute</key><integer>30</integer></dict>
    <dict><key>Minute</key><integer>45</integer></dict>
  </array>

  <key>StandardOutPath</key>
  <string>/tmp/nocturnal-logs/launchd-proposal.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/nocturnal-logs/launchd-proposal.log</string>
</dict>
</plist>
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
# AI backend: "claude" (default) or "codex"
provider = "claude"

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

[codex]
# Default Codex model (default: "gpt-5.4")
model = "gpt-5.4"

# Override model for implement/develop operations (optional, falls back to model)
implement_model = "o4-mini"

# Override model for review/proposal-review operations (optional, falls back to model)
review_model = "o3-mini"

# Reasoning effort level (default: "high")
reasoning_effort = "high"

[vcs]
# "auto"   — detect GitLab/GitHub from origin remote URL
# "github" — force GitHub
# "gitlab" — force GitLab
# "local"  — merge directly into the local target branch after approval
# "off"    — no proposals (default if section is missing)
mode = "auto"

# Branch worktrees are created from (default: "main")
base_branch = "main"

# Target branch for proposals or local merges (default: same as base_branch)
target_branch = "main"

# Local merge strategy: "ff", "no-ff", or "rebase"
# Defaults to "rebase" in local mode and "ff" otherwise.
merge_strategy = "rebase"

# Enable auto-merge on created proposals (default: true)
# Set to false if your repo has no branch protection rules,
# otherwise the PR/MR will be merged immediately on creation.
auto_merge = false

# Delete the remote branch after a proposal is merged (default: false)
delete_branch_on_merge = false
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

## Security

nocturnal runs Claude with `--dangerously-skip-permissions`, giving it unrestricted filesystem and command execution access. Task descriptions are untrusted input that can execute arbitrary code under your user account. Worktree isolation limits accidental branch-level changes but is not a security boundary.

See the **Security / Trust Model** section in `CLAUDE.md` for the full trust boundary analysis and recommended mitigations.

## License

MIT
