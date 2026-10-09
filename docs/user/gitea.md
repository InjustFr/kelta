# Gitea and Forgejo

Issues and pull requests on a self-hosted Gitea or Forgejo, or Codeberg.

1. In your profile, Settings > Applications, create an access token with the `repository` and `user` scopes
   (read and write).
2. Store it: `secret = "keyring:gitea-main"`, `env:GITEA_TOKEN` or `command:pass show gitea`.
   (Settings > Accounts > Add account > Gitea does this for you.)
3. Add the account and bind a view:

```toml
[accounts.gitea-main]
kind = "gitea"
base_url = "https://git.example.org"   # required; a trailing /api/v1 is fine
secret = "keyring:gitea-main"

# in a project file
code_host = { account = "gitea-main", repo = "owner/name" }
[project.tracker]
account = "gitea-main"
views = [
  { id = "mine", label = "My issues", project = "owner/name" },
  # scope = "assigned_to_me" (default) | "all"; status = "open" (default) | "closed" | "*"
]
```

Issues are open or closed, so the board has two columns and moves are Close / Reopen. With `assigned_to_me` and
a `project`, the assigned search is filtered to that repository, so a page can come back short. Kelta does not
poll a change gate for Gitea: reviews refresh on the polling interval.
