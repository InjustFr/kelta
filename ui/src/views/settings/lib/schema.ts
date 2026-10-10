// Schema-driven form model: turns the flattened settings JSON Schema (`schema/settings.schema.json`,
// annotated with `x-kelta-*`) into field kinds. No per-key code lives in the renderer; the only
// key-aware tables here are presentation hints (template placeholders, acronyms).

import type { JsonValue } from '$lib/gen';

import { getAt, isRecord, joinPath } from './paths';

export interface SchemaNode {
  type?: string | string[];
  properties?: Record<string, SchemaNode>;
  additionalProperties?: boolean | SchemaNode;
  items?: SchemaNode;
  enum?: (string | null)[];
  enumDescriptions?: string[];
  anyOf?: SchemaNode[];
  oneOf?: SchemaNode[];
  description?: string;
  title?: string;
  default?: JsonValue;
  minimum?: number;
  maximum?: number;
  format?: string;
  required?: string[];
  $ref?: string;
  'x-kelta-category'?: string;
  'x-kelta-order'?: number;
  'x-kelta-scope'?: string[];
  'x-kelta-secret'?: boolean;
  'x-kelta-restart'?: boolean;
  'x-kelta-exec'?: boolean;
  'x-kelta-merge'?: string;
}

export type FieldKind =
  | 'bool'
  | 'enum'
  | 'int'
  | 'float'
  | 'string'
  | 'secret'
  | 'string-list'
  | 'string-map'
  | 'enum-map'
  | 'list-map'
  | 'object-map'
  | 'keyed-list'
  | 'group'
  | 'variant'
  | 'json';

export interface Resolved {
  node: SchemaNode;
  nullable: boolean;
}

const X_KEYS = [
  'x-kelta-category',
  'x-kelta-order',
  'x-kelta-scope',
  'x-kelta-secret',
  'x-kelta-restart',
  'x-kelta-exec',
  'x-kelta-merge',
] as const;

/** Removes the `null` alternative of an optional field (`T | null`). */
export function resolve(node: SchemaNode): Resolved {
  let nullable = false;
  let cur: SchemaNode = node;
  if (Array.isArray(cur.type)) {
    const types = cur.type.filter((t) => t !== 'null');
    nullable = types.length !== cur.type.length;
    cur = { ...cur, type: types.length === 1 ? types[0] : types };
  }
  if (cur.enum?.includes(null)) {
    nullable = true;
    cur = { ...cur, enum: cur.enum.filter((e) => e !== null) };
  }
  const alts = cur.anyOf ?? cur.oneOf;
  if (alts) {
    const nonNull = alts.filter((a) => a.type !== 'null');
    if (nonNull.length !== alts.length) nullable = true;
    if (nonNull.length === 1) {
      const only = nonNull[0]!;
      const merged: SchemaNode = { ...only };
      if (cur.description && !merged.description) merged.description = cur.description;
      if (cur.default !== undefined) merged.default = cur.default;
      for (const k of X_KEYS) if (cur[k] !== undefined) (merged as Record<string, unknown>)[k] = cur[k];
      const inner = resolve(merged);
      return { node: inner.node, nullable: nullable || inner.nullable };
    }
    cur = { ...cur, anyOf: nonNull, oneOf: undefined };
  }
  return { node: cur, nullable };
}

function valueSchema(node: SchemaNode): SchemaNode | null {
  const ap = node.additionalProperties;
  return ap && typeof ap === 'object' ? resolve(ap).node : null;
}

function isVariant(node: SchemaNode): boolean {
  const alts = node.anyOf ?? node.oneOf;
  return (
    !!alts &&
    alts.length > 1 &&
    alts.every((a) => {
      const r = resolve(a).node;
      return r.type === 'object' && Object.keys(r.properties ?? {}).length === 1;
    })
  );
}

export function fieldKind(input: SchemaNode): FieldKind {
  const node = resolve(input).node;
  if (node.$ref) return 'json';
  if (isVariant(node)) return 'variant';
  if (node.enum && node.enum.length > 0) return 'enum';
  const t = Array.isArray(node.type) ? node.type[0] : node.type;
  switch (t) {
    case 'boolean':
      return 'bool';
    case 'integer':
      return 'int';
    case 'number':
      return 'float';
    case 'string':
      return node['x-kelta-secret'] ? 'secret' : 'string';
    case 'array': {
      const items = node.items ? resolve(node.items).node : null;
      if (node['x-kelta-merge'] === 'by_id' && items?.type === 'object') return 'keyed-list';
      if (items && (items.type === 'string' || items.enum)) return 'string-list';
      return 'json';
    }
    case 'object': {
      if (node.properties && Object.keys(node.properties).length > 0) return 'group';
      const v = valueSchema(node);
      if (!v) return 'json';
      if (v.$ref) return 'json';
      if (v.enum) return 'enum-map';
      if (v.type === 'string') return 'string-map';
      if (v.type === 'array') {
        const it = v.items ? resolve(v.items).node : null;
        return it?.type === 'string' ? 'list-map' : 'json';
      }
      if (v.type === 'object' && v.properties) return 'object-map';
      return 'json';
    }
    default:
      return 'json';
  }
}

