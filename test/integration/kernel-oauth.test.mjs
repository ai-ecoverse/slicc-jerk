import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createNodeKernel } from '@ai-ecoverse/slicc-kernel/node';
import { stubCdp } from './cdp-stub.mjs';
import { wasm } from './helpers.mjs';

const skip = !existsSync(wasm) && 'run npm run build:wasm first';
const placeholder = 'fnox_proxy_333333333333333333333333';
const fake = 'access-FAKE-1234567890';
const refresh = 'refresh-FAKE-1234567890';
const callback = 'http://127.0.0.1:43819/callback';

function bridge(wsUrl) {
  return async () => {
    const ws = new WebSocket(wsUrl);
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
  };
}

async function seven(mode, options = {}) {
  const posts = [];
  const seals = [];
  let polls = 0;
  const kernel = await createNodeKernel({
    ...(options.browser ? { cdp: bridge(options.browser.wsUrl) } : {}),
    sealed: async (req) => {
      assert.equal(req.op, 'seal');
      seals.push(req);
      return options.refused
        ? { v: 1, error: { code: 'EEXIST', message: fake } }
        : { v: 1, placeholder, state: 'inert' };
    },
    network: {
      transport: {
        traits: { manualRedirects: true, encodedBodies: false, maxRequestBody: 1024 * 1024 },
        async fetch(req) {
          posts.push({
            url: req.url,
            method: req.method,
            body: new TextDecoder().decode(req.body),
          });
          let response;
          let status = 200;
          if (req.url.endsWith('/device')) {
            response = {
              device_code: 'device-FAKE',
              user_code: 'CODE-FAKE',
              verification_uri: 'https://idp.example.test/verify',
              expires_in: 30,
              interval: 0,
            };
          } else if (options.denied) {
            status = 400;
            response = { error: 'access_denied', error_description: fake };
          } else if (mode === 'device' && polls++ === 0) {
            status = 400;
            response = { error: 'authorization_pending' };
          } else {
            response = {
              access_token: fake,
              refresh_token: refresh,
              token_type: 'Bearer',
              expires_in: options.expired ? 0 : 3600,
            };
          }
          return {
            status,
            statusText: 'stub',
            headers: [['content-type', 'application/json']],
            body: (async function* () {
              yield new TextEncoder().encode(JSON.stringify(response));
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
  await kernel.writeFile(
    '/tmp/providers.toml',
    `[providers.demo]
env = ["DEMO_TOKEN"]
[providers.demo.oauth]
mode = "${mode}"
flow = "pkce"
authorize_url = "https://idp.example.test/${mode === 'device' ? 'device' : 'authorize?redirect_uri={redirect_uri}&state={state}&code_challenge={code_challenge}'}"
token_url = "https://idp.example.test/token"
client_id = "fake-client"
redirect_uri = "${callback}"
capture = { at = "request", from = "query", param = "code" }
timeout = "3s"
`
  );
  const run = (...args) =>
    kernel.run(['jerk', 'demo', ...args], {
      env: { SLICC_JERK_CONFIG: '/tmp/providers.toml', SLICC_JERK_STATE_DIR: '/tmp/oauth-state' },
    });
  return { kernel, posts, seals, run };
}

test('kernel device flow uses outbound HTTPS and persists only a sealed grant', {
  skip,
}, async () => {
  const { kernel, posts, seals, run } = await seven('device');
  try {
    const created = await run('create');
    assert.equal(created.status, 0, created.stderr);
    assert.equal(created.stdout, 'demo: signed in\n');
    assert.match(created.stderr, /CODE-FAKE/);
    assert.doesNotMatch(created.stdout + created.stderr, /access-FAKE|refresh-FAKE/);
    assert.equal(posts.length, 3);
    assert.equal(seals[0].name, 'DEMO_TOKEN');
    assert.equal(seals[0].value, fake);
    const tmp = await kernel.root.getDirectoryHandle('tmp');
    const state = await tmp.getDirectoryHandle('oauth-state');
    const grants = await state.getDirectoryHandle('grants');
    for await (const handle of grants.values()) {
      const metadata = await (await handle.getFile()).text();
      assert.doesNotMatch(metadata, /access-FAKE|refresh-FAKE/);
      assert.match(metadata, /fnox_proxy_/);
    }
    const get = await run('get', '--json', '--reveal');
    assert.equal(get.status, 0, get.stderr);
    assert.deepEqual(JSON.parse(get.stdout), { provider: 'demo', source: 'oauth', placeholder });
    assert.equal(
      (await run('create')).stdout,
      'demo: already signed in; use --force to sign in again\n'
    );
    assert.equal(posts.length, 3);
  } finally {
    kernel.terminate();
  }
});

for (const mode of ['loopback', 'auto', 'intercept']) {
  test(`kernel OAuth ${mode} captures the browser redirect without a kernel listener`, {
    skip,
  }, async () => {
    const browser = await stubCdp({
      paused: (authorize) => {
        const state = new URL(authorize).searchParams.get('state');
        return [
          {
            request: {
              url: `${callback}?code=fake-code&state=${state}`,
              method: 'GET',
              headers: {},
            },
          },
        ];
      },
    });
    const { kernel, posts, run } = await seven(mode, { browser });
    try {
      const res = await run('create');
      assert.equal(res.status, 0, res.stderr);
      assert.equal(res.stdout, 'demo: signed in\n');
      assert.doesNotMatch(res.stdout + res.stderr, /access-FAKE|refresh-FAKE|fake-code/);
      const form = new URLSearchParams(posts[0].body);
      assert.equal(form.get('redirect_uri'), callback);
      assert.equal(form.get('code'), 'fake-code');
      assert.ok(form.get('code_verifier'));
      assert.match(browser.log(), /Fetch.fulfillRequest|Fetch.failRequest/);
      assert.match(browser.log(), /Target.closeTarget/);
      assert.equal((await run('get')).stdout, `${placeholder}\n`);
    } finally {
      kernel.terminate();
      await browser.close();
    }
  });
}

for (const options of [{ refused: true }, { denied: true }, { expired: true }]) {
  test(`kernel OAuth fails safely: ${JSON.stringify(options)}`, { skip }, async () => {
    const { kernel, run } = await seven('device', options);
    try {
      const res = await run('create', '--force');
      assert.equal(res.status, options.expired ? 0 : 1, res.stderr);
      assert.doesNotMatch(res.stdout + res.stderr, /access-FAKE|refresh-FAKE/);
      const get = await run('get', '--reveal');
      assert.equal(get.status, 3, get.stderr);
      assert.equal(get.stdout, '');
    } finally {
      kernel.terminate();
    }
  });
}
