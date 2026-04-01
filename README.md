# git-migration

Migrate repositories between Git providers (GitHub, GitLab, GitBucket).

## Prerequisites

- Rust toolchain installed (`cargo`)
- A `.credentials.json` file in the project root (see [Credentials setup](#credentials-setup) below)

## Setup

### 1. Build

```bash
cargo build --release
```

### 2. Configure which repositories to migrate

Depending on the mode you want to run:

- **exclude mode**: create an `excluded` file listing repository names to skip, one per line.
- **include mode**: create an `included` file listing the only repositories to migrate, one per line.

Example `excluded`:
```
my-private-scratch
old-archived-repo
```

Both files default to the project root. Their paths can be overridden with `--excluded-file` and `--included-file` (see Usage).

## Usage

```bash
./target/release/git-migration \
  --source github \
  --destination gitlab \
  --mode exclude
```

| Parameter | Description |
|---|---|
| `--source` | Provider to migrate from (`github`, `gitlab`, `gitbucket`) |
| `--destination` | Provider to migrate to (`github`, `gitlab`, `gitbucket`) |
| `--mode` | `exclude` to skip repos in `excluded` file, `include` to only migrate repos in `included` file |
| `--jobs` | Number of concurrent migrations (default: number of CPUs); use `--jobs 1` for sequential |
| `--excluded-file` | Path to the excluded list (default: `./excluded`) |
| `--included-file` | Path to the included list (default: `./included`) |
| `--credentials-file` | Path to the credentials file (default: `./.credentials.json`) |

The program runs once and exits with code 0 on full success or 1 if any repositories failed. Re-run it manually to sync again.

Lines in the `excluded`/`included` files starting with `#` are treated as comments and ignored.

---

## Credentials setup

Create a `.credentials.json` file in the project root. This file is gitignored and never committed. Only include entries for the providers you intend to use.

```json
{
  "github": {
    "api_url": "https://api.github.com",
    "token": "<personal_access_token>",
    "username": "<your_github_username>"
  },
  "gitlab": {
    "api_url": "https://gitlab.com/api/v4",
    "token": "<personal_access_token>",
    "username": "<your_gitlab_username>",
    "namespace": "<your_gitlab_username_or_group>"
  }
}
```

### GitHub — getting a personal access token

1. Go to [github.com](https://github.com) and sign in.
2. Click your profile picture (top right) → **Settings**.
3. In the left sidebar, scroll down and click **Developer settings**.
4. Click **Personal access tokens** → **Tokens (classic)**.
5. Click **Generate new token** → **Generate new token (classic)**.
6. Give it a descriptive name (e.g. `git-migration`).
7. Set an expiration as appropriate.
8. Under **Select scopes**, check **`repo`** (grants full access to public and private repositories — required to read private repos and their visibility settings).
9. Click **Generate token**.
10. Copy the token immediately — GitHub will not show it again. Paste it as the `token` value under `"github"` in `.credentials.json`.

Your `username` is your GitHub login name shown at the top of your profile page.

### GitLab — getting a personal access token

1. Go to [gitlab.com](https://gitlab.com) and sign in.
2. Click your profile picture (top right) → **Edit profile**.
3. In the left sidebar, click **Access tokens**.
4. Click **Add new token**.
5. Give it a descriptive name (e.g. `git-migration`).
6. Set an expiration date as appropriate.
7. Under **Select scopes**, check **`api`** (grants full API access, including creating repositories and pushing code).
8. Click **Create personal access token**.
9. Copy the token immediately — GitLab will not show it again. Paste it as the `token` value under `"gitlab"` in `.credentials.json`.

Your `username` is your GitLab login name shown on your profile page.

The `namespace` is the path prefix used for all repository operations (`namespace/repo-name`). It can be omitted — if absent, `username` is used as the fallback.

- **Personal repos** (under your own account): use your GitLab username, or omit the field entirely.
- **Group**: use the group's URL slug. For example, if your group is at `gitlab.com/my-org`, the namespace is `my-org`. For nested subgroups like `gitlab.com/my-org/team`, use `my-org/team`.

To find the right value, navigate to the group or your profile on GitLab and copy the path from the URL after `gitlab.com/`.
