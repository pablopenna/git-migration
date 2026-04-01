use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::Deserialize;

use crate::credentials::ProviderCredentials;
use crate::provider::{Provider, RepoInfo};

pub struct GithubProvider {
    pub creds: ProviderCredentials,
    client: Client,
}

#[derive(Deserialize)]
struct GithubRepo {
    name: String,
    private: bool,
}

impl GithubProvider {
    pub fn new(creds: ProviderCredentials) -> Self {
        Self {
            creds,
            client: Client::new(),
        }
    }

    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.creds.token).parse().unwrap(),
        );
        headers.insert(
            reqwest::header::ACCEPT,
            "application/vnd.github+json".parse().unwrap(),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            "2022-11-28".parse().unwrap(),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            "git-migration/1.0".parse().unwrap(),
        );
        headers
    }
}

#[async_trait::async_trait]
impl Provider for GithubProvider {
    async fn list_repos(&self) -> Result<Vec<RepoInfo>> {
        let mut repos = Vec::new();
        let mut page = 1u32;

        loop {
            let url = format!(
                "{}/user/repos?per_page=100&page={}",
                self.creds.api_url, page
            );
            let resp = self
                .client
                .get(&url)
                .headers(self.auth_headers())
                .send()
                .await
                .context("Failed to list GitHub repos")?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                bail!("GitHub list repos failed ({}): {}", status, body);
            }

            let page_repos: Vec<GithubRepo> = resp
                .json()
                .await
                .context("Failed to parse GitHub repos response")?;
            if page_repos.is_empty() {
                break;
            }

            for r in page_repos {
                repos.push(RepoInfo {
                    name: r.name,
                    is_private: r.private,
                });
            }

            page += 1;
        }

        Ok(repos)
    }

    async fn repo_exists(&self, name: &str) -> Result<bool> {
        let url = format!(
            "{}/repos/{}/{}",
            self.creds.api_url, self.creds.username, name
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("Failed to check GitHub repo existence")?;

        Ok(resp.status().is_success())
    }

    async fn create_repo(&self, name: &str, private: bool) -> Result<()> {
        let url = format!("{}/user/repos", self.creds.api_url);
        let body = serde_json::json!({
            "name": name,
            "private": private,
            "auto_init": false,
        });

        let resp = self
            .client
            .post(&url)
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await
            .context("Failed to create GitHub repo")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("GitHub create repo failed ({}): {}", status, body);
        }

        Ok(())
    }

    fn clone_url(&self, name: &str) -> String {
        // Extract hostname from api_url (https://api.github.com -> github.com)
        let host = self
            .creds
            .api_url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let host = if host.starts_with("api.") {
            &host[4..]
        } else {
            host
        };
        format!(
            "https://{}:{}@{}/{}/{}.git",
            self.creds.username, self.creds.token, host, self.creds.username, name
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

    fn make_provider(api_url: &str) -> GithubProvider {
        GithubProvider::new(ProviderCredentials {
            api_url: api_url.to_string(),
            token: "mytoken".to_string(),
            username: "alice".to_string(),
            namespace: None,
        })
    }

    #[test]
    fn test_clone_url_standard() {
        let p = make_provider("https://api.github.com");
        assert_eq!(
            p.clone_url("myrepo"),
            "https://alice:mytoken@github.com/alice/myrepo.git"
        );
    }

    #[test]
    fn test_clone_url_strips_api_prefix() {
        // api.github.com → github.com
        let p = make_provider("https://api.github.com");
        let url = p.clone_url("repo");
        assert!(!url.contains("api.github.com"), "api. prefix should be stripped");
        assert!(url.contains("github.com"));
    }

    #[test]
    fn test_clone_url_self_hosted_no_api_prefix() {
        // Self-hosted GitHub Enterprise where the API is at https://ghes.corp.com/api/v3
        // The host part has no "api." prefix, so it should be kept as-is.
        let p = make_provider("https://ghes.corp.com");
        let url = p.clone_url("repo");
        assert!(url.contains("ghes.corp.com"), "custom host should be preserved");
    }

    #[test]
    fn test_clone_url_embeds_credentials() {
        let p = make_provider("https://api.github.com");
        let url = p.clone_url("repo");
        assert!(url.contains("alice:mytoken@"), "credentials should be embedded");
    }

    #[test]
    fn test_push_url_equals_clone_url() {
        let p = make_provider("https://api.github.com");
        assert_eq!(p.clone_url("repo"), p.push_url("repo"));
    }
}
