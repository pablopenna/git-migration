use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderCredentials {
    pub api_url: String,
    pub token: String,
    pub username: String,
    pub namespace: Option<String>,
}

pub type Credentials = HashMap<String, ProviderCredentials>;

pub fn load_credentials(path: &Path) -> Result<Credentials> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read credentials file: {}", path.display()))?;
    let creds: Credentials = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse credentials file: {}", path.display()))?;
    Ok(creds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_load_valid_credentials() {
        let json = r#"{
            "github": {
                "api_url": "https://api.github.com",
                "token": "ghp_abc",
                "username": "alice"
            },
            "gitlab": {
                "api_url": "https://gitlab.com/api/v4",
                "token": "glpat-xyz",
                "username": "alice",
                "namespace": "mygroup"
            }
        }"#;
        let f = write_temp(json);
        let creds = load_credentials(f.path()).unwrap();

        let gh = creds.get("github").unwrap();
        assert_eq!(gh.api_url, "https://api.github.com");
        assert_eq!(gh.token, "ghp_abc");
        assert_eq!(gh.username, "alice");
        assert!(gh.namespace.is_none());

        let gl = creds.get("gitlab").unwrap();
        assert_eq!(gl.namespace.as_deref(), Some("mygroup"));
    }

    #[test]
    fn test_load_missing_file() {
        let result = load_credentials(Path::new("/nonexistent/path/.credentials.json"));
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Failed to read credentials file"));
    }

    #[test]
    fn test_load_invalid_json() {
        let f = write_temp("not valid json {{{");
        let result = load_credentials(f.path());
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Failed to parse credentials file"));
    }

    #[test]
    fn test_namespace_optional() {
        // namespace field should be absent without causing a parse error
        let json = r#"{"github": {"api_url": "https://api.github.com", "token": "t", "username": "u"}}"#;
        let f = write_temp(json);
        let creds = load_credentials(f.path()).unwrap();
        assert!(creds.get("github").unwrap().namespace.is_none());
    }
}
