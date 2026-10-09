# Linear

1. In Linear, open Settings > Security & access and create a personal API key.
2. Store it: `secret = "keyring:linear-acme"`, `env:LINEAR_API_KEY` or `command:pass show linear`.
   (Settings > Accounts > Add account > Linear does this for you.)
3. Add the account and bind a view:

```toml
[accounts.linear-acme]
kind = "linear"
secret = "keyring:linear-acme"
# auth = "bearer"   # only for OAuth access tokens; a personal key is sent as is

# in a project file
[project.tracker]
account = "linear-acme"
views = [
  { id = "eng", label = "Engineering", team = "ENG", project = "Website", labels = ["bug"] },
  # scope = "assigned_to_me" (default) | "all"; status = "open" (default) | "closed" | "*"
]
```

Board columns are the workflow states of the team (all teams if no `team` is set). Moving a ticket looks up the
state by name in the ticket's own team at the time you move it, so renamed or custom states just work.
Lists stop after 20 pages of 50 issues. Linear's rate limit is shared by all your tools; when it is spent Kelta
pauses requests to that account until the window resets.
