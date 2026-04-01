# Git Migration

This repository is an implementation in Rust to migrate your repositories hosted in Github.com to other providers like Gitlab.com

## Requirements

1.- Required credentials for accessing Github or other platform accounts will be present in a file called `.credentials.json` which will be excluded to be uploaded to this repository in `.gitignore`. The format is as follows:

```json
{
  "github": {
    "api_url": "https://api.github.com",
    "token": "<personal_access_token>",
    "username": "<github_username>"
  },
  "gitlab": {
    "api_url": "https://gitlab.com/api/v4",
    "token": "<personal_access_token>",
    "username": "<gitlab_username>",
    "namespace": "<group_or_username_for_new_repos>"
  },
  "gitbucket": {
    "api_url": "https://<gitbucket_host>/api/v3",
    "token": "<personal_access_token>",
    "username": "<gitbucket_username>",
    "namespace": "<group_or_username_for_new_repos>"
  }
}
```

Each top-level key is a provider name matching the supported providers in the codebase. At runtime, the entries for `--source` and `--destination` are looked up by their provider name.

- `api_url`: the base REST API URL. Overridable to support self-hosted instances.
- `token`: a personal access token (PAT) with at minimum `repo` scope on the source and `api` scope on the destination.
- `username`: the authenticated user's login name, used to list owned repositories on the source.
- `namespace` (destination only): the user or group namespace under which new repositories will be created. On GitLab/GitBucket this maps to a group path or username.

The source and destination providers are passed as CLI parameters:

```
git-migration --source <provider> --destination <provider> --mode <exclude|include> \
              [--jobs <n>] [--excluded-file <path>] [--included-file <path>] \
              [--credentials-file <path>]
```

- `--source`: the provider to migrate from (e.g. `github`, `gitlab`, `gitbucket`).
- `--destination`: the provider to migrate to (e.g. `github`, `gitlab`, `gitbucket`).
- `--jobs`: number of concurrent migrations (default: number of CPUs).
- `--excluded-file`: path to the exclusion list (default: `./excluded`).
- `--included-file`: path to the inclusion list (default: `./included`).
- `--credentials-file`: path to the credentials file (default: `./.credentials.json`).

Both `--source` and `--destination` must match a supported provider in the codebase. Credentials for each are looked up by their role (`source` / `destination`) in `.credentials.json`.

2.- The program should have two execution modes: `exclude` and `include` which are provided as a parameter.

  a.- In the exclude mode, all repositories will be copied over save for the ones whose names match the ones in the file `excluded`. The format of the file is one repository name per line
  b.- In the include mode, only the repositories in the `included` file will be copied over. The format of the files is the same as `excluded`: one repository name per line.
  c.- Both files default to the project root (`./excluded` and `./included`). Their paths can be overridden via `--excluded-file <path>` and `--included-file <path>` CLI parameters.

3.- The program runs once and exits, it is not a process that polls the git provider. It has to be manually retriggered by the user.

4.- The logic of copying over is the following for each repository that is being processed:
  a.- Check if the repository exists in the destination. If not, create a new one with the same name, preserving the visibility of the source repository (public repositories are created as public, private repositories are created as private).
  b.- Mirror-clone the source repository with `git clone --mirror` into a temporary directory.
  c.- Push all branches with `git push --force --all` and all tags with `git push --force --tags` to the destination. Both commands are idempotent: refs that already exist and are up-to-date are no-ops, new refs are created, and updated refs are force-pushed. Stale refs at the destination that no longer exist at the source are not deleted. The temporary directory is cleaned up automatically after the push.

5.- The base structure of the app should be flexible enough so it can be easily expanded to support extra Git providers like GitBucket.

## Design Decisions

1. **Git operations**: Use native `git` CLI commands via `std::process::Command`. No libgit2 dependency.

2. **Parallelism**: Repositories are migrated in parallel by default using `tokio`. A `--jobs <n>` CLI parameter allows overriding the concurrency level (e.g. `--jobs 1` for sequential).

3. **Error handling**: On failure, skip the repository and continue. Print a summary at the end of execution with counts (succeeded / failed / skipped) and per-repository error details for any failures.

4. **Destination sync**: Use `git clone --mirror` to fetch all refs, then `git push --force --all` + `git push --force --tags` to push branches and tags respectively. `git push --mirror` is intentionally avoided: providers like GitLab protect the default branch from force-pushes and ref deletions, responding with HTTP 422. The force-push approach works regardless of branch protection settings. Trade-off: stale refs at the destination are not pruned.

5. **GitBucket**: Stub out the provider with the correct trait implementation so the architecture supports it, but leave the actual API calls unimplemented (panic or return `unimplemented!`). Only GitHub and GitLab are fully implemented.

6. **TLS**: Use `rustls` (pure-Rust TLS) instead of system OpenSSL. Enabled via `reqwest` feature flag `rustls-tls` with `default-features = false`. This makes the binary self-contained with no system SSL library dependency, so it runs identically on any machine regardless of OpenSSL version or presence.