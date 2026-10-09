// Renders each <section class="frame"> of mockups.html to <id>.png. Run: node render.mjs (from ui/ deps).
import { chromium } from '../../../ui/node_modules/@playwright/test/index.mjs';
import { fileURLToPath } from 'node:url';
const dir = fileURLToPath(new URL('.', import.meta.url));
const b = await chromium.launch();
const p = await b.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
await p.goto('file://' + dir + 'mockups.html');
for (const id of await p.$$eval('section.frame', (s) => s.map((e) => e.id))) {
  await p.locator('#' + id).screenshot({ path: dir + id + '.png' });
  console.log(id);
}
await b.close();
