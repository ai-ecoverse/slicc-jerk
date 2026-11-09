import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createNodeKernel } from '@ai-ecoverse/slicc-kernel/node';
import { stubCdp } from './cdp-stub.mjs';
import { wasm } from './helpers.mjs';

const skip = !existsSync(wasm) && 'run npm run build:wasm first';
const code = 'fake-captured-code';
const token = 'fake-exchanged-token';
const placeholder = 'fnox_proxy_444444444444444444444444';
const callback = 'http://127.0.0.1:43991/callback';
const args = [
  'intercept',
  '--authorize-url',
  'https://idp.example.test/authorize',
  '--redirect',
  callback,
];

async function seven(browser, refused = false) {
  const seals = [];
  const posts = [];
  const kernel = await createNodeKernel({
    cdp: async () => {
      const ws = new WebSocket(browser.wsUrl);
      await new Promise((resolve, reject) => {
        ws.addEventListener('open', resolve, { once: true });
        ws.addEventListener('error', reject, { once: true });
      });
      const conn = {
        onmessage: null,
        onclose: null,
        send: (s) => ws.send(s),
        close: () => ws.close(),
      };
      ws.addEventListener('message', (event) => conn.onmessage?.(String(event.data)));
      ws.addEventListener('close', () => conn.onclose?.());
      return conn;
    },
    sealed: async (req) => {
      seals.push(req);
      return refused
        ? { v: 1, error: { code: 'EEXIST', message: token } }
        : { v: 1, placeholder, state: 'inert' };
    },
    network: {
      transport: {
        traits: { manualRedirects: true, encodedBodies: false, maxRequestBody: 1048576 },
        async fetch(req) {
          posts.push({ url: req.url, body: new TextDecoder().decode(req.body) });
          return {
            status: 200,
            statusText: 'OK',
            headers: [['content-type', 'application/json']],
            body: (async function* () {
              yield new TextEncoder().encode(
                JSON.stringify({ access_token: token, token_type: 'Bearer' })
              );
            })(),
            cancel: async () => {},
          };
        },
      },
    },
  });
  const pkg = '/node_modules/@ai-ecoverse/slicc-jerk';
  await kernel.writeFile(
    `${pkg}/package.json`,
    JSON.stringify({
      name: '@ai-ecoverse/slicc-jerk',
      slicc: { abi: 'wasi', commands: { jerk: { wasm: 'jerk.wasm' } } },
    })
  );
  await kernel.writeFile(`${pkg}/jerk.wasm`, readFileSync(wasm));
  return { kernel, seals, posts };
}

for (const exchange of [false, true]) {
  test(`kernel intercept captures through SLICC_CDP_URL and seals ${exchange ? 'the exchanged token' : 'the code'}`, {
    skip,
  }, async () => {
    const browser = await stubCdp({
      paused: [{ request: { url: `${callback}?code=${code}`, method: 'GET', headers: {} } }],
    });
    const { kernel, seals, posts } = await seven(browser);
    try {
      const extra = exchange
        ? [
            '--exchange',
            '--token-url',
            'https://idp.example.test/token',
            '--client-id',
            'fake-client',
          ]
        : ['--cdp', '9222'];
      const res = await kernel.run(['jerk', ...args, ...extra]);
      assert.equal(res.status, 0, res.stderr);
      assert.equal(res.stdout, `${placeholder}\n`);
      assert.doesNotMatch(res.stdout + res.stderr, /fake-captured-code|fake-exchanged-token/);
      assert.equal(seals[0].op, 'seal');
      assert.equal(seals[0].name, 'JERK_INTERCEPT_TOKEN');
      assert.equal(seals[0].value, exchange ? token : code);
      if (exchange) assert.equal(new URLSearchParams(posts[0].body).get('code'), code);
      assert.match(browser.log(), /Fetch.fulfillRequest|Fetch.failRequest/);
      assert.match(browser.log(), /Target.closeTarget/);
    } finally {
      kernel.terminate();
      await browser.close();
    }
  });
}

test('kernel intercept fails closed when sealing is refused', { skip }, async () => {
  const browser = await stubCdp({
    paused: [{ request: { url: `${callback}?code=${code}`, method: 'GET', headers: {} } }],
  });
  const { kernel } = await seven(browser, true);
  try {
    const res = await kernel.run(['jerk', ...args]);
    assert.equal(res.status, 1, res.stderr);
    assert.equal(res.stdout, '');
    assert.match(res.stderr, /EEXIST/);
    assert.doesNotMatch(res.stderr, /fake-captured-code|fake-exchanged-token/);
  } finally {
    kernel.terminate();
    await browser.close();
  }
});

test('kernel intercept reports secure CDP URLs as unavailable rather than unsupported', {
  skip,
}, async () => {
  const browser = await stubCdp({ paused: [] });
  const { kernel } = await seven(browser);
  try {
    const res = await kernel.run(['jerk', ...args, '--cdp', 'wss://cdp.example.test/browser']);
    assert.equal(res.status, 4, res.stderr);
    assert.match(res.stderr, /wss and https are not available/);
    assert.equal(browser.log(), '');
  } finally {
    kernel.terminate();
    await browser.close();
  }
});
