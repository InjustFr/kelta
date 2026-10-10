// KPP example: a tracker served from a JSON file. One JSON-RPC 2.0 message per line on stdio.
// Every call carries {account_id, account, secret}; this tracker needs no secret.
'use strict';
const fs = require('fs');
const path = require('path');
const readline = require('readline');

const file = (account) => account.base_url || path.join(__dirname, 'tickets.json');
const load = (account) => JSON.parse(fs.readFileSync(file(account), 'utf8'));
const save = (account, db) => fs.writeFileSync(file(account), JSON.stringify(db, null, 2) + '\n');

const esc = (s) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
const user = (db, id) => (id === db.me.id ? db.me : { id, name: id, login: id });
const status = (db, id) => db.statuses.find((s) => s.id === id) || { id, name: id, category: 'unknown' };
const find = (db, ref) => {
  const t = db.tickets.find((t) => t.key === ref.key);
  if (!t) throw Object.assign(new Error(`no ticket ${ref.key}`), { data: { code: 'not_found' } });
  return t;
};
const ticket = (db, p, t) => ({
  ref: { account: p.account_id, key: t.key, id: t.key },
  title: t.title,
  url: `file://${file(p.account)}#${t.key}`,
  status: status(db, t.status),
  assignee: t.assignee ? user(db, t.assignee) : null,
  labels: [],
  updated_at: t.updated_at,
  project_hint: t.key.split('-')[0],
});
// Read, change, write back, return the changed ticket.
const update = (p, change) => {
  const db = load(p.account);
  const t = find(db, p.ticket);
  change(db, t);
  t.updated_at = new Date().toISOString();
  save(p.account, db);
  return ticket(db, p, t);
};

const methods = {
  'tracker.me': (p) => load(p.account).me,
  'tracker.list': (p) => {
    const db = load(p.account);
    return { items: db.tickets.map((t) => ticket(db, p, t)), next: null };
  },
  'tracker.get': (p) => {
    const db = load(p.account);
    const t = find(db, p.ticket);
    return {
      ticket: ticket(db, p, t),
      body_md: t.body,
      body_html: '', // Kelta renders body_md
      body_format: 'markdown',
      comments: t.comments.slice(-20).map((c) => ({
        author: user(db, c.author),
        created_at: c.created_at,
        body_html: `<p>${esc(c.body)}</p>`,
      })),
      parent: null,
    };
  },
  'tracker.columns': (p) =>
    load(p.account).statuses.map((s, i) => ({ id: s.id, name: s.name, category: s.category, order: i, match_names: [s.name] })),
  'tracker.transitions': (p) =>
    load(p.account).statuses.map((s) => ({ id: s.id, name: `Move to ${s.name}`, to: s, needs_fields: false })),
  'tracker.transition': (p) => update(p, (_db, t) => (t.status = p.transition_id)),
  'tracker.comment': (p) => {
    update(p, (db, t) => t.comments.push({ author: db.me.id, created_at: new Date().toISOString(), body: p.markdown }));
    return null;
  },
  'tracker.assign': (p) =>
    update(p, (db, t) => (t.assignee = p.who.kind === 'me' ? db.me.id : p.who.kind === 'user' ? p.who.id : null)),
};

const send = (msg) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...msg }) + '\n');
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  let req;
  try {
    req = JSON.parse(line);
  } catch {
    return send({ id: null, error: { code: -32700, message: 'parse error' } });
  }
  const fn = methods[req.method];
  if (!fn) return send({ id: req.id, error: { code: -32601, message: `unknown method ${req.method}` } });
  try {
    send({ id: req.id, result: fn(req.params) });
  } catch (e) {
    send({ id: req.id, error: { code: -32000, message: e.message, data: e.data } });
  }
});
