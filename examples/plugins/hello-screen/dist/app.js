// Hello screen: everything goes through the permission-checked host API (no network, no browser storage).
import { connect } from './kelta-sdk.js';

const $ = (id) => document.getElementById(id);

function text(el, value) {
  el.textContent = value;
}

async function main() {
  const kelta = await connect();
  const { greeting = 'Hello' } = await kelta.settings.get();
  const info = await kelta.app.info();
  text($('greeting'), `${greeting} from Kelta ${info.version}`);

  try {
    const project = await kelta.projects.current();
    text($('project'), `Project: ${project.name} (${project.repos.length} repo(s))`);
  } catch (e) {
    text($('project'), `Project unavailable: ${e.message}`);
  }

  // Persisted per plugin by the host (`storage` permission, `kv.*`).
  try {
    const visits = ((await kelta.kv.get('visits')) ?? 0) + 1;
    await kelta.kv.set('visits', visits);
    text($('visits'), `Opened ${visits} time(s).`);
  } catch (e) {
    text($('visits'), `Storage unavailable: ${e.message}`);
  }

  const renderSessions = async () => {
    const list = $('sessions');
    list.replaceChildren();
    try {
      const sessions = await kelta.sessions.list();
      for (const s of sessions) {
        const li = document.createElement('li');
        li.textContent = `${s.name} — ${s.status}`;
        list.append(li);
      }
      if (sessions.length === 0) text(list, 'No sessions in this project.');
    } catch (e) {
      text(list, `Cannot list sessions: ${e.message}`);
    }
  };
  await renderSessions();

  kelta.events.on('session.*', (payload, name) => {
    const li = document.createElement('li');
    li.textContent = `${new Date().toLocaleTimeString()} ${name}`;
    $('events').prepend(li);
    while ($('events').childElementCount > 20) $('events').lastElementChild.remove();
    if (name === 'session.spawned' || name === 'session.exited') void renderSessions();
  });

  $('notify').addEventListener('click', async () => {
    try {
      await kelta.notify('Hello screen', `${greeting}!`);
      text($('status'), 'Sent.');
    } catch (e) {
      text($('status'), `${e.code ?? 'error'}: ${e.message}`);
    }
  });

  document.body.dataset.connected = 'true';
}

main().catch((e) => {
  text($('greeting'), `Could not connect to Kelta: ${e.message}`);
});
