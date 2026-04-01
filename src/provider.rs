use anyhow::Result;

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub name: String,
    pub is_private: bool,
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// List all repos owned by the authenticated user.
    async fn list_repos(&self) -> Result<Vec<RepoInfo>>;

    /// Check if a repo with the given name exists on the provider.
    async fn repo_exists(&self, name: &str) -> Result<bool>;

    /// Create a new repo with the given name and visibility.
    async fn create_repo(&self, name: &str, private: bool) -> Result<()>;

    /// Return the authenticated clone URL for a repo.
    fn clone_url(&self, name: &str) -> String;

    /// Return the authenticated push URL for a repo.
    fn push_url(&self, name: &str) -> String;
}
