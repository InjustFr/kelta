# Sign in with the browser (GitHub and GitLab)

Instead of pasting a personal access token, a GitHub or GitLab account can sign in with the OAuth device
flow: Kelta shows a short code, you open the provider's page, type the code and approve. Kelta then keeps its
own token in your keychain (or the encrypted secrets file) and, on GitLab, refreshes it before it expires.

The flow needs an OAuth application registered once per host. Its client id is public (the device flow uses no
client secret), but Kelta ships none: you register your own and put its id in `oauth.client_ids`. Until a host
has one, the accounts wizard explains this and only offers tokens.

## GitHub (github.com or Enterprise Server)

1. Settings > Developer settings > OAuth Apps > New OAuth App
   (<https://github.com/settings/applications/new>; an organization can own it instead).
2. Name `Kelta`, homepage URL anything (e.g. `https://github.com/InjustFr/kelta`), callback URL anything
   (the device flow does not use it).
3. Register, then tick **Enable Device Flow** and save.
4. Copy the **Client ID** (`Ov23li…`). Do not generate a client secret; Kelta does not need one.

Kelta asks for the `repo read:org project` scopes. OAuth App tokens do not expire, so nothing is refreshed.

## GitLab (gitlab.com or self-managed, 17.9 or later)

1. Edit profile > Access > Applications > Add new application (a group or instance admin can own it instead).
2. Name `Kelta`, redirect URI anything (e.g. `http://localhost/`), **untick Confidential**, scope `api`.
3. Save and copy the **Application ID**.

Register the application on each self-managed instance you use. GitLab access tokens last two hours: Kelta
refreshes them with the refresh token shortly before they expire. If the refresh fails (token revoked,
application deleted), the account shows "needs auth" and you sign in again from Settings > Accounts.

## Configure Kelta

Put the ids in the global `config.toml`, keyed by the web host (not `api.github.com`):

```toml
[oauth.client_ids]
"github.com" = "Ov23liXXXXXXXXXXXXXX"
"gitlab.com" = "0123456789abcdef…"
"gitlab.acme.example" = "fedcba9876543210…"
```

Then Settings > Accounts > Add account > GitHub or GitLab > Next shows **Sign in with GitHub / GitLab**. The
account is saved with `auth = "oauth"` and `secret = "keyring:<id>"` (or the `file:` reference you picked).

## Where the tokens live

- The access token is stored at the account's `secret` reference, exactly like a pasted token.
- Next to it, `<secret>.oauth` (e.g. `keyring:gitlab.oauth`) holds the client id, the token URL, the refresh
  token and the expiry. Both live only in the secret backend: never in `config.toml`, the logs or the UI.
- The device code stays in the Kelta process; the UI only ever sees the code you type.
