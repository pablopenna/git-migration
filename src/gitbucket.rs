use anyhow::Result;

use crate::credentials::ProviderCredentials;
use crate::provider::{Provider, RepoInfo};

/// Stub implementation of the GitBucket provider.
/// All methods are unimplemented and will panic at runtime.
pub struct GitbucketProvider {
    #[allow(dead_code)]
    pub creds: ProviderCredentials,
}

impl GitbucketProvider {
    pub fn new(creds: ProviderCredentials) -> Self {
        Self { creds }
    }
}

#[async_trait::async_trait]
impl Provider for GitbucketProvider {
    async fn list_repos(&self) -> Result<Vec<RepoInfo>> {
        unimplemented!("GitBucket provider is not yet implemented")
    }

    async fn repo_exists(&self, _name: &str) -> Result<bool> {
        unimplemented!("GitBucket provider is not yet implemented")
    }

    async fn create_repo(&self, _name: &str, _private: bool) -> Result<()> {
        unimplemented!("GitBucket provider is not yet implemented")
    }

    fn clone_url(&self, _name: &str) -> String {
        unimplemented!("GitBucket provider is not yet implemented")
    }

    fn push_url(&self, _name: &str) -> String {
        unimplemented!("GitBucket provider is not yet implemented")
    }
}
