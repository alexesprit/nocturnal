use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

struct ToolCheck {
    name: &'static str,
    required: bool,
}

const CORE_TOOLS: &[ToolCheck] = &[
    ToolCheck {
        name: "td",
        required: true,
    },
    ToolCheck {
        name: "git",
        required: true,
    },
    ToolCheck {
        name: "claude",
        required: true,
    },
    ToolCheck {
        name: "git-gtr",
        required: false,
    },
];

pub fn run(project_root: &Path, dry_run: bool) -> Result<()> {
    println!("Initializing nocturnal in: {}", project_root.display());
    println!();

    // 1. Check required tools
    let all_ok = check_tools(project_root)?;
    println!();
    if !all_ok {
        anyhow::bail!("One or more required tools are missing. Install them and re-run.");
    }

    // 2. Run td init if needed
    init_td(project_root, dry_run)?;

    // 3. Create .nocturnal.toml if not present
    create_toml(project_root, dry_run)?;

    // 4. Create .nocturnal/ prompt extras directory
    create_prompt_extras_dir(project_root, dry_run)?;

    // 5. Print summary
    println!();
    println!("Done. Next steps:");
    println!("  - Edit .nocturnal.toml to configure VCS mode, model, and other options");
    println!("  - Add tasks with: td add \"Task description\"");
    println!("  - Run: nocturnal implement");

    Ok(())
}

