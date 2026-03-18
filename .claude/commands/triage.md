# Triage: Evaluate feature/bug for nocturnal implementation

Evaluate whether a described change (feature or bug fix) is suitable for automated implementation via nocturnal + td. If suitable, create a td task with a detailed description.

## Input

The user provides: $ARGUMENTS

If the description is empty or missing, use AskUserQuestion to ask the user to describe the change they want.

## Evaluation process

### Step 1: Understand the project

Read the project's CLAUDE.md to understand:
- What the project does
- Its architecture and conventions
- How it's built and tested

### Step 2: Explore relevant code

Based on the described change, find the relevant files and understand the scope of work needed. Use Grep, Glob, and Read to explore. Be thorough enough to assess feasibility but don't over-explore.

### Step 3: Evaluate suitability

Score the change against these criteria. Use AskUserQuestion if any criterion is genuinely unclear from the description alone.

**Must-pass criteria (all required for "go"):**

1. **Well-scoped**: The change has a single, clear objective. It's not a sprawling refactor or a vague "improve X" request. You can describe what files change and roughly how.

2. **Worktree-compatible**: The change can be done in an isolated git worktree. It doesn't require:
   - Coordinated changes across multiple repositories
   - Running long-lived services or manual testing
   - Access to external credentials, APIs, or infrastructure
   - Interactive debugging or UI verification

3. **Clear acceptance criteria**: You can define concrete, verifiable conditions for "done". Claude reviewing the code can determine pass/fail without running the app manually.

### Step 4: Decide and act

**If GO**: Create a td task using `td create` with:
- A clear, concise title
- A detailed description including:
  - What to change and why
  - Which files/modules are involved
  - Implementation approach (specific enough for an autonomous agent)
- Acceptance criteria via `--acceptance` flag (as a checklist)
- Appropriate priority (`-p P1`/`P2`/`P3` — ask if unclear)
- Type (`-t task`, `bug`, `feature`, or `chore`)
- Dependencies (`--depends-on <id>`) if this depends on an existing open task

Report the created task ID and a brief summary.

**If NO-GO**: Explain which criteria failed and why. Suggest how the user could restructure the request to make it suitable (e.g., break into smaller tasks, clarify scope).

## Guidelines

- Err toward asking clarifying questions rather than guessing intent
- When exploring code, focus on understanding scope — you don't need to plan every line of the implementation
- The task description should be detailed enough that nocturnal's implement agent can work autonomously without asking questions
- Check existing td tasks (`td list`) to avoid duplicates and to identify dependencies
- Don't over-specify implementation details when the approach is obvious from the codebase patterns
