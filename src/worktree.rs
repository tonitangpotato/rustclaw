//! Git Worktree Lifecycle Manager.
//!
//! Provides isolated git worktrees for specialist agents.
//! Each agent works in its own branch without affecting others.
//!
//! This implements DESIGN.md Key Decision #3:
//! - Each agent gets its own git worktree (branch)
//! - CEO agent (main) spawns specialists
//! - Specialists work on their own branch
//! - CEO merges results back to main

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use tokio::process::Command;

/// Manages git worktrees for specialist agents.
pub struct WorktreeManager {
    /// Root directory of the git repository.
    repo_root: PathBuf,
    /// Directory where worktrees are stored (e.g., .rustclaw/worktrees/).
    worktrees_dir: PathBuf,
}

/// Information about a git worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Worktree name (used as directory name and branch suffix).
    pub name: String,
    /// Full path to the worktree directory.
    pub path: PathBuf,
    /// Branch name (e.g., rustclaw/<name>).
    pub branch: String,
    /// When the worktree was created.
    pub created_at: DateTime<Utc>,
    /// Agent ID currently using this worktree (if any).
    pub agent_id: Option<String>,
}

/// Result of a merge operation.
#[derive(Debug, Clone)]
pub enum MergeResult {
    /// Merge succeeded.
    Success {
        /// The resulting commit hash.
        commit_hash: String,
    },
    /// Merge has conflicts.
    Conflict {
        /// Files with conflicts.
        conflicting_files: Vec<String>,
    },
    /// No changes to merge.
    NothingToMerge,
}

