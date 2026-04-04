use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer};
use tracing::debug;

/// Validates a task ID against `^[a-zA-Z0-9_-]+$`.
/// Returns an error for IDs that could cause path traversal or flag injection.
pub(crate) fn validate_task_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("invalid task ID: {id:?}");
    }
    Ok(())
}

pub(crate) fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

#[allow(dead_code, clippy::struct_field_names)] // fields used by askama templates
#[derive(Debug, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub title: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub status: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub labels: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub priority: String,
    #[serde(rename = "type", default, deserialize_with = "null_as_default")]
    pub task_type: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub sprint: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub created_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub updated_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub closed_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub parent_id: String,
}

/// Detailed view of a task, as returned by `td show --json`.
#[allow(dead_code)] // fields used by askama templates
#[derive(Debug, Deserialize)]
pub struct IssueDetail {
    pub id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub title: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub status: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub priority: String,
    #[serde(rename = "type", default, deserialize_with = "null_as_default")]
    pub issue_type: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub labels: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub sprint: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub acceptance: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub created_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub updated_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub closed_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub defer_date: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub due_date: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub parent_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub children: Vec<Task>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub depends_on: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub blocked_by: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub comments: Vec<Comment>,
    #[serde(rename = "logs", default, deserialize_with = "null_as_default")]
    pub activity: Vec<ActivityEntry>,
}

#[allow(dead_code)] // fields used by askama templates
#[derive(Debug, Deserialize)]
pub struct Comment {
    #[serde(default, deserialize_with = "null_as_default")]
    pub author: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub body: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub created: String,
}

#[allow(dead_code)] // fields used by askama templates
#[derive(Debug, Deserialize)]
pub struct ActivityEntry {
    #[serde(rename = "type", default, deserialize_with = "null_as_default")]
    pub action: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub timestamp: String,
    #[serde(rename = "message", default, deserialize_with = "null_as_default")]
    pub details: String,
}

/// Options for filtering and sorting `td list` output.
pub struct ListOpts {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub task_type: Option<String>,
    pub query: Option<String>,
    pub sort: Option<String>,
    pub all: bool,
    pub limit: Option<usize>,
    pub reverse: bool,
}

impl Default for ListOpts {
    fn default() -> Self {
        Self {
            status: None,
            priority: None,
            task_type: None,
            query: None,
            sort: Some("priority".to_string()),
            all: false,
            limit: None,
            reverse: false,
        }
    }
}

pub struct Td<'a> {
    project_root: &'a std::path::Path,
}

impl<'a> Td<'a> {
    pub fn new(project_root: &'a std::path::Path) -> Self {
        Self { project_root }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::new("td");
        cmd.arg("-w").arg(self.project_root);
        cmd
    }

    /// Command that identifies as the nocturnal implementer session.
    /// Used for start/review calls that nocturnal makes on behalf of Claude.
    fn cmd_implementer(&self) -> Command {
        let mut cmd = self.cmd();
        cmd.env("TD_SESSION_ID", "nocturnal-implementer");
        cmd
    }

