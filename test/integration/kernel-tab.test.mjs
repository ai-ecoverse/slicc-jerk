import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createNodeKernel } from '@ai-ecoverse/slicc-kernel/node';
import { stubBrowser } from './cdp-stub.mjs';
import { wasm } from './helpers.mjs';

const skip = !existsSync(wasm) && 'run npm run build:wasm first';
const app = 'https://app.example.test/inbox';
const appOrigin = 'https://app.example.test';

function bridge(wsUrl) {
  return async () => {
    const ws = new WebSocket(wsUrl);
    await new Promise((resolve, reject) => {
      ws.addEventListener('open', resolve, { once: true });
      ws.addEventListener('error', () => reject(new Error('stub browser refused')), {
        once: true,
      });
    });
    const conn = {
      onmessage: null,
      onclose: null,
      send: (message) => ws.send(message),
      close: () => ws.close(),
    };
    ws.addEventListener('message', (event) => conn.onmessage?.(String(event.data)));
    ws.addEventListener('close', () => conn.onclose?.());
    return conn;
  };
}

async function seven(browser) {
  const kernel = await createNodeKernel({ cdp: bridge(browser.wsUrl) });
  const pkg = '/node_modules/@ai-ecoverse/slicc-jerk';
  await kernel.writeFile(
    `${pkg}/package.json`,
    JSON.stringify({
      name: '@ai-ecoverse/slicc-jerk',
      slicc: { abi: 'wasi', commands: { jerk: { wasm: 'bin/jerk.wasm' } } },
    })
  );
  await kernel.writeFile(`${pkg}/bin/jerk.wasm`, readFileSync(wasm));
  return kernel;
}

const signedIn = () => ({
  pages: [{ targetId: 'BBBB2222', url: app, title: 'Inbox' }],
  cookies: [
    {
      name: 'sid',
      value: 'fake-session',
      domain: 'app.example.test',
      path: '/',
      expires: 2000000000,
      httpOnly: true,
      secure: true,
      session: false,
    },
  ],
  storage: { [appOrigin]: { localStorage: { auth: 'fake-local-token' }, sessionStorage: {} } },
});

test('jerk tab moves a tab between the browsers of two kernels', { skip }, async () => {
  const from = await stubBrowser(signedIn());
  const to = await stubBrowser({});
  const one = await seven(from);
  const two = await seven(to);
  try {
    const listed = await one.run(['jerk', 'tab', 'list', '--json']);
    assert.equal(listed.status, 0, listed.stderr);
    assert.deepEqual(JSON.parse(listed.stdout), [{ id: 'BBBB2222', url: app, title: 'Inbox' }]);

    const exported = await one.run(['jerk', 'tab', 'export', '--url', 'inbox']);
    assert.equal(exported.status, 0, exported.stderr);
    assert.match(exported.stderr, /exported 1 cookie and 1 storage entry from https:\/\/app/);

    const imported = await two.run(['jerk', 'tab', 'import', '--cdp', '9222'], {
      stdin: exported.stdout,
    });
    assert.equal(imported.status, 0, imported.stderr);
    assert.equal(imported.stdout, 'NEW1\n');
    assert.equal(to.state.pages.find((p) => p.targetId === 'NEW1').url, app);
    assert.deepEqual(to.state.storage[appOrigin].localStorage, { auth: 'fake-local-token' });
    assert.deepEqual(
      to.state.setCookies.map((c) => [c.name, c.url, c.httpOnly]),
      [['sid', 'https://app.example.test/', true]]
    );
  } finally {
    one.terminate();
    two.terminate();
    await from.close();
    await to.close();
  }
});

test('jerk tab in a kernel without a browser is unavailable', { skip }, async () => {
  const kernel = await createNodeKernel({});
  await kernel.writeFile(
    '/node_modules/@ai-ecoverse/slicc-jerk/package.json',
    JSON.stringify({
      name: '@ai-ecoverse/slicc-jerk',
      slicc: { abi: 'wasi', commands: { jerk: { wasm: 'bin/jerk.wasm' } } },
    })
  );
  await kernel.writeFile('/node_modules/@ai-ecoverse/slicc-jerk/bin/jerk.wasm', readFileSync(wasm));
  try {
    const res = await kernel.run(['jerk', 'tab', 'list']);
    assert.equal(res.status, 4, res.stderr);
    assert.match(res.stderr, /^jerk tab: /);
  } finally {
    kernel.terminate();
  }
});