fn tool_on_path(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn check_tools(project_root: &Path) -> Result<bool> {
    println!("Checking required tools:");

    // Detect which VCS tools to check based on git remote
    let remote_url = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(project_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let need_gh = remote_url.as_deref().is_some_and(|u| u.contains("github"));
    let need_glab = remote_url.as_deref().is_some_and(|u| u.contains("gitlab"));

    let mut all_required_ok = true;

    for tool in CORE_TOOLS {
        let found = tool_on_path(tool.name);
        let status = if found { "found" } else { "MISSING" };
        let required_label = if tool.required {
            " (required)"
        } else {
            " (optional)"
        };
        println!("  {}: {}{}", tool.name, status, required_label);
        if !found && tool.required {
            all_required_ok = false;
        }
    }

    // VCS-specific tools
    if need_gh {
        let found = tool_on_path("gh");
        let status = if found { "found" } else { "MISSING" };
        println!("  gh: {} (required for GitHub VCS mode)", status);
        if !found {
            all_required_ok = false;
        }
    }

    if need_glab {
        let found = tool_on_path("glab");
        let status = if found { "found" } else { "MISSING" };
        println!("  glab: {} (required for GitLab VCS mode)", status);
        if !found {
            all_required_ok = false;
        }
    }

    Ok(all_required_ok)
}

fn init_td(project_root: &Path, dry_run: bool) -> Result<()> {
    let todos_dir = project_root.join(".todos");
    if todos_dir.is_dir() {
        println!("td: already initialized, skipping");
        return Ok(());
    }

    if dry_run {
        println!("td: would run `td init` (dry-run)");
        return Ok(());
    }

    println!("td: running `td init`...");
    let status = Command::new("td")
        .arg("init")
        .current_dir(project_root)
        .status()?;

    if !status.success() {
        anyhow::bail!("`td init` failed with exit code: {status}");
    }
    println!("td: initialized");

    Ok(())
}

fn detect_vcs_mode(project_root: &Path) -> &'static str {
    let url = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(project_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    match url.as_deref() {
        Some(u) if u.contains("gitlab") => "gitlab",
        Some(u) if u.contains("github") => "github",
        _ => "off",
    }
}

fn detect_default_branch(project_root: &Path) -> String {
    // Try git symbolic-ref refs/remotes/origin/HEAD
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .current_dir(project_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    if let Some(ref_name) = output {
        // Strips "origin/" prefix
        if let Some(branch) = ref_name.strip_prefix("origin/") {
            return branch.to_string();
        }
        return ref_name;
    }

    "main".to_string()
}

fn create_toml(project_root: &Path, dry_run: bool) -> Result<()> {
    let toml_path = project_root.join(".nocturnal.toml");

    if toml_path.exists() {
        println!(".nocturnal.toml: already exists, skipping");
        return Ok(());
    }

    let vcs_mode = detect_vcs_mode(project_root);
    let base_branch = detect_default_branch(project_root);

    let content = format!(
        r#"# nocturnal per-project configuration
# Generated by `nocturnal init`. Edit as needed.

# [vcs]
# mode = "{vcs_mode}"       # auto-detected from git remote
# base_branch = "{base_branch}"  # branch worktrees are created from
# target_branch = "{base_branch}" # branch PRs/MRs target
# auto_merge = true          # enable auto-merge on proposals
# delete_branch_on_merge = false

# [claude]
# model = "sonnet"           # default model for all operations
# implement_model = "sonnet" # override for implement/develop
# review_model = "sonnet"    # override for review

# max_reviews = 3            # max review cycles before blocking
# max_budget = 5             # max USD per Claude run

[vcs]
mode = "{vcs_mode}"
base_branch = "{base_branch}"
"#
    );

    if dry_run {
        println!(".nocturnal.toml: would create (dry-run)");
        println!("  vcs.mode = {vcs_mode}");
        println!("  vcs.base_branch = {base_branch}");
        return Ok(());
    }

    fs::write(&toml_path, content)?;
    println!(".nocturnal.toml: created (vcs.mode={vcs_mode}, base_branch={base_branch})");

    Ok(())
}

fn create_prompt_extras_dir(project_root: &Path, dry_run: bool) -> Result<()> {
    let dir = project_root.join(".nocturnal");

    if !dir.exists() {
        if dry_run {
            println!(".nocturnal/: would create directory (dry-run)");
        } else {
            fs::create_dir_all(&dir)?;
            println!(".nocturnal/: created directory");
        }
    } else {
        println!(".nocturnal/: already exists, skipping");
    }

    let placeholders = [
        (
            "prompt-extra.md",
            "<!-- Appended to ALL nocturnal prompt templates. Add project-specific context here. -->\n",
        ),
        (
            "prompt-implement.md",
            "<!-- Appended to the implement prompt template only. -->\n",
        ),
        (
            "prompt-review.md",
            "<!-- Appended to the review prompt template only. -->\n",
        ),
    ];

    for (name, content) in &placeholders {
        let file_path = dir.join(name);
        if file_path.exists() {
            println!(".nocturnal/{name}: already exists, skipping");
        } else if dry_run {
            println!(".nocturnal/{name}: would create (dry-run)");
        } else {
            fs::write(&file_path, content)?;
            println!(".nocturnal/{name}: created");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn create_toml_writes_file() {
        let dir = make_git_repo();
        create_toml(dir.path(), false).unwrap();
        let toml_path = dir.path().join(".nocturnal.toml");
        assert!(toml_path.exists());
        let content = fs::read_to_string(&toml_path).unwrap();
        assert!(content.contains("[vcs]"));
        assert!(content.contains("mode ="));
    }

    #[test]
    fn create_toml_idempotent() {
        let dir = make_git_repo();
        create_toml(dir.path(), false).unwrap();
        let first = fs::read_to_string(dir.path().join(".nocturnal.toml")).unwrap();
        create_toml(dir.path(), false).unwrap();
        let second = fs::read_to_string(dir.path().join(".nocturnal.toml")).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn create_toml_dry_run_does_not_create() {
        let dir = make_git_repo();
        create_toml(dir.path(), true).unwrap();
        assert!(!dir.path().join(".nocturnal.toml").exists());
    }

    #[test]
    fn create_toml_no_overwrite_existing() {
        let dir = make_git_repo();
        let toml_path = dir.path().join(".nocturnal.toml");
        fs::write(&toml_path, "existing = true\n").unwrap();
        create_toml(dir.path(), false).unwrap();
        let content = fs::read_to_string(&toml_path).unwrap();
        assert_eq!(content, "existing = true\n");
    }

    #[test]
    fn create_prompt_extras_dir_creates_files() {
        let dir = TempDir::new().unwrap();
        create_prompt_extras_dir(dir.path(), false).unwrap();
        assert!(dir.path().join(".nocturnal").is_dir());
        assert!(dir.path().join(".nocturnal/prompt-extra.md").exists());
        assert!(dir.path().join(".nocturnal/prompt-implement.md").exists());
        assert!(dir.path().join(".nocturnal/prompt-review.md").exists());
    }

    #[test]
    fn create_prompt_extras_dir_idempotent() {
        let dir = TempDir::new().unwrap();
        create_prompt_extras_dir(dir.path(), false).unwrap();
        create_prompt_extras_dir(dir.path(), false).unwrap();
        // No error, files still exist
        assert!(dir.path().join(".nocturnal/prompt-extra.md").exists());
    }

    #[test]
    fn create_prompt_extras_dir_dry_run() {
        let dir = TempDir::new().unwrap();
        create_prompt_extras_dir(dir.path(), true).unwrap();
        assert!(!dir.path().join(".nocturnal").exists());
    }

    #[test]
    fn create_prompt_extras_preserves_existing_files() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".nocturnal")).unwrap();
        let extra = dir.path().join(".nocturnal/prompt-extra.md");
        fs::write(&extra, "custom content\n").unwrap();
        create_prompt_extras_dir(dir.path(), false).unwrap();
        let content = fs::read_to_string(&extra).unwrap();
        assert_eq!(content, "custom content\n");
    }

    #[test]
    fn detect_vcs_mode_no_remote_returns_off() {
        let dir = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let mode = detect_vcs_mode(dir.path());
        assert_eq!(mode, "off");
    }

    #[test]
    fn detect_default_branch_fallback() {
        let dir = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let branch = detect_default_branch(dir.path());
        assert_eq!(branch, "main");
    }
}
