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
  const requests = [];
  let polls = 0;
  const kernel = await createNodeKernel({
    ...(options.browser ? { cdp: bridge(options.browser.wsUrl) } : {}),
    sealed: async (req) => {
      requests.push(req);
      if (options.sealed) return options.sealed(req);
      if (req.kind === 'unseal') return { v: 1, exit: options.dropDenied ? 1 : 0 };
      if (req.jop === 'status') return { v: 1, exit: 3 };
      if (req.op === 'placeholder') return { v: 1, error: { code: 'ENOENT' } };
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
              scope: options.secretScope ? refresh : undefined,
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
      slicc: {
        abi: 'wasi',
        commands: { jerk: { wasm: 'jerk.wasm', imports: 'kernel-process.mjs' } },
      },
    })
  );
  await kernel.writeFile(`${pkg}/jerk.wasm`, readFileSync(wasm));
  await kernel.writeFile(
    `${pkg}/kernel-process.mjs`,
    readFileSync(new URL('../../npm/kernel-process.mjs', import.meta.url))
  );
  if (options.cli) {
    const minter = '/node_modules/fake-minter';
    await kernel.writeFile(
      `${minter}/package.json`,
      JSON.stringify({
        name: 'fake-minter',
        slicc: { abi: 'js', commands: { 'fake-minter': { module: 'minter.mjs' } } },
      })
    );
    await kernel.writeFile(
      `${minter}/minter.mjs`,
      `export default async function(ctx) { await ctx.fs.writeFile('/tmp/minter-call', 'called'); await ctx.write(1, '${fake}'); return 0; }`
    );
  }
  await kernel.writeFile(
    '/tmp/providers.toml',
    options.config ??
      `[providers.demo]
env = ["DEMO_TOKEN"]
${options.cli ? 'requires = "fake-minter"\ncreate = { run = ["fake-minter"], parse = "text" }' : ''}
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
      env: {
        SLICC_JERK_CONFIG: '/tmp/providers.toml',
        SLICC_JERK_STATE_DIR: '/tmp/oauth-state',
        ...options.env,
      },
    });
  return { kernel, posts, seals, run, requests };
}

test('kernel OAuth create bypasses unsealed CLI minting even with --reveal', { skip }, async () => {
  const { kernel, run, seals } = await seven('device', { cli: true });
  try {
    const res = await run('create', '--reveal');
    assert.equal(res.status, 0, res.stderr);
    assert.equal(res.stdout, 'demo: signed in\n');
    assert.doesNotMatch(res.stdout + res.stderr, /access-FAKE|refresh-FAKE/);
    assert.equal(seals.length, 1);
    await assert.rejects(kernel.readFile('/tmp/minter-call'), { code: 'ENOENT' });
  } finally {
    kernel.terminate();
  }
});

test('kernel OAuth get preserves pre-existing vault-backed sign-ins', { skip }, async () => {
  const { kernel, run, requests, posts } = await seven('device', {
    sealed: (req) => {
      assert.equal(req.jop, 'status');
      return { v: 1, exit: 0, placeholder };
    },
  });
  try {
    const res = await run('get', '--json', '--reveal');
    assert.equal(res.status, 0, res.stderr);
    assert.deepEqual(JSON.parse(res.stdout), { provider: 'demo', source: 'sealed', placeholder });
    assert.equal(requests.length, 1);
    assert.equal(posts.length, 0);
  } finally {
    kernel.terminate();
  }
});

test('kernel OAuth preserves a custom env placeholder only when the vault confirms it', {
  skip,
}, async () => {
  const custom = 'sealed:demo:placeholder';
  const { kernel, run, requests } = await seven('device', {
    env: { DEMO_TOKEN: custom },
    sealed: (req) => {
      assert.equal(req.op, 'placeholder');
      assert.equal(req.name, 'DEMO_TOKEN');
      return { v: 1, placeholder: custom, state: 'active' };
    },
  });
  try {
    const res = await run('get', '--reveal');
    assert.equal(res.status, 0, res.stderr);
    assert.equal(res.stdout, `${custom}\n`);
    assert.equal(requests.length, 1);
  } finally {
    kernel.terminate();
  }
});

for (const expired of [false, true]) {
  test(`kernel client credentials get seals before storing, expired=${expired}`, {
    skip,
  }, async () => {
    const config = `[providers.demo]
env = ["DEMO_TOKEN"]
[providers.demo.oauth]
mode = "client_credentials"
flow = "client_credentials"
client_id = "fake-client"
client_secret = "fake-secret"
token_url = "https://idp.example.test/token"
`;
    const { kernel, run, posts, seals } = await seven('client_credentials', { config, expired });
    try {
      const first = await run('get', '--reveal');
      assert.equal(first.status, 0, first.stderr);
      assert.equal(first.stdout, `${placeholder}\n`);
      const next = await run('get');
      assert.equal(next.status, 0, next.stderr);
      assert.equal(next.stdout, `${placeholder}\n`);
      assert.equal(posts.length, expired ? 2 : 1);
      assert.equal(seals.length, posts.length);
      assert.equal(new URLSearchParams(posts[0].body).get('grant_type'), 'client_credentials');
      assert.doesNotMatch(
        first.stdout + first.stderr + next.stdout + next.stderr,
        /access-FAKE|refresh-FAKE/
      );
    } finally {
      kernel.terminate();
    }
  });
}

for (const dropDenied of [false, true]) {
  test(`kernel OAuth drop invalidates its local grant only on success, denied=${dropDenied}`, {
    skip,
  }, async () => {
    const { kernel, run } = await seven('device', { dropDenied });
    try {
      assert.equal((await run('create')).status, 0);
      const dropped = await run('drop', '--yes');
      assert.equal(dropped.status, dropDenied ? 1 : 0, dropped.stderr);
      const get = await run('get', '--reveal');
      assert.equal(get.status, dropDenied ? 0 : 3, get.stderr);
      const create = await run('create');
      assert.equal(create.status, 0, create.stderr);
      assert.equal(create.stdout.includes('already signed in'), dropDenied);
    } finally {
      kernel.terminate();
    }
  });
}

for (const mode of ['loopback', 'auto']) {
  for (const wrongState of [false, true]) {
    test(`kernel ${mode} captures callback JSON with CORS and state checking, wrongState=${wrongState}`, {
      skip,
    }, async () => {
      const browser = await stubCdp({
        paused: (authorize) => {
          const state = new URL(authorize).searchParams.get('state');
          const request = (method, origin, url = callback) => ({
            request: {
              url,
              method,
              headers: { Origin: origin },
              postData: JSON.stringify({ authToken: fake, state: wrongState ? 'wrong' : state }),
            },
          });
          return [
            request('POST', 'https://evil.example.test'),
            request('POST', 'https://idp.example.test', `${callback}-wrong`),
            request('OPTIONS', 'https://idp.example.test'),
            {
              request: {
                url: callback,
                method: 'POST',
                headers: { Origin: 'https://idp.example.test' },
                postData: 'x'.repeat(65537),
              },
            },
            request('POST', 'https://idp.example.test'),
          ];
        },
      });
      const config = `[providers.demo]
env = ["DEMO_TOKEN"]
cors_origins = ["https://idp.example.test"]
[providers.demo.oauth]
mode = "${mode}"
flow = "implicit"
authorize_url = "https://idp.example.test/authorize?redirect_uri={redirect_uri}&state={state}"
client_id = "fake-client"
redirect_uri = "${callback}"
capture = { at = "callback", from = "body", param = "authToken" }
timeout = "3s"
`;
      const { kernel, run, seals } = await seven(mode, { browser, config });
      try {
        const res = await run('create');
        assert.equal(res.status, wrongState ? 1 : 0, res.stderr);
        assert.equal(seals.length, wrongState ? 0 : 1);
        if (wrongState) assert.match(res.stderr, /state mismatch/);
        else assert.equal(seals[0].value, fake);
        assert.doesNotMatch(res.stdout + res.stderr, /access-FAKE|refresh-FAKE/);
        const commands = browser
          .log()
          .split('\n')
          .filter((line) => line.startsWith('>> '))
          .map((line) => JSON.parse(line.slice(3)));
        const replies = commands.filter((msg) => msg.method === 'Fetch.fulfillRequest');
        assert.deepEqual(
          replies.map((msg) => msg.params.responseCode),
          [204, 200]
        );
        assert.ok(
          replies[0].params.responseHeaders.some(
            (h) =>
              h.name === 'Access-Control-Allow-Origin' && h.value === 'https://idp.example.test'
          )
        );
        assert.equal(commands.filter((msg) => msg.method === 'Fetch.failRequest').length, 3);
        assert.match(browser.log(), /Target.closeTarget/);
      } finally {
        kernel.terminate();
        await browser.close();
      }
    });
  }
}

test('kernel device flow uses outbound HTTPS and persists only a sealed grant', {
  skip,
}, async () => {
  const { kernel, posts, seals, run } = await seven('device', { secretScope: true });
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
