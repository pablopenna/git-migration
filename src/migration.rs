use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;

use crate::provider::{Provider, RepoInfo};

/// Strip embedded credentials (username:token@) from a URL for safe display in error messages.
pub(crate) fn redact_url(url: &str) -> String {
    // Match https://user:token@host/... or https://token@host/...
    if let Some(at_pos) = url.find('@') {
        let scheme_end = url.find("://").map(|i| i + 3).unwrap_or(0);
        if at_pos > scheme_end {
            let scheme = &url[..scheme_end];
            let rest = &url[at_pos + 1..];
            return format!("{}<credentials>@{}", scheme, rest);
        }
    }
    url.to_string()
}

/// Run a git command, returning stdout on success or an error with stderr on failure.
/// The `display_args` parameter is used in error messages instead of the real args
/// to avoid leaking credentials embedded in URLs.
fn run_git(args: &[&str], display_args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    // Prevent git from prompting for credentials interactively
    cmd.env("GIT_TERMINAL_PROMPT", "0");

    let output = cmd
        .output()
        .with_context(|| format!("Failed to spawn git {:?}", display_args))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        bail!(
            "git {:?} failed (exit {})\nstdout: {}\nstderr: {}",
            display_args,
            output.status,
            stdout.trim(),
            stderr.trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Migrate a single repository from source to destination.
pub async fn migrate_repo(
    repo: &RepoInfo,
    source: Arc<dyn Provider>,
    dest: Arc<dyn Provider>,
) -> Result<()> {
    let name = &repo.name;

    // Create a temp directory for the bare clone
    let tmp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
    let clone_path = tmp_dir.path().join(name);
    let clone_path_str = clone_path.to_str().unwrap();

    // 1. Mirror-clone from source
    let src_url = source.clone_url(name);
    let src_display = redact_url(&src_url);
    run_git(
        &["clone", "--mirror", &src_url, clone_path_str],
        &["clone", "--mirror", &src_display, clone_path_str],
        None,
    )
    .with_context(|| format!("Failed to mirror-clone '{}'", name))?;

    // 2. Ensure destination repo exists; create it if not
    let exists = dest
        .repo_exists(name)
        .await
        .with_context(|| format!("Failed to check if '{}' exists on destination", name))?;

    if !exists {
        dest.create_repo(name, repo.is_private)
            .await
            .with_context(|| format!("Failed to create '{}' on destination", name))?;
    }

    // 3. Push all refs to destination with --mirror (handles branches, tags, deletions)
    let dest_url = dest.push_url(name);
    let dest_display = redact_url(&dest_url);
    run_git(
        &["push", "--mirror", &dest_url],
        &["push", "--mirror", &dest_display],
        Some(&clone_path),
    )
    .with_context(|| format!("Failed to mirror-push '{}' to destination", name))?;

    // tmp_dir is dropped here, cleaning up the bare clone automatically
    Ok(())
}

/// Result for a single repo migration.
pub struct RepoResult {
    pub name: String,
    pub error: Option<String>,
}

/// Run the full migration across all repos, with concurrency limited to `jobs`.
pub async fn run_migration(
    repos: Vec<RepoInfo>,
    source: Arc<dyn Provider>,
    dest: Arc<dyn Provider>,
    jobs: usize,
) -> Vec<RepoResult> {
    use futures::stream::{self, StreamExt};

    let semaphore = Arc::new(tokio::sync::Semaphore::new(jobs));

    let results: Vec<RepoResult> = stream::iter(repos)
        .map(|repo| {
            let source = Arc::clone(&source);
            let dest = Arc::clone(&dest);
            let sem = Arc::clone(&semaphore);

            async move {
                let _permit = sem.acquire().await.unwrap();
                let name = repo.name.clone();
                match migrate_repo(&repo, source, dest).await {
                    Ok(()) => {
                        println!("[OK] {}", name);
                        RepoResult { name, error: None }
                    }
                    Err(e) => {
                        println!("[FAIL] {}: {:#}", name, e);
                        RepoResult {
                            name,
                            error: Some(format!("{:#}", e)),
                        }
                    }
                }
            }
        })
        .buffer_unordered(jobs)
        .collect()
        .await;

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- redact_url ---

    #[test]
    fn test_redact_url_user_and_token() {
        let url = "https://alice:ghp_secret123@github.com/alice/repo.git";
        let redacted = redact_url(url);
        assert_eq!(redacted, "https://<credentials>@github.com/alice/repo.git");
        assert!(!redacted.contains("ghp_secret123"));
        assert!(!redacted.contains("alice:"));
    }

    #[test]
    fn test_redact_url_token_only() {
        let url = "https://oauth2:glpat-token@gitlab.com/ns/repo.git";
        let redacted = redact_url(url);
        assert_eq!(redacted, "https://<credentials>@gitlab.com/ns/repo.git");
        assert!(!redacted.contains("glpat-token"));
    }

    #[test]
    fn test_redact_url_no_credentials() {
        let url = "https://github.com/alice/repo.git";
        assert_eq!(redact_url(url), url);
    }

    #[test]
    fn test_redact_url_preserves_path() {
        let url = "https://user:token@example.com/org/subrepo.git";
        let redacted = redact_url(url);
        assert!(redacted.ends_with("@example.com/org/subrepo.git"));
    }

    // --- run_migration orchestration ---

    struct MockProvider {
        /// Repos to report as already existing on this provider.
        existing: Vec<String>,
        /// If true, clone_url returns an invalid URL so git clone will fail.
        fail_clone: bool,
        /// Track which repos were passed to create_repo.
        created: Arc<Mutex<Vec<String>>>,
    }

    impl MockProvider {
        fn failing() -> Self {
            Self {
                existing: vec![],
                fail_clone: true,
                created: Arc::new(Mutex::new(vec![])),
            }
        }
    }

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        async fn list_repos(&self) -> Result<Vec<RepoInfo>> {
            Ok(vec![])
        }

        async fn repo_exists(&self, name: &str) -> Result<bool> {
            Ok(self.existing.contains(&name.to_string()))
        }

        async fn create_repo(&self, name: &str, _private: bool) -> Result<()> {
            self.created.lock().unwrap().push(name.to_string());
            Ok(())
        }

        fn clone_url(&self, name: &str) -> String {
            if self.fail_clone {
                // Invalid URL — git clone will fail immediately
                format!("https://invalid.local.invalid/repo/{}.git", name)
            } else {
                // Won't be called in tests that expect success without real git
                String::new()
            }
        }

        fn push_url(&self, name: &str) -> String {
            self.clone_url(name)
        }
    }

    fn make_repos(names: &[&str]) -> Vec<RepoInfo> {
        names
            .iter()
            .map(|n| RepoInfo {
                name: n.to_string(),
                is_private: false,
            })
            .collect()
    }

    #[tokio::test]
    async fn test_run_migration_empty_list() {
        let source = Arc::new(MockProvider::failing());
        let dest = Arc::new(MockProvider::failing());
        let results = run_migration(vec![], source, dest, 4).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_run_migration_continues_on_failure() {
        // All git clones will fail (invalid host), but run_migration must not abort early.
        let repos = make_repos(&["alpha", "beta", "gamma"]);
        let source = Arc::new(MockProvider::failing());
        let dest = Arc::new(MockProvider::failing());

        let results = run_migration(repos, source, dest, 2).await;

        assert_eq!(results.len(), 3, "all repos should produce a result");
        assert!(
            results.iter().all(|r| r.error.is_some()),
            "all should fail with git errors"
        );
    }

    #[tokio::test]
    async fn test_run_migration_result_names_match_input() {
        let repos = make_repos(&["repo-a", "repo-b"]);
        let source = Arc::new(MockProvider::failing());
        let dest = Arc::new(MockProvider::failing());

        let results = run_migration(repos, source, dest, 1).await;

        let mut names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["repo-a", "repo-b"]);
    }
}