    /// Command that identifies as the nocturnal reviewer session.
    /// Used for approve/reject calls — distinct from implementer to avoid self-approval.
    fn cmd_reviewer(&self) -> Command {
        let mut cmd = self.cmd();
        cmd.env("TD_SESSION_ID", "nocturnal-reviewer");
        cmd
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let output = self.cmd().args(args).output().context("Failed to run td")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("td {} failed: {}", args.join(" "), stderr.trim());
        }
        String::from_utf8(output.stdout).context("td output was not valid UTF-8")
    }

    fn run_as_reviewer(&self, args: &[&str]) -> Result<String> {
        let output = self
            .cmd_reviewer()
            .args(args)
            .output()
            .context("Failed to run td")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("td {} failed: {}", args.join(" "), stderr.trim());
        }
        String::from_utf8(output.stdout).context("td output was not valid UTF-8")
    }

    fn run_quiet(&self, args: &[&str]) {
        if let Err(e) = self.run(args) {
            debug!("td {} (ignored): {e:#}", args.join(" "));
        }
    }

    pub fn show(&self, task_id: &str) -> Result<Task> {
        let json = self.run(&["show", task_id, "--json"])?;
        serde_json::from_str(&json).context("Failed to parse task JSON")
    }

    pub fn show_detail(&self, issue_id: &str) -> Result<IssueDetail> {
        let json = self.run(&["show", issue_id, "--json"])?;
        serde_json::from_str(&json).context("Failed to parse issue detail JSON")
    }

    pub fn list(&self, opts: &ListOpts) -> Result<Vec<Task>> {
        let mut args = vec!["list", "--json"];

        if opts.all {
            args.push("--all");
        }

        let status_val;
        if let Some(ref s) = opts.status {
            if s != "all" {
                status_val = s.clone();
                args.push("--status");
                args.push(&status_val);
            }
        }

        let priority_val;
        if let Some(ref p) = opts.priority {
            if p != "all" {
                priority_val = p.clone();
                args.push("--priority");
                args.push(&priority_val);
            }
        }

        let type_val;
        if let Some(ref t) = opts.task_type {
            if t != "all" {
                type_val = t.clone();
                args.push("--type");
                args.push(&type_val);
            }
        }

        let query_val;
        if let Some(ref q) = opts.query {
            query_val = q.clone();
            args.push("-q");
            args.push(&query_val);
        }

        let sort_val;
        if let Some(ref s) = opts.sort {
            sort_val = s.clone();
            args.push("--sort");
            args.push(&sort_val);
        }

        if opts.reverse {
            args.push("--reverse");
        }

        let limit_val;
        if let Some(n) = opts.limit {
            limit_val = n.to_string();
            args.push("--limit");
            args.push(&limit_val);
        }

        let json = self.run(&args)?;
        let tasks: Vec<Task> =
            serde_json::from_str::<Option<Vec<Task>>>(&json)?.unwrap_or_default();
        Ok(tasks)
    }

    pub fn depends_on(&self, issue_id: &str) -> Vec<String> {
        let Ok(json) = self.run(&["depends-on", issue_id, "--json"]) else {
            return vec![];
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
            return vec![];
        };
        value["dependencies"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn blocked_by(&self, issue_id: &str) -> Vec<String> {
        let Ok(json) = self.run(&["blocked-by", issue_id, "--json"]) else {
            return vec![];
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
            return vec![];
        };
        value["direct"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v["id"].as_str().map(std::string::ToString::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn list_by_status(&self, status: &str) -> Result<Vec<Task>> {
        let json = self.run(&["list", "--json", "--status", status])?;
        let tasks: Vec<Task> = serde_json::from_str::<Option<Vec<Task>>>(&json)
            .context("failed to parse td list output")?
            .unwrap_or_default();
        Ok(tasks
            .into_iter()
            .filter(|t| validate_task_id(&t.id).is_ok())
            .collect())
    }

    pub fn get_next_task_id(&self) -> Result<Option<String>> {
        let output = self
            .cmd()
            .args(["next"])
            .output()
            .context("Failed to run td next (is td installed?)")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_trimmed = stderr.trim();
            // td next exits non-zero with empty stderr when there are no open tasks
            if stderr_trimmed.is_empty() {
                return Ok(None);
            }
            bail!("td next failed: {stderr_trimmed}");
        }

        let stdout = String::from_utf8(output.stdout).context("td output was not valid UTF-8")?;
        let Some(candidate) = stdout.split_whitespace().next() else {
            return Ok(None);
        };
        validate_task_id(candidate)?;
        // Confirm the candidate is a real task ID by fetching it. td next exits 0
        // with a human-readable message (e.g. "No open issues") when there are no
        // tasks; the first token of that message passes validate_task_id but is not
        // a valid ID.
        if self.show(candidate).is_err() {
            return Ok(None);
        }
        Ok(Some(candidate.to_string()))
    }

    pub fn get_reviewable_task_id(&self) -> Result<Option<String>> {
        let tasks = self.list_by_status("in_review")?;
        Ok(tasks
            .into_iter()
            .find(|t| !t.labels.iter().any(|l| l.starts_with("noc-proposal")))
            .map(|t| t.id))
    }

    pub fn get_proposal_task_ids(&self) -> Result<Vec<String>> {
        let tasks = self.list_by_status("in_review")?;
        Ok(tasks
            .into_iter()
            .filter(|t| t.labels.iter().any(|l| l.starts_with("noc-proposal:")))
            .map(|t| t.id)
            .collect())
    }

    /// Determine the next action the orchestrator would take.
    /// `check_proposals` should be true when VCS integration is active.
    pub fn get_next_action(&self, check_proposals: bool) -> Result<NextAction> {
        if check_proposals {
            let proposals = self.get_proposal_task_ids()?;
            if !proposals.is_empty() {
                return Ok(NextAction::ProposalReview(proposals));
            }
        }

        if let Some(task_id) = self.get_reviewable_task_id()? {
            return Ok(NextAction::Review(task_id));
        }

        if let Some(task_id) = self.get_next_task_id()? {
            return Ok(NextAction::Implement(task_id));
        }

        Ok(NextAction::Idle)
    }

    pub fn start(&self, task_id: &str) -> Result<()> {
        let output = self
            .cmd_implementer()
            .args(["start", task_id])
            .output()
            .context("Failed to run td start")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("td start {} failed: {}", task_id, stderr.trim());
        }
        debug!(
            "td start {}: {}",
            task_id,
            String::from_utf8_lossy(&output.stdout).trim()
        );
        Ok(())
    }

    pub fn review(&self, task_id: &str) -> Result<()> {
        let output = self
            .cmd_implementer()
            .args(["review", task_id])
            .output()
            .context("Failed to run td review")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("td review {} failed: {}", task_id, stderr.trim());
        }
        debug!(
            "td review {}: {}",
            task_id,
            String::from_utf8_lossy(&output.stdout).trim()
        );
        Ok(())
    }

    pub fn approve(&self, task_id: &str) -> Result<()> {
        let output = self
            .cmd_reviewer()
            .args(["approve", task_id])
            .output()
            .context("Failed to run td approve")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            bail!("td approve {} failed: {}", task_id, stderr.trim());
        }
        let out = stdout.trim();
        let out_lower = out.to_ascii_lowercase();
        if out_lower.starts_with("error:") || out_lower.starts_with("warning:") {
            bail!("td approve {task_id} failed: {out}");
        }
        Ok(())
    }

    pub fn reject(&self, task_id: &str, reason: &str) -> Result<()> {
        self.run_as_reviewer(&["reject", task_id, "--reason", reason])
            .map(|_| ())
    }

    pub fn block(&self, task_id: &str) -> Result<()> {
        self.run(&["block", task_id]).map(|_| ())
    }

    #[allow(clippy::unnecessary_wraps)]
    pub fn comment(&self, task_id: &str, message: &str) -> Result<()> {
        self.run_quiet(&["comment", task_id, message]);
        Ok(())
    }

    #[allow(clippy::unnecessary_wraps)]
    pub fn log(&self, message: &str) -> Result<()> {
        self.run_quiet(&["log", message]);
        Ok(())
    }

    pub fn update_labels(&self, task_id: &str, labels: &str) -> Result<()> {
        self.run(&["update", task_id, "--labels", labels])?;
        Ok(())
    }

    pub fn update_priority(&self, task_id: &str, priority: &str) -> Result<()> {
        self.run(&["update", task_id, "--priority", priority])?;
        Ok(())
    }

    pub fn handoff(&self, task_id: &str, done: &str, remaining: &str) -> Result<()> {
        self.run(&["handoff", task_id, "--done", done, "--remaining", remaining])
            .map(|_| ())
    }
}

