# Bitbucket Cloud

Pull requests only: Bitbucket removed its built-in issue tracker in August 2026, so bind a Jira account for tickets
and use Bitbucket as the code host.

1. Create an Atlassian API token with the Bitbucket scopes (account, repositories, pull requests read and write).
   App passwords were retired in 2026. A repository or workspace access token also works (`auth = "bearer"`), but
   it only sees one repository or workspace.
2. Store it: `secret = "keyring:bitbucket-acme"`, `env:BITBUCKET_TOKEN` or `command:pass show bitbucket`.
   (Settings > Accounts > Add account > Bitbucket does this for you.)

```toml
[accounts.bitbucket-acme]
kind = "bitbucket"
email = "me@acme.com"            # the Atlassian account e-mail; not needed with auth = "bearer"
secret = "keyring:bitbucket-acme"

# in a project file, per repository
code_host = { account = "bitbucket-acme", repo = "workspace/repo-slug" }
```

Limits: review requests are searched in the 50 most recently updated repositories you are a member of in each
workspace, lists show no CI status (the detail view does), approving cannot be pinned to a commit, and "Review
locally" fetches the PR's source branch, so PRs from forks cannot be reviewed locally.
