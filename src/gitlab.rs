use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::Deserialize;

use crate::credentials::ProviderCredentials;
use crate::provider::{Provider, RepoInfo};

pub struct GitlabProvider {
    pub creds: ProviderCredentials,
    client: Client,
}

#[derive(Deserialize)]
struct GitlabProject {
    /// The URL-friendly slug (path) of the project, used as the canonical name.
    path: String,
    visibility: String,
}

impl GitlabProvider {
    pub fn new(creds: ProviderCredentials) -> Self {
        Self {
            creds,
            client: Client::new(),
        }
    }

    fn namespace(&self) -> &str {
        self.creds
            .namespace
            .as_deref()
            .unwrap_or(self.creds.username.as_str())
    }

    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("PRIVATE-TOKEN", self.creds.token.parse().unwrap());
        headers.insert(
            reqwest::header::USER_AGENT,
            "git-migration/1.0".parse().unwrap(),
        );
        headers
    }

    /// URL-encode "namespace/repo" for GitLab project path lookup.
    pub(crate) fn encoded_path(&self, name: &str) -> String {
        let path = format!("{}/{}", self.namespace(), name);
        path.replace('/', "%2F")
    }

    /// Derive the base host URL from api_url by stripping the "/api/v4" suffix.
    pub(crate) fn base_host(&self) -> &str {
        let raw = self
            .creds
            .api_url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        raw.split('/').next().unwrap_or(raw)
    }
}

#[async_trait::async_trait]
impl Provider for GitlabProvider {
    async fn list_repos(&self) -> Result<Vec<RepoInfo>> {
        let mut repos = Vec::new();
        let mut page = 1u32;

        loop {
            let url = format!(
                "{}/projects?owned=true&per_page=100&page={}",
                self.creds.api_url, page
            );
            let resp = self
                .client
                .get(&url)
                .headers(self.auth_headers())
                .send()
                .await
                .context("Failed to list GitLab projects")?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                bail!("GitLab list projects failed ({}): {}", status, body);
            }

            let page_repos: Vec<GitlabProject> = resp
                .json()
                .await
                .context("Failed to parse GitLab projects")?;
            if page_repos.is_empty() {
                break;
            }

            for r in page_repos {
                repos.push(RepoInfo {
                    name: r.path,
                    is_private: r.visibility != "public",
                });
            }

            page += 1;
        }

        Ok(repos)
    }

    async fn repo_exists(&self, name: &str) -> Result<bool> {
        let url = format!(
            "{}/projects/{}",
            self.creds.api_url,
            self.encoded_path(name)
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("Failed to check GitLab project existence")?;

        Ok(resp.status().is_success())
    }

    async fn create_repo(&self, name: &str, private: bool) -> Result<()> {
        let url = format!("{}/projects", self.creds.api_url);
        let visibility = if private { "private" } else { "public" };
        let body = serde_json::json!({
            "name": name,
            "path": name,
            "namespace_path": self.namespace(),
            "visibility": visibility,
        });

        let resp = self
            .client
            .post(&url)
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await
            .context("Failed to create GitLab project")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("GitLab create project failed ({}): {}", status, body);
        }

        Ok(())
    }

    fn clone_url(&self, name: &str) -> String {
        format!(
            "https://oauth2:{}@{}/{}/{}.git",
            self.creds.token,
            self.base_host(),
            self.namespace(),
            name
        )
    }

    fn push_url(&self, name: &str) -> String {
        self.clone_url(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::ProviderCredentials;

    fn make_provider(namespace: Option<&str>) -> GitlabProvider {
        GitlabProvider::new(ProviderCredentials {
            api_url: "https://gitlab.com/api/v4".to_string(),
            token: "glpat-secret".to_string(),
            username: "alice".to_string(),
            namespace: namespace.map(|s| s.to_string()),
        })
    }

    // --- base_host ---

    #[test]
    fn test_base_host_standard() {
        let p = make_provider(None);
        assert_eq!(p.base_host(), "gitlab.com");
    }

    #[test]
    fn test_base_host_self_hosted() {
        let p = GitlabProvider::new(ProviderCredentials {
            api_url: "https://gitlab.corp.example.com/api/v4".to_string(),
            token: "t".to_string(),
            username: "u".to_string(),
            namespace: None,
        });
        assert_eq!(p.base_host(), "gitlab.corp.example.com");
    }

    // --- encoded_path ---

    #[test]
    fn test_encoded_path_uses_namespace() {
        let p = make_provider(Some("mygroup"));
        assert_eq!(p.encoded_path("myrepo"), "mygroup%2Fmyrepo");
    }

    #[test]
    fn test_encoded_path_falls_back_to_username() {
        let p = make_provider(None);
        assert_eq!(p.encoded_path("myrepo"), "alice%2Fmyrepo");
    }

    #[test]
    fn test_encoded_path_no_literal_slash() {
        let p = make_provider(Some("group"));
        assert!(!p.encoded_path("repo").contains('/'));
    }

    // --- clone_url / push_url ---

    #[test]
    fn test_clone_url_format() {
        let p = make_provider(Some("mygroup"));
        assert_eq!(
            p.clone_url("myrepo"),
            "https://oauth2:glpat-secret@gitlab.com/mygroup/myrepo.git"
        );
    }

    #[test]
    fn test_clone_url_embeds_token() {
        let p = make_provider(None);
        let url = p.clone_url("repo");
        assert!(url.contains("glpat-secret"), "token should be in URL");
        assert!(url.contains("oauth2:"), "oauth2 prefix should be present");
    }

    #[test]
    fn test_push_url_equals_clone_url() {
        let p = make_provider(Some("grp"));
        assert_eq!(p.clone_url("r"), p.push_url("r"));
    }
}
