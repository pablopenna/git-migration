mod credentials;
mod gitbucket;
mod github;
mod gitlab;
mod migration;
mod provider;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use provider::RepoInfo;

use credentials::load_credentials;
use gitbucket::GitbucketProvider;
use github::GithubProvider;
use gitlab::GitlabProvider;
use migration::run_migration;
use provider::Provider;

#[derive(Parser, Debug)]
#[command(name = "git-migration", about = "Migrate git repositories between providers")]
struct Cli {
    /// Source provider (github, gitlab, gitbucket)
    #[arg(long)]
    source: String,

    /// Destination provider (github, gitlab, gitbucket)
    #[arg(long)]
    destination: String,

    /// Execution mode: exclude (copy all except listed) or include (copy only listed)
    #[arg(long)]
    mode: String,

    /// Number of concurrent migration jobs (default: number of CPUs)
    #[arg(long)]
    jobs: Option<usize>,

    /// Path to the exclusion list file (default: ./excluded)
    #[arg(long, default_value = "./excluded")]
    excluded_file: PathBuf,

    /// Path to the inclusion list file (default: ./included)
    #[arg(long, default_value = "./included")]
    included_file: PathBuf,

    /// Path to the credentials file (default: ./.credentials.json)
    #[arg(long, default_value = "./.credentials.json")]
    credentials_file: PathBuf,
}

pub(crate) fn filter_repos(repos: Vec<RepoInfo>, mode: &str, list: &[String]) -> Vec<RepoInfo> {
    match mode {
        "exclude" => repos.into_iter().filter(|r| !list.contains(&r.name)).collect(),
        "include" => repos.into_iter().filter(|r| list.contains(&r.name)).collect(),
        _ => repos,
    }
}

fn build_provider(name: &str, creds: &credentials::Credentials) -> Result<Arc<dyn Provider>> {
    let provider_creds = creds
        .get(name)
        .with_context(|| format!("No credentials found for provider '{}'", name))?
        .clone();

    match name {
        "github" => Ok(Arc::new(GithubProvider::new(provider_creds))),
        "gitlab" => Ok(Arc::new(GitlabProvider::new(provider_creds))),
        "gitbucket" => Ok(Arc::new(GitbucketProvider::new(provider_creds))),
        other => bail!(
            "Unknown provider: '{}'. Supported providers: github, gitlab, gitbucket",
            other
        ),
    }
}