pub enum NextAction {
    ProposalReview(Vec<String>),
    Review(String),
    Implement(String),
    Idle,
}

impl NextAction {
    pub fn task_id(&self) -> Option<&str> {
        match self {
            NextAction::ProposalReview(ids) => ids.first().map(String::as_str),
            NextAction::Review(id) | NextAction::Implement(id) => Some(id),
            NextAction::Idle => None,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            NextAction::ProposalReview(_) => "Proposal Review",
            NextAction::Review(_) => "Review",
            NextAction::Implement(_) => "Implement",
            NextAction::Idle => "Idle",
        }
    }
}

// --- Label helpers ---

pub fn get_review_count(task: &Task) -> u32 {
    task.labels
        .iter()
        .find_map(|l| l.strip_prefix("noc-reviews:"))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

pub fn build_labels_with_review_count(task: &Task, count: u32) -> String {
    let mut labels: Vec<String> = task
        .labels
        .iter()
        .filter(|l| !l.starts_with("noc-reviews:"))
        .cloned()
        .collect();
    labels.push(format!("noc-reviews:{count}"));
    labels.join(",")
}

pub fn swap_label(task: &Task, remove_prefix: &str, add_label: Option<&str>) -> String {
    swap_labels(task, &[remove_prefix], add_label)
}

pub fn swap_labels(task: &Task, remove_prefixes: &[&str], add_label: Option<&str>) -> String {
    let mut labels: Vec<String> = task
        .labels
        .iter()
        .filter(|l| !remove_prefixes.iter().any(|p| l.starts_with(p)))
        .cloned()
        .collect();
    if let Some(add) = add_label {
        labels.push(add.to_string());
    }
    labels.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_task_id ---

    #[test]
    fn validate_task_id_accepts_valid_ids() {
        assert!(validate_task_id("td-24a7b3").is_ok());
        assert!(validate_task_id("task_1").is_ok());
        assert!(validate_task_id("ABC123").is_ok());
        assert!(validate_task_id("a").is_ok());
    }

    #[test]
    fn validate_task_id_rejects_empty() {
        assert!(validate_task_id("").is_err());
    }

    #[test]
    fn validate_task_id_rejects_path_traversal() {
        assert!(validate_task_id("../etc/passwd").is_err());
        assert!(validate_task_id("foo/bar").is_err());
    }

    #[test]
    fn validate_task_id_rejects_spaces_and_special_chars() {
        assert!(validate_task_id("task 1").is_err());
        assert!(validate_task_id("task\n1").is_err());
        assert!(validate_task_id("task;rm").is_err());
    }

    fn task_with_labels(labels: &[&str]) -> Task {
        Task {
            id: "t1".to_string(),
            title: String::new(),
            description: String::new(),
            status: String::new(),
            labels: labels.iter().map(|s| (*s).to_string()).collect(),
            priority: String::new(),
            task_type: String::new(),
            sprint: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            closed_at: String::new(),
            parent_id: String::new(),
        }
    }

    // --- get_review_count ---

    #[test]
    fn get_review_count_returns_zero_when_no_label() {
        let task = task_with_labels(&["bug", "urgent"]);
        assert_eq!(get_review_count(&task), 0);
    }

    #[test]
    fn get_review_count_returns_zero_for_empty_labels() {
        let task = task_with_labels(&[]);
        assert_eq!(get_review_count(&task), 0);
    }

    #[test]
    fn get_review_count_parses_label() {
        let task = task_with_labels(&["noc-reviews:2", "bug"]);
        assert_eq!(get_review_count(&task), 2);
    }

    #[test]
    fn get_review_count_returns_zero_for_malformed_value() {
        let task = task_with_labels(&["noc-reviews:abc"]);
        assert_eq!(get_review_count(&task), 0);
    }

    #[test]
    fn get_review_count_returns_zero_for_empty_value() {
        let task = task_with_labels(&["noc-reviews:"]);
        assert_eq!(get_review_count(&task), 0);
    }

    // --- build_labels_with_review_count ---

    #[test]
    fn build_labels_replaces_existing_review_count() {
        let task = task_with_labels(&["bug", "noc-reviews:1"]);
        assert_eq!(
            build_labels_with_review_count(&task, 2),
            "bug,noc-reviews:2"
        );
    }

    #[test]
    fn build_labels_adds_count_when_missing() {
        let task = task_with_labels(&["bug"]);
        assert_eq!(
            build_labels_with_review_count(&task, 1),
            "bug,noc-reviews:1"
        );
    }

    #[test]
    fn build_labels_with_empty_labels() {
        let task = task_with_labels(&[]);
        assert_eq!(build_labels_with_review_count(&task, 0), "noc-reviews:0");
    }

    // --- swap_label ---

    #[test]
    fn swap_label_removes_prefix_and_adds_new() {
        let task = task_with_labels(&["noc-proposal:42", "bug"]);
        assert_eq!(
            swap_label(&task, "noc-proposal", Some("noc-done")),
            "bug,noc-done"
        );
    }

    #[test]
    fn swap_label_removes_without_adding() {
        let task = task_with_labels(&["noc-proposal:42", "bug"]);
        assert_eq!(swap_label(&task, "noc-proposal", None), "bug");
    }

    #[test]
    fn swap_label_no_match_keeps_all_and_adds() {
        let task = task_with_labels(&["bug", "urgent"]);
        assert_eq!(
            swap_label(&task, "noc-proposal", Some("new")),
            "bug,urgent,new"
        );
    }

    // --- swap_labels (multiple prefixes) ---

    #[test]
    fn swap_labels_removes_multiple_prefixes() {
        let task = task_with_labels(&["noc-proposal:42", "noc-proposal-ready", "bug"]);
        assert_eq!(
            swap_labels(
                &task,
                &["noc-proposal:", "noc-proposal-ready"],
                Some("done")
            ),
            "bug,done"
        );
    }

    // --- handoff ---

    #[test]
    #[cfg(unix)]
    fn handoff_sends_correct_cli_args() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let _guard = PATH_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let args_file = tmp.path().join("args.txt");
        let args_path = args_file.display().to_string();

        // Script writes its positional args (after stripping -w <path>) to a file.
        let script = format!(
            "#!/bin/sh\nwhile [ \"$1\" = \"-w\" ]; do shift 2; done\nprintf '%s\\n' \"$@\" > '{args_path}'\nexit 0\n",
            args_path = args_path,
        );
        let script_path = tmp.path().join("td");
        let mut f = std::fs::File::create(&script_path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();

        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let old_path = std::env::var("PATH").unwrap_or_default();
        // SAFETY: PATH_MUTEX ensures no concurrent PATH mutations in this process.
        unsafe {
            std::env::set_var("PATH", format!("{}:{}", tmp.path().display(), old_path));
        }

        let td = Td::new(&project_root);
        let result = td.handoff("td-abc123", "implementation attempted", "build failed");

        // SAFETY: restoring PATH to previous value.
        unsafe {
            std::env::set_var("PATH", old_path);
        }

        assert!(result.is_ok());
        let recorded = std::fs::read_to_string(&args_file).unwrap();
        let args: Vec<&str> = recorded.lines().collect();
        assert_eq!(
            args,
            vec![
                "handoff",
                "td-abc123",
                "--done",
                "implementation attempted",
                "--remaining",
                "build failed",
            ]
        );
    }

    // --- get_next_task_id ---

    // Serialise tests that temporarily mutate PATH so they don't race.
    #[cfg(unix)]
    static PATH_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Write a minimal fake `td` shell script and return the temp directory.
    /// The script responds to `next` and `show`:
    /// - `next` prints `next_output` and exits 0
    /// - `show <id> --json` exits 0 with a stub task JSON only if `id == valid_id`,
    ///   otherwise exits 1
    #[cfg(unix)]
    fn make_fake_td(tmp: &std::path::Path, next_output: &str, valid_id: &str) {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let script_path = tmp.join("td");
        let next_out = next_output.to_string();
        let vid = valid_id.to_string();
        // Td::cmd() prepends "-w <path>" before the subcommand, so the script
        // must skip those flags to find the actual subcommand and its arguments.
        let script = format!(
            r#"#!/bin/sh
# skip -w <path> pairs
while [ "$1" = "-w" ]; do shift 2; done
case "$1" in
  next) printf '%s\n' '{next_out}'; exit 0 ;;
  show)
    if [ "$2" = '{vid}' ]; then
      printf '{{"id":"{vid}","title":"","description":"","status":"open","labels":[]}}\n'
      exit 0
    else
      echo "issue not found" >&2; exit 1
    fi
    ;;
  *) exit 1 ;;
esac
"#,
            next_out = next_out,
            vid = vid,
        );
        let mut f = std::fs::File::create(&script_path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn get_next_task_id_returns_none_for_no_open_issues_message() {
        let _guard = PATH_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        make_fake_td(tmp.path(), "No open issues", "td-000000");
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let old_path = std::env::var("PATH").unwrap_or_default();
        // SAFETY: PATH_MUTEX ensures no concurrent PATH mutations in this process.
        unsafe {
            std::env::set_var("PATH", format!("{}:{}", tmp.path().display(), old_path));
        }
        let td = Td::new(&project_root);
        let result = td.get_next_task_id();
        // SAFETY: restoring PATH to previous value.
        unsafe {
            std::env::set_var("PATH", old_path);
        }
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    #[cfg(unix)]
    fn get_next_task_id_returns_id_for_valid_task() {
        let _guard = PATH_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        make_fake_td(tmp.path(), "td-abc123 Fix something", "td-abc123");
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let old_path = std::env::var("PATH").unwrap_or_default();
        // SAFETY: PATH_MUTEX ensures no concurrent PATH mutations in this process.
        unsafe {
            std::env::set_var("PATH", format!("{}:{}", tmp.path().display(), old_path));
        }
        let td = Td::new(&project_root);
        let result = td.get_next_task_id();
        // SAFETY: restoring PATH to previous value.
        unsafe {
            std::env::set_var("PATH", old_path);
        }
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("td-abc123".to_string()));
    }

    // --- Task deserialization ---

    #[test]
    fn task_deserializes_with_null_fields() {
        let json =
            r#"{"id": "t1", "title": null, "description": null, "status": null, "labels": null}"#;
        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(task.id, "t1");
        assert_eq!(task.title, "");
        assert_eq!(task.description, "");
        assert_eq!(task.status, "");
        assert!(task.labels.is_empty());
    }

    #[test]
    fn task_deserializes_with_missing_fields() {
        let json = r#"{"id": "t1"}"#;
        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(task.id, "t1");
        assert!(task.labels.is_empty());
    }

    #[test]
    fn task_deserializes_with_full_fields() {
        let json = r#"{"id": "t1", "title": "Fix bug", "description": "desc", "status": "open", "labels": ["a", "b"]}"#;
        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(task.title, "Fix bug");
        assert_eq!(task.labels, vec!["a", "b"]);
    }
}
