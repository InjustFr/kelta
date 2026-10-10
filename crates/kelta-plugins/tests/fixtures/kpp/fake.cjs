// Misbehaving KPP provider for the runtime tests; `account.user` picks the behaviour of tracker.me.
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