/// Read a list of repo names from a file (one per line, ignoring blank lines and comments).
pub(crate) fn read_name_list(path: &PathBuf) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;
    let names = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect();
    Ok(names)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Validate mode
    if cli.mode != "exclude" && cli.mode != "include" {
        bail!(
            "--mode must be either 'exclude' or 'include', got '{}'",
            cli.mode
        );
    }

    // Load credentials
    let creds = load_credentials(&cli.credentials_file).context("Failed to load credentials")?;

    // Build provider instances
    let source = build_provider(&cli.source, &creds)
        .with_context(|| format!("Failed to build source provider '{}'", cli.source))?;
    let dest = build_provider(&cli.destination, &creds)
        .with_context(|| format!("Failed to build destination provider '{}'", cli.destination))?;

    // Determine concurrency
    let jobs = cli.jobs.unwrap_or_else(num_cpus::get).max(1);
    println!("Concurrency: {} job(s)", jobs);

    // List all repos from source
    println!("Fetching repo list from '{}'...", cli.source);
    let all_repos = source
        .list_repos()
        .await
        .context("Failed to list source repos")?;
    println!("Found {} repo(s) on source.", all_repos.len());

    // Apply include/exclude filter
    let filter_list: Vec<String> = match cli.mode.as_str() {
        "exclude" => {
            if cli.excluded_file.exists() {
                read_name_list(&cli.excluded_file).context("Failed to read excluded file")?
            } else {
                println!(
                    "Excluded file '{}' not found; no repos will be excluded.",
                    cli.excluded_file.display()
                );
                vec![]
            }
        }
        "include" => read_name_list(&cli.included_file).with_context(|| {
            format!(
                "Failed to read included file: {}",
                cli.included_file.display()
            )
        })?,
        _ => unreachable!(),
    };
    let repos = filter_repos(all_repos, &cli.mode, &filter_list);

    println!("Migrating {} repo(s)...", repos.len());

    if repos.is_empty() {
        println!("Nothing to migrate.");
        return Ok(());
    }

    // Run migration
    let results = run_migration(repos, source, dest, jobs).await;

    // Print summary
    let total = results.len();
    let succeeded = results.iter().filter(|r| r.error.is_none()).count();
    let failed = total - succeeded;

    println!();
    println!("=== Migration Summary ===");
    println!("Total:     {}", total);
    println!("Succeeded: {}", succeeded);
    println!("Failed:    {}", failed);

    if failed > 0 {
        println!();
        println!("Failed repositories:");
        for result in &results {
            if let Some(ref err) = result.error {
                println!("  - {}: {}", result.name, err);
            }
        }
        // Exit with non-zero to indicate partial failure
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn repos(names: &[&str]) -> Vec<RepoInfo> {
        names
            .iter()
            .map(|n| RepoInfo {
                name: n.to_string(),
                is_private: false,
            })
            .collect()
    }

    fn names(repos: &[RepoInfo]) -> Vec<&str> {
        repos.iter().map(|r| r.name.as_str()).collect()
    }

    // --- filter_repos ---

    #[test]
    fn test_filter_exclude_removes_listed() {
        let all = repos(&["alpha", "beta", "gamma"]);
        let excluded = vec!["beta".to_string()];
        let result = filter_repos(all, "exclude", &excluded);
        assert_eq!(names(&result), vec!["alpha", "gamma"]);
    }

    #[test]
    fn test_filter_exclude_empty_list_keeps_all() {
        let all = repos(&["alpha", "beta"]);
        let result = filter_repos(all, "exclude", &[]);
        assert_eq!(names(&result), vec!["alpha", "beta"]);
    }

    #[test]
    fn test_filter_include_keeps_only_listed() {
        let all = repos(&["alpha", "beta", "gamma"]);
        let included = vec!["alpha".to_string(), "gamma".to_string()];
        let result = filter_repos(all, "include", &included);
        assert_eq!(names(&result), vec!["alpha", "gamma"]);
    }

    #[test]
    fn test_filter_include_no_matches_returns_empty() {
        let all = repos(&["alpha", "beta"]);
        let included = vec!["delta".to_string()];
        let result = filter_repos(all, "include", &included);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_exclude_all_listed() {
        let all = repos(&["alpha", "beta"]);
        let excluded = vec!["alpha".to_string(), "beta".to_string()];
        let result = filter_repos(all, "exclude", &excluded);
        assert!(result.is_empty());
    }

    // --- read_name_list ---

    fn write_temp(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_read_name_list_basic() {
        let f = write_temp("repo-a\nrepo-b\nrepo-c\n");
        let list = read_name_list(&f.path().to_path_buf()).unwrap();
        assert_eq!(list, vec!["repo-a", "repo-b", "repo-c"]);
    }

    #[test]
    fn test_read_name_list_skips_blank_lines() {
        let f = write_temp("repo-a\n\n   \nrepo-b\n");
        let list = read_name_list(&f.path().to_path_buf()).unwrap();
        assert_eq!(list, vec!["repo-a", "repo-b"]);
    }

    #[test]
    fn test_read_name_list_skips_comments() {
        let f = write_temp("# this is a comment\nrepo-a\n# another comment\nrepo-b\n");
        let list = read_name_list(&f.path().to_path_buf()).unwrap();
        assert_eq!(list, vec!["repo-a", "repo-b"]);
    }

    #[test]
    fn test_read_name_list_trims_whitespace() {
        let f = write_temp("  repo-a  \n  repo-b\n");
        let list = read_name_list(&f.path().to_path_buf()).unwrap();
        assert_eq!(list, vec!["repo-a", "repo-b"]);
    }

    #[test]
    fn test_read_name_list_missing_file_errors() {
        let result = read_name_list(&PathBuf::from("/nonexistent/file"));
        assert!(result.is_err());
    }
}
