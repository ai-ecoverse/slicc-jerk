import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createNodeKernel } from '@ai-ecoverse/slicc-kernel/node';
import { stubCdp } from './cdp-stub.mjs';
import { wasm } from './helpers.mjs';

const skip = !existsSync(wasm) && 'run npm run build:wasm first';
const captured = 'captured-FAKE-1234567890';
const placeholder = 'fnox_proxy_0123456789abcdef01234567';

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

function vault() {
  const seen = [];
  const sealed = async (req) => {
    seen.push(req);
    if (req.op === 'seal') return { v: 1, placeholder, state: 'inert' };
    return { v: 1, error: { code: 'ENOENT' } };
  };
  return { sealed, seen };
}

async function seven(options) {
  const kernel = await createNodeKernel(options);
  const pkg = '/node_modules/@ai-ecoverse/slicc-jerk';
  await kernel.writeFile(
    `${pkg}/package.json`,
    JSON.stringify({
      name: '@ai-ecoverse/slicc-jerk',
      slicc: { abi: 'wasi', commands: { jerk: { wasm: 'bin/jerk.wasm' } } },
    })
  );
  await kernel.writeFile(`${pkg}/bin/jerk.wasm`, readFileSync(wasm));
  await kernel.writeFile(
    '/node_modules/print-cdp/package.json',
    JSON.stringify({
      name: 'print-cdp',
      slicc: { commands: { 'print-cdp': { abi: 'js', module: 'index.mjs' } } },
    })
  );
  await kernel.writeFile(
    '/node_modules/print-cdp/index.mjs',
    "export default async (ctx) => { await ctx.write(1, `${ctx.env.SLICC_CDP_URL ?? ''}\\n`); };\n"
  );
  return kernel;
}

const args = [
  'jerk',
  'intercept',
  '--authorize-url',
  'https://idp.example.test/authorize',
  '--redirect',
  'https://app.example.test/callback',
];

test('the kernel browser is plain ws on the kernel loopback', { skip }, async () => {
  const browser = await stubCdp({ paused: [] });
  const kernel = await seven({ cdp: bridge(browser.wsUrl) });
  try {
    const res = await kernel.run(['print-cdp']);
    assert.equal(res.status, 0, res.stderr);
    assert.match(res.stdout, /^ws:\/\/127\.0\.0\.1:9222\/devtools\/browser\/\S+\n$/);
  } finally {
    kernel.terminate();
    await browser.close();
  }
});

test('jerk intercept in the kernel seals the capture and prints its placeholder', {
  skip,
}, async () => {
  const browser = await stubCdp({
    paused: [
      {
        resourceType: 'Document',
        request: {
          url: `https://app.example.test/callback?code=${captured}`,
          method: 'GET',
          headers: {},
        },
      },
    ],
  });
  const { sealed, seen } = vault();
  const kernel = await seven({ cdp: bridge(browser.wsUrl), sealed });
  try {
    const res = await kernel.run([...args, '--name', 'APP_TOKEN']);
    assert.equal(res.status, 0, `${res.stderr}\n--- cdp ---\n${browser.log()}`);
    assert.equal(res.stdout, `${placeholder}\n`);
    assert.match(res.stderr, /captured \(not exchanged\), sealed in seven's vault/);
    assert.doesNotMatch(res.stdout + res.stderr, /captured-FAKE/);
    assert.match(browser.log(), /https:\/\/idp\.example\.test\/authorize/);
    const seals = seen.filter((req) => req.op === 'seal');
    assert.deepEqual(
      seals.map((req) => [req.name, req.value]),
      [['APP_TOKEN', captured]]
    );
  } finally {
    kernel.terminate();
    await browser.close();
  }
});

test('jerk intercept in a kernel without a browser is unavailable', { skip }, async () => {
  const { sealed, seen } = vault();
  const kernel = await seven({ sealed });
  try {
    const hostless = await kernel.run(args);
    assert.equal(hostless.status, 4, hostless.stderr);
    assert.match(
      hostless.stderr,
      /^jerk intercept: HTTP 503\nslicc-kernel: no CDP host is attached/
    );
    const unset = await kernel.run(args, { env: { SLICC_CDP_URL: '' } });
    assert.equal(unset.status, 4, unset.stderr);
    assert.equal(
      unset.stderr,
      'jerk intercept: this kernel offers no browser: SLICC_CDP_URL is not set\n'
    );
    assert.equal(hostless.stdout + unset.stdout, '');
    assert.deepEqual(seen, []);
  } finally {
    kernel.terminate();
  }
});