/** Child property schemas of a group, scalars first, then nested structures; each alphabetical. */
export function childEntries(node: SchemaNode): [string, SchemaNode][] {
  const props = resolve(node).node.properties ?? {};
  const weight = (n: SchemaNode): number => {
    const k = fieldKind(n);
    return k === 'group' || k === 'object-map' || k === 'keyed-list' ? 1 : 0;
  };
  return Object.entries(props).sort(([ka, a], [kb, b]) => weight(a) - weight(b) || ka.localeCompare(kb));
}

/** Schema of a map's values / a list's items. */
export function entrySchema(node: SchemaNode): SchemaNode | null {
  const r = resolve(node).node;
  if (r.items) return resolve(r.items).node;
  return valueSchema(r);
}

/** Variant alternatives: `[propertyName, propertySchema]` per alternative. */
export function variantOptions(node: SchemaNode): [string, SchemaNode][] {
  const r = resolve(node).node;
  return (r.anyOf ?? r.oneOf ?? []).flatMap((a) => {
    const props = resolve(a).node.properties ?? {};
    const first = Object.entries(props)[0];
    return first ? [first] : [];
  });
}

export interface EnumOption {
  value: string;
  label: string;
  description?: string;
}

export function enumOptions(node: SchemaNode): EnumOption[] {
  const r = resolve(node).node;
  const values = (r.enum ?? []).filter((v): v is string => typeof v === 'string');
  return values.map((value, i) => ({
    value,
    label: value[0]!.toUpperCase() + value.slice(1),
    description: r.enumDescriptions?.[i],
  }));
}

/** UI wording for settings whose generated title or doc comment speaks implementation. */
export const COPY: Record<string, { label?: string; help?: string }> = {
  'app.restore_mode': {
    label: 'Reopen sessions on launch',
    help: 'Lazy reopens each session when you first look at it, eager reopens all of them at startup, none starts empty.',
  },
  'app.confirm_quit_with_running': { label: 'Ask before quitting while sessions run' },
};

const ACRONYMS: Record<string, string> = {
  mcp: 'MCP',
  osc52: 'OSC 52',
  ide: 'IDE',
  url: 'URL',
  id: 'ID',
  ci: 'CI',
  pr: 'PR',
  hud: 'HUD',
  gdk: 'GDK',
  nvidia: 'NVIDIA',
  dmabuf: 'DMA-BUF',
  ms: '(ms)',
  secs: '(s)',
  mb: '(MB)',
  jql: 'JQL',
  api: 'API',
  tui: 'TUI',
  regex: 'regex',
};

/** `max_live_views` → "Max live views". */
export function titleOf(key: string): string {
  const words = key.split(/[_\-\s]+/).filter(Boolean);
  const out = words.map((w, i) => ACRONYMS[w] ?? (i === 0 ? w[0]!.toUpperCase() + w.slice(1) : w));
  return out.join(' ');
}

export function descriptionOf(node: SchemaNode): string {
  return (resolve(node).node.description ?? node.description ?? '').trim();
}

// ---- annotations ------------------------------------------------------------------------------

export interface PathInfo {
  scope: string[];
  exec: boolean;
  restart: boolean;
  secret: boolean;
  byId: boolean;
  known: boolean;
}

/** Child schema of `node` for a path segment (property, map value, or union member). */
function childOf(node: SchemaNode, seg: string): SchemaNode | null {
  const direct = node.properties?.[seg];
  if (direct) return direct;
  const v = node.additionalProperties;
  if (v && typeof v === 'object') return v;
  for (const alt of node.anyOf ?? node.oneOf ?? []) {
    const c = childOf(alt, seg);
    if (c) return c;
  }
  return null;
}

