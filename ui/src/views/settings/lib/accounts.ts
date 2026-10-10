// Account wizard model (SETTINGS §2 `[accounts.<id>]`): per-kind defaults and validation, and
// the JSON written to `accounts.<id>` (defaults are omitted so the file stays minimal).

import type { AccountKind, JsonValue } from '$lib/gen';

export interface KindInfo {
  kind: AccountKind;
  label: string;
  blurb: string;
  /** `base_url` must be provided. */
  baseUrlRequired: boolean;
  baseUrlDefault: string | null;
  baseUrlPlaceholder: string;
  /** Default SecretRef source when the account is created. */
  secretDefault: 'keyring' | 'gh-cli' | 'glab-cli';
  authOptions: readonly ('basic' | 'bearer' | 'api_key' | 'token')[];
  codeHost: boolean;
}

export const KINDS: readonly KindInfo[] = [
  {
    kind: 'jira',
    label: 'Jira',
    blurb: 'Jira Cloud or Data Center issues.',
    baseUrlRequired: true,
    baseUrlDefault: null,
    baseUrlPlaceholder: 'https://acme.atlassian.net',
    secretDefault: 'keyring',
    authOptions: ['basic', 'bearer'],
    codeHost: false,
  },
  {
    kind: 'redmine',
    label: 'Redmine',
    blurb: 'Redmine issues with an API key.',
    baseUrlRequired: true,
    baseUrlDefault: null,
    baseUrlPlaceholder: 'https://redmine.example.org',
    secretDefault: 'keyring',
    authOptions: ['api_key'],
    codeHost: false,
  },
  {
    kind: 'github',
    label: 'GitHub',
    blurb: 'GitHub issues, Projects and pull requests (github.com or Enterprise).',
    baseUrlRequired: false,
    baseUrlDefault: 'https://api.github.com',
    baseUrlPlaceholder: 'https://api.github.com',
    secretDefault: 'gh-cli',
    authOptions: ['token'],
    codeHost: true,
  },
  {
    kind: 'gitlab',
    label: 'GitLab',
    blurb: 'GitLab issues and merge requests (gitlab.com or self-managed).',
    baseUrlRequired: false,
    baseUrlDefault: 'https://gitlab.com',
    baseUrlPlaceholder: 'https://gitlab.com',
    secretDefault: 'glab-cli',
    authOptions: ['token'],
    codeHost: true,
  },
  {
    kind: 'bitbucket',
    label: 'Bitbucket',
    blurb: 'Bitbucket Cloud pull requests (its issue tracker was removed; use Jira).',
    baseUrlRequired: false,
    baseUrlDefault: 'https://api.bitbucket.org/2.0',
    baseUrlPlaceholder: 'https://api.bitbucket.org/2.0',
    secretDefault: 'keyring',
    authOptions: ['basic', 'bearer'],
    codeHost: true,
  },
  {
    kind: 'gitea',
    label: 'Gitea / Forgejo',
    blurb: 'Gitea or Forgejo issues and pull requests (self-hosted, Codeberg).',
    baseUrlRequired: true,
    baseUrlDefault: null,
    baseUrlPlaceholder: 'https://git.example.org',
    secretDefault: 'keyring',
    authOptions: ['token'],
    codeHost: true,
  },
  {
    kind: 'linear',
    label: 'Linear',
    blurb: 'Linear issues with a personal API key.',
    baseUrlRequired: false,
    baseUrlDefault: 'https://api.linear.app',
    baseUrlPlaceholder: 'https://api.linear.app',
    secretDefault: 'keyring',
    authOptions: [],
    codeHost: false,
  },
];

export function kindInfo(kind: AccountKind): KindInfo {
  return KINDS.find((k) => k.kind === kind) ?? KINDS[0]!;
}

export interface AccountDraft {
  id: string;
  kind: AccountKind;
  base_url: string;
  flavor: 'auto' | 'cloud' | 'dc';
  /** `oauth` is set by "Sign in with GitHub / GitLab", never picked by hand. */
  auth: '' | 'basic' | 'bearer' | 'api_key' | 'token' | 'oauth';
  email: string;
  user: string;
  secret: string;
  text_format: 'textile' | 'markdown';
  poll_secs: string;
  web_url: string;
}

export function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40);
}

/** Suggested id: `jira-acme` from the kind and the host of the base URL. */
export function suggestId(kind: AccountKind, baseUrl: string, taken: readonly string[]): string {
  let host: string;
  try {
    host =
      new URL(baseUrl).hostname.replace(/^(www|api|gitlab|github|redmine|jira)\./, '').split('.')[0] ?? '';
  } catch {
    host = '';
  }
  const base = slugify(host && !['com', 'org', 'io'].includes(host) ? `${kind}-${host}` : kind);
  if (!taken.includes(base)) return base;
  for (let i = 2; i < 100; i++) if (!taken.includes(`${base}-${i}`)) return `${base}-${i}`;
  return base;
}

export function emptyDraft(kind: AccountKind): AccountDraft {
  const info = kindInfo(kind);
  return {
    id: '',
    kind,
    base_url: info.baseUrlDefault ?? '',
    flavor: 'auto',
    auth: '',
    email: '',
    user: '',
    secret: info.secretDefault === 'keyring' ? '' : info.secretDefault,
    text_format: 'textile',
    poll_secs: '',
    web_url: '',
  };
}