impl WorktreeManager {
    /// Create a new worktree manager.
    ///
    /// # Arguments
    /// * `repo_root` - Root directory of the git repository
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        let repo_root = repo_root.into();
        let worktrees_dir = repo_root.join(".rustclaw").join("worktrees");
        Self {
            repo_root,
            worktrees_dir,
        }
    }

    /// Create a new worktree for a specialist agent.
    ///
    /// Creates: `git worktree add <worktrees_dir>/<name> -b rustclaw/<name> [base_branch]`
    ///
    /// # Arguments
    /// * `name` - Unique name for the worktree (used as directory and branch suffix)
    /// * `base_branch` - Optional base branch to create from (defaults to HEAD)
    pub async fn create(
        &self,
        name: &str,
        base_branch: Option<&str>,
    ) -> anyhow::Result<WorktreeInfo> {
        // Ensure worktrees directory exists
        tokio::fs::create_dir_all(&self.worktrees_dir).await?;

        let worktree_path = self.worktrees_dir.join(name);
        let branch_name = format!("rustclaw/{}", name);

        // Build git worktree add command
        let mut args = vec![
            "worktree".to_string(),
            "add".to_string(),
            worktree_path.display().to_string(),
            "-b".to_string(),
            branch_name.clone(),
        ];

        if let Some(base) = base_branch {
            args.push(base.to_string());
        }

        let output = Command::new("git")
            .args(&args)
            .current_dir(&self.repo_root)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to create worktree: {}", stderr);
        }

        tracing::info!(
            "Created worktree '{}' at {} on branch {}",
            name,
            worktree_path.display(),
            branch_name
        );

        Ok(WorktreeInfo {
            name: name.to_string(),
            path: worktree_path,
            branch: branch_name,
            created_at: Utc::now(),
            agent_id: None,
        })
    }

    /// Remove a worktree and optionally delete its branch.
    ///
    /// Runs: `git worktree remove <path>` followed by `git branch -D rustclaw/<name>`
    pub async fn remove(&self, name: &str) -> anyhow::Result<()> {
        let worktree_path = self.worktrees_dir.join(name);
        let branch_name = format!("rustclaw/{}", name);

        // Remove the worktree
        let output = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .current_dir(&self.repo_root)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If worktree doesn't exist, that's fine
            if !stderr.contains("is not a working tree") {
                anyhow::bail!("Failed to remove worktree: {}", stderr);
            }
        }

        // Delete the branch
        let output = Command::new("git")
            .args(["branch", "-D", &branch_name])
            .current_dir(&self.repo_root)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If branch doesn't exist, that's fine
            if !stderr.contains("not found") {
                tracing::warn!("Failed to delete branch {}: {}", branch_name, stderr);
            }
        }

        tracing::info!("Removed worktree '{}'", name);
        Ok(())
    }

    /// List all worktrees.
    ///
    /// Runs: `git worktree list --porcelain`
    pub async fn list(&self) -> anyhow::Result<Vec<WorktreeInfo>> {
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(&self.repo_root)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to list worktrees: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let worktrees = self.parse_worktree_list(&stdout).await?;

        Ok(worktrees)
    }

    /// Parse `git worktree list --porcelain` output.
    async fn parse_worktree_list(&self, output: &str) -> anyhow::Result<Vec<WorktreeInfo>> {
        let mut worktrees = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_branch: Option<String> = None;

        for line in output.lines() {
            if line.starts_with("worktree ") {
                // Save previous worktree if any
                if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                    if let Some(info) = self.worktree_info_from_path(&path, &branch).await {
                        worktrees.push(info);
                    }
                }
                current_path = Some(PathBuf::from(line.strip_prefix("worktree ").unwrap()));
            } else if line.starts_with("branch ") {
                current_branch = Some(line.strip_prefix("branch refs/heads/").unwrap_or(line.strip_prefix("branch ").unwrap_or("")).to_string());
            }
        }

        // Don't forget the last one
        if let (Some(path), Some(branch)) = (current_path, current_branch) {
            if let Some(info) = self.worktree_info_from_path(&path, &branch).await {
                worktrees.push(info);
            }
        }

        // Filter to only rustclaw worktrees
        worktrees.retain(|w| w.branch.starts_with("rustclaw/"));

        Ok(worktrees)
    }

    /// Create WorktreeInfo from path and branch.
    async fn worktree_info_from_path(
        &self,
        path: &Path,
        branch: &str,
    ) -> Option<WorktreeInfo> {
        // Extract name from path (last component)
        let name = path.file_name()?.to_str()?.to_string();

        // Get creation time from directory metadata
        let created_at = tokio::fs::metadata(path)
            .await
            .ok()
            .and_then(|m| m.created().ok())
            .map(|t| DateTime::<Utc>::from(t))
            .unwrap_or_else(Utc::now);

        Some(WorktreeInfo {
            name,
            path: path.to_path_buf(),
            branch: branch.to_string(),
            created_at,
            agent_id: None,
        })
    }

    /// Merge a specialist's branch back to the target branch.
    ///
    /// Steps:
    /// 1. `git checkout <target_branch>` (in repo_root)
    /// 2. `git merge rustclaw/<name> --no-ff -m "merge: ..."`
    /// 3. Handle conflicts if any
    pub async fn merge_back(
        &self,
        name: &str,
        target_branch: &str,
    ) -> anyhow::Result<MergeResult> {
        let branch_name = format!("rustclaw/{}", name);

        // Check if there are any commits to merge
        let output = Command::new("git")
            .args(["rev-list", "--count", &format!("{}..{}", target_branch, branch_name)])
            .current_dir(&self.repo_root)
            .output()
            .await?;

        let count: i32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        if count == 0 {
            return Ok(MergeResult::NothingToMerge);
        }

        // Checkout target branch
        let output = Command::new("git")
            .args(["checkout", target_branch])
            .current_dir(&self.repo_root)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to checkout {}: {}", target_branch, stderr);
        }

        // Attempt merge
        let merge_message = format!("merge: {} work from specialist agent", name);
        let output = Command::new("git")
            .args(["merge", &branch_name, "--no-ff", "-m", &merge_message])
            .current_dir(&self.repo_root)
            .output()
            .await?;

        if output.status.success() {
            // Get the merge commit hash
            let hash_output = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&self.repo_root)
                .output()
                .await?;

            let commit_hash = String::from_utf8_lossy(&hash_output.stdout)
                .trim()
                .to_string();

            tracing::info!(
                "Merged branch {} into {}: {}",
                branch_name,
                target_branch,
                commit_hash
            );

            return Ok(MergeResult::Success { commit_hash });
        }

        // Check for conflicts
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.repo_root)
            .output()
            .await?;

        let status = String::from_utf8_lossy(&status_output.stdout);
        let conflicting_files: Vec<String> = status
            .lines()
            .filter(|line| line.starts_with("UU") || line.starts_with("AA") || line.starts_with("DD"))
            .map(|line| line[3..].to_string())
            .collect();

        if !conflicting_files.is_empty() {
            // Abort the merge
            let _ = Command::new("git")
                .args(["merge", "--abort"])
                .current_dir(&self.repo_root)
                .output()
                .await;

            tracing::warn!(
                "Merge conflict for branch {}: {:?}",
                branch_name,
                conflicting_files
            );

            return Ok(MergeResult::Conflict { conflicting_files });
        }

        // Some other merge failure
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Merge failed: {}", stderr);
    }

    /// Commit all changes in a worktree.
    ///
    /// Runs: `git add -A && git commit -m "..."` in the worktree directory.
    ///
    /// Returns the commit hash.
    pub async fn commit_work(&self, name: &str, message: &str) -> anyhow::Result<String> {
        let worktree_path = self.worktrees_dir.join(name);

        // Stage all changes
        let output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(&worktree_path)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to stage changes: {}", stderr);
        }

        // Check if there's anything to commit
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&worktree_path)
            .output()
            .await?;

        if status_output.stdout.is_empty() {
            anyhow::bail!("Nothing to commit");
        }

        // Commit
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&worktree_path)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to commit: {}", stderr);
        }

        // Get commit hash
        let hash_output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&worktree_path)
            .output()
            .await?;

        let commit_hash = String::from_utf8_lossy(&hash_output.stdout)
            .trim()
            .to_string();

        tracing::info!("Committed work in '{}': {}", name, commit_hash);

        Ok(commit_hash)
    }

    /// Revert a worktree to a specific checkpoint (commit hash).
    ///
    /// Used for the autoresearch pattern: reset to checkpoint if experiment fails.
    ///
    /// Runs: `git reset --hard <commit_hash>` in the worktree directory.
    pub async fn revert_to_checkpoint(
        &self,
        name: &str,
        commit_hash: &str,
    ) -> anyhow::Result<()> {
        let worktree_path = self.worktrees_dir.join(name);

        let output = Command::new("git")
            .args(["reset", "--hard", commit_hash])
            .current_dir(&worktree_path)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to reset to checkpoint: {}", stderr);
        }

        tracing::info!("Reverted '{}' to checkpoint {}", name, commit_hash);
        Ok(())
    }

    /// Create a checkpoint (snapshot of current state).
    ///
    /// Returns the current HEAD commit hash.
    ///
    /// Used for the autoresearch pattern: save state before experiment.
    pub async fn create_checkpoint(&self, name: &str) -> anyhow::Result<String> {
        let worktree_path = self.worktrees_dir.join(name);

        // First, commit any uncommitted changes
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&worktree_path)
            .output()
            .await?;

        if !status_output.stdout.is_empty() {
            // Auto-commit with checkpoint message
            let _ = Command::new("git")
                .args(["add", "-A"])
                .current_dir(&worktree_path)
                .output()
                .await;

            let _ = Command::new("git")
                .args(["commit", "-m", "checkpoint: auto-save before experiment"])
                .current_dir(&worktree_path)
                .output()
                .await;
        }

        // Get current HEAD
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&worktree_path)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to get HEAD: {}", stderr);
        }

        let commit_hash = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        tracing::debug!("Created checkpoint for '{}': {}", name, commit_hash);
        Ok(commit_hash)
    }

    /// Clean up worktrees older than the specified age.
    ///
    /// Returns the names of removed worktrees.
    pub async fn cleanup_stale(&self, max_age_hours: u64) -> anyhow::Result<Vec<String>> {
        let worktrees = self.list().await?;
        let now = Utc::now();
        let max_age = chrono::Duration::hours(max_age_hours as i64);
        let mut removed = Vec::new();

        for worktree in worktrees {
            let age = now.signed_duration_since(worktree.created_at);
            if age > max_age {
                match self.remove(&worktree.name).await {
                    Ok(_) => {
                        tracing::info!(
                            "Cleaned up stale worktree '{}' (age: {}h)",
                            worktree.name,
                            age.num_hours()
                        );
                        removed.push(worktree.name);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to clean up worktree '{}': {}",
                            worktree.name,
                            e
                        );
                    }
                }
            }
        }

        Ok(removed)
    }

    /// Get the path to a worktree by name.
    pub fn worktree_path(&self, name: &str) -> PathBuf {
        self.worktrees_dir.join(name)
    }

    /// Check if a worktree exists.
    pub async fn exists(&self, name: &str) -> bool {
        let path = self.worktree_path(name);
        tokio::fs::metadata(&path).await.is_ok()
    }

    /// Get the current branch of a worktree.
    pub async fn current_branch(&self, name: &str) -> anyhow::Result<String> {
        let worktree_path = self.worktrees_dir.join(name);

        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&worktree_path)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to get current branch: {}", stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Get the status of a worktree (modified files).
    pub async fn status(&self, name: &str) -> anyhow::Result<Vec<String>> {
        let worktree_path = self.worktrees_dir.join(name);

        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&worktree_path)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to get status: {}", stderr);
        }

        let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.to_string())
            .collect();

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_test_repo() -> (TempDir, WorktreeManager) {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path();

        // Initialize a git repo
        Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .await
            .unwrap();

        // Configure git user for commits
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(repo_path)
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(repo_path)
            .output()
            .await
            .unwrap();

        // Create initial commit
        let readme = repo_path.join("README.md");
        tokio::fs::write(&readme, "# Test Repo").await.unwrap();

        Command::new("git")
            .args(["add", "-A"])
            .current_dir(repo_path)
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(repo_path)
            .output()
            .await
            .unwrap();

        let manager = WorktreeManager::new(repo_path);
        (temp_dir, manager)
    }

    #[tokio::test]
    async fn test_create_and_list_worktree() {
        let (_temp_dir, manager) = setup_test_repo().await;

        // Create a worktree
        let info = manager.create("test-agent", None).await.unwrap();
        assert_eq!(info.name, "test-agent");
        assert_eq!(info.branch, "rustclaw/test-agent");
        assert!(info.path.exists());

        // List worktrees
        let worktrees = manager.list().await.unwrap();
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].name, "test-agent");

        // Remove worktree
        manager.remove("test-agent").await.unwrap();
        let worktrees = manager.list().await.unwrap();
        assert!(worktrees.is_empty());
    }

    #[tokio::test]
    async fn test_commit_and_checkpoint() {
        let (_temp_dir, manager) = setup_test_repo().await;

        // Create a worktree
        let info = manager.create("checkpoint-test", None).await.unwrap();

        // Create a file
        let test_file = info.path.join("test.txt");
        tokio::fs::write(&test_file, "hello").await.unwrap();

        // Create checkpoint (should auto-commit)
        let checkpoint = manager.create_checkpoint("checkpoint-test").await.unwrap();
        assert!(!checkpoint.is_empty());

        // Modify file
        tokio::fs::write(&test_file, "modified").await.unwrap();

        // Commit the change
        let commit = manager
            .commit_work("checkpoint-test", "test commit")
            .await
            .unwrap();
        assert!(!commit.is_empty());
        assert_ne!(commit, checkpoint);

        // Revert to checkpoint
        manager
            .revert_to_checkpoint("checkpoint-test", &checkpoint)
            .await
            .unwrap();

        // Verify file is back to original
        let content = tokio::fs::read_to_string(&test_file).await.unwrap();
        assert_eq!(content, "hello");
    }

    #[tokio::test]
    async fn test_worktree_path() {
        let (_temp_dir, manager) = setup_test_repo().await;

        let path = manager.worktree_path("my-agent");
        assert!(path.ends_with("my-agent"));
        assert!(path.to_string_lossy().contains(".rustclaw/worktrees"));
    }
}