/** Annotation lookup mirroring `kelta_config::schema::SchemaIndex::info`. */
export function infoAt(root: SchemaNode, segs: readonly string[]): PathInfo {
  let scope = ['global', 'project'];
  let exec = false;
  let restart = false;
  let secret = false;
  let byId = false;
  let cur = root;
  for (const seg of segs) {
    const next = childOf(cur, seg);
    if (!next) return { scope, exec, restart, secret: false, byId: false, known: false };
    cur = next;
    if (cur['x-kelta-scope']) scope = cur['x-kelta-scope'];
    exec ||= !!cur['x-kelta-exec'];
    restart ||= !!cur['x-kelta-restart'];
    secret = !!cur['x-kelta-secret'];
    byId = cur['x-kelta-merge'] === 'by_id';
  }
  return { scope, exec, restart, secret, byId, known: true };
}

export function nodeAt(root: SchemaNode, segs: readonly string[]): SchemaNode | null {
  let cur = root;
  for (const seg of segs) {
    const next = childOf(cur, seg);
    if (!next) return null;
    cur = next;
  }
  return cur;
}

/** Repo-local allowed keys (SETTINGS §4); mirrors `kelta_proto::settings::REPO_ALLOWED_KEYS`. */
export const REPO_ALLOWED_KEYS: readonly string[] = [
  'tools',
  'triggers',
  'session_templates',
  'commands',
  'worktree.include',
  'worktree.setup',
  'worktree.setup_blocking',
  'worktree.branch_template',
  'env',
  'claude.append_system_prompt',
  'editor.review_args',
];

/** Repo-local keys that stay inert until trusted (`REPO_EXEC_KEYS`). */
export const REPO_EXEC_KEYS: readonly string[] = ['tools', 'triggers', 'commands', 'worktree.setup', 'env'];

export function repoAllows(path: string): boolean {
  return REPO_ALLOWED_KEYS.some((k) => path === k || path.startsWith(`${k}.`));
}

/** Allowed at the repo layer, or a parent of an allowed key (shown as a heading). */
export function repoRelevant(path: string): boolean {
  return repoAllows(path) || REPO_ALLOWED_KEYS.some((k) => k.startsWith(`${path}.`));
}

// ---- template hints ---------------------------------------------------------------------------

const TEMPLATE_PATTERNS: RegExp[] = [
  /^worktree\.(root|branch_template)$/,
  /^work\.on_(start|pr)\.comment$/,
  /^work\.pr\.(title_template|body_template)$/,
  /^claude\.prompt_templates\./,
  /^session_templates\.\d+\.layout/,
];

export const TEMPLATE_HELP =
  'Placeholders: {project.id|name|root} {repo.id|path|name} {worktree} {branch} {base} {key} {slug} {type} ' +
  '{ticket.key|title|url|file} {pr.url|number|head|base|title} {session.id|name|cwd} {sid8} {run} {port} ' +
  '{config_dir} {data_dir} {home} {user}. Filters: |slug |shell |json; {a|b} = first non-empty.';

export function isTemplatePath(path: string): boolean {
  return TEMPLATE_PATTERNS.some((p) => p.test(path));
}

// ---- traversal --------------------------------------------------------------------------------

export interface LeafField {
  segs: string[];
  path: string;
  node: SchemaNode;
  kind: FieldKind;
}

/** Flattens `roots` (top-level keys) into editable fields; groups are expanded. */
export function leafFields(schema: SchemaNode, roots: readonly string[]): LeafField[] {
  const out: LeafField[] = [];
  const visit = (segs: string[], node: SchemaNode): void => {
    const kind = fieldKind(node);
    if (kind === 'group') {
      for (const [k, child] of childEntries(node)) visit([...segs, k], child);
      return;
    }
    out.push({ segs, path: joinPath(segs), node, kind });
  };
  for (const root of roots) {
    const node = schema.properties?.[root];
    if (node) visit([root], node);
  }
  return out;
}

/** Default value of a field (schema default, or an empty value of its kind). */
export function defaultOf(node: SchemaNode): JsonValue {
  const r = resolve(node).node;
  if (r.default !== undefined) return r.default;
  switch (fieldKind(node)) {
    case 'bool':
      return false;
    case 'int':
    case 'float':
      return r.minimum ?? 0;
    case 'string':
    case 'secret':
      return '';
    case 'enum':
      return enumOptions(node)[0]?.value ?? '';
    case 'string-list':
    case 'keyed-list':
      return [];
    case 'group':
    case 'string-map':
    case 'enum-map':
    case 'list-map':
    case 'object-map':
      return {};
    default:
      return null;
  }
}

/** Does `value` look like an object path container (helper for sources lookups). */
export function hasChildren(value: JsonValue | undefined): boolean {
  return isRecord(value) && Object.keys(value).length > 0;
}

export { getAt };