const ID_RE = /^[a-z0-9-]+$/;

export function validateDraft(
  d: AccountDraft,
  taken: readonly string[],
  editing = false,
): Record<string, string> {
  const errors: Record<string, string> = {};
  if (!d.id) errors.id = 'Choose an id';
  else if (!ID_RE.test(d.id)) errors.id = 'Use lowercase letters, digits and dashes';
  else if (!editing && taken.includes(d.id)) errors.id = 'This id is already used';
  const info = kindInfo(d.kind);
  if (info.baseUrlRequired && !d.base_url.trim()) errors.base_url = 'The server URL is required';
  if (d.base_url.trim() && !/^https?:\/\/\S+$/.test(d.base_url.trim())) {
    errors.base_url = 'Enter a full URL starting with http:// or https://';
  }
  if (d.web_url.trim() && !/^https?:\/\/\S+$/.test(d.web_url.trim())) {
    errors.web_url = 'Enter a full URL starting with http:// or https://';
  }
  if (d.kind === 'jira' && needsEmail(d) && !d.email.trim())
    errors.email = 'Jira Cloud needs the account e-mail';
  if (d.kind === 'bitbucket' && needsEmail(d) && !d.email.trim())
    errors.email = 'Bitbucket API tokens need the Atlassian account e-mail';
  if (d.poll_secs.trim() && !/^\d+$/.test(d.poll_secs.trim())) errors.poll_secs = 'Whole seconds';
  if (!d.secret.trim()) errors.secret = 'Choose where the token comes from';
  return errors;
}

/** Jira Cloud (`*.atlassian.net` or flavor=cloud) and Bitbucket API tokens authenticate with e-mail + token. */
export function needsEmail(d: AccountDraft): boolean {
  // Bitbucket: API token = e-mail + token (Basic); access tokens are Bearer.
  if (d.kind === 'bitbucket') return d.auth !== 'bearer';
  if (d.kind !== 'jira') return false;
  if (d.flavor === 'cloud') return true;
  return d.flavor === 'auto' && /atlassian\.net/.test(d.base_url);
}

/** The value stored at `accounts.<id>` (defaults and empty fields omitted). */
export function buildAccount(d: AccountDraft): JsonValue {
  const info = kindInfo(d.kind);
  const out: { [k: string]: JsonValue } = { kind: d.kind };
  const url = d.base_url.trim().replace(/\/+$/, '');
  if (url && url !== info.baseUrlDefault) out.base_url = url;
  if (d.kind === 'jira' && d.flavor !== 'auto') out.flavor = d.flavor;
  if (d.auth) out.auth = d.auth;
  if (d.email.trim()) out.email = d.email.trim();
  if (d.user.trim()) out.user = d.user.trim();
  if (d.secret.trim() && d.secret.trim() !== info.secretDefault) out.secret = d.secret.trim();
  if (d.kind === 'redmine' && d.text_format !== 'textile') out.text_format = d.text_format;
  if (d.poll_secs.trim()) out.poll_secs = Number(d.poll_secs.trim());
  if (d.web_url.trim()) out.web_url = d.web_url.trim();
  return out;
}

/** Host `oauth.client_ids` is keyed by, for kinds with browser sign-in (mirrors kelta-http `oauth::client_host`). */
export function oauthHost(kind: AccountKind, baseUrl: string): string | null {
  if (kind !== 'github' && kind !== 'gitlab') return null;
  try {
    const host = new URL(baseUrl.trim() || kindInfo(kind).baseUrlDefault!).hostname;
    return host === 'api.github.com' ? 'github.com' : host;
  } catch {
    return null;
  }
}

/** Where browser sign-in stores the token: the draft's own keyring:/file: ref, else `keyring:<id>`. */
export function oauthSecretRef(d: AccountDraft): string {
  return /^(keyring|file):./.test(d.secret) ? d.secret : `keyring:${d.id}`;
}

export interface SecretAdvice {
  title: string;
  steps: string[];
  snippets: string[];
}

/** What to show when a secret backend is unavailable (SPEC §5 "Secret Service missing"). */
export function backendAdvice(backend: string): SecretAdvice | null {
  switch (backend) {
    case 'secret-service':
      return {
        title: 'No Secret Service provider',
        steps: [
          'On GNOME / KDE the keyring starts with your session. On Sway or Hyprland nothing provides it by default.',
          'Start gnome-keyring for secrets, or enable Secret Service integration in KeePassXC (Settings → Secret Service Integration).',
          'Or avoid the keyring: store tokens in the passphrase-encrypted file (file: references), or use a command: reference (pass, op, secret-tool) or an env: reference.',
        ],
        snippets: [
          'gnome-keyring-daemon --start --components=secrets',
          'command:pass show jira/acme',
          'command:secret-tool lookup service kelta account jira',
          'env:JIRA_TOKEN',
        ],
      };
    case 'gh-cli':
      return {
        title: 'GitHub CLI not found',
        steps: [
          'Install gh, then run `gh auth login`. Kelta reads the token with `gh auth token` and never copies it.',
        ],
        snippets: ['gh auth login'],
      };
    case 'glab-cli':
      return {
        title: 'GitLab CLI not found',
        steps: ['Install glab, then run `glab auth login`. Kelta reads glab’s own config.'],
        snippets: ['glab auth login'],
      };
    default:
      return null;
  }
}
