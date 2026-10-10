// Misbehaving KPP provider for the runtime tests; `account.user` picks the behaviour of tracker.me.
// The codehost.* methods are well behaved except that they claim another account and send raw HTML.
'use strict';
process.stderr.write('fake provider started\n');
const send = (m) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...m }) + '\n');
require('readline')
  .createInterface({ input: process.stdin })
  .on('line', (line) => {
    const { id, method, params } = JSON.parse(line);
    if (method === 'tracker.list') {
      const ref = { account: 'someone-else', key: 'X-1', id: '1' };
      const status = { id: 's', name: 'S', category: 'todo' };
      return send({ id, result: { items: [{ ref, title: 't', url: 'u', status, updated_at: 'now' }], next: null } });
    }
    if (method.startsWith('codehost.')) return codehost(id, method, params);
    if (method !== 'tracker.me') return send({ id, error: { code: -32601, message: 'unknown method' } });
    switch (params.account.user) {
      case 'crash':
        process.exit(3);
        break;
      case 'hang':
        break;
      case 'garbage':
        process.stdout.write('this is not json\n');
        break;
      case 'denied':
        send({ id, error: { code: -32000, message: 'bad token', data: { status: 401 } } });
        break;
      default:
        send({ id, result: { id: String(process.pid), name: params.secret } });
    }
  });

// Every ref names another account and the detail carries a script: KppCodeHost must fix both.
function review(kind, number) {
  const ref = { account: 'someone-else', repo: 'acme/shop', number };
  const author = { id: '2', name: 'A', login: 'a' };
  return {
    ref, title: 't', url: 'u', author, draft: false, head_sha: 'abc', source_branch: 'feature/x',
    target_branch: 'main', ci: 'success', kind, updated_at: 'now',
  };
}

function codehost(id, method, params) {
  switch (method) {
    case 'codehost.me':
      return send({ id, result: { id: '1', name: 'Me', login: 'me' } });
    case 'codehost.list_reviews':
      return send({ id, result: [review(params.query.kind, 1)] });
    case 'codehost.get': {
      const body_html = '<p>hi</p><script>alert(1)</script>';
      return send({ id, result: { review: review('review_requested', 1), body_html, reviewers: [], checks: [], files: [] } });
    }
    case 'codehost.approve':
    case 'codehost.comment':
    case 'codehost.request_changes':
      return send({ id, result: null });
    case 'codehost.create':
      return send({ id, result: review('authored', 2) });
    case 'codehost.find_for_branch':
      return send({ id, result: review('authored', 1) });
    default: // changed_since_last included: Kelta must then assume a change
      return send({ id, error: { code: -32601, message: 'unknown method' } });
  }
}
