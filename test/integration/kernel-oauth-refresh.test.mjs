import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { test } from 'node:test';
import { wasm } from './helpers.mjs';
import { fake, placeholder, refresh, seven } from './kernel-oauth-harness.mjs';

const skip = !existsSync(wasm) && 'run npm run build:wasm first';
const future = () => Date.now() + 3600000;
const status = (extra = {}) => ({
  v: 1,
  placeholder,
  state: 'ready',
  expiresAt: future(),
  refreshable: true,
  ...extra,
});
const record = (extra = {}) => ({ ...status(extra), oauthManaged: true });
const config = (extra = '') => `[providers.demo]
env = ["DEMO_TOKEN"]
[providers.demo.oauth]
mode = "device"
flow = "pkce"
authorize_url = "https://idp.example.test/device"
token_url = "https://idp.example.test/token"
client_id = "\${DEMO_CLIENT:-fake-client}"
redirect_uri = "http://127.0.0.1:43819/callback"
capture = { at = "request", from = "query", param = "code" }
scope = "read"
${extra}
`;

function privateOutput(result) {
  assert.doesNotMatch(result.stdout + result.stderr, /access-FAKE|refresh-FAKE|secret-FAKE/);
}

async function references(kernel) {
  const tmp = await kernel.root.getDirectoryHandle('tmp');
  const state = await tmp.getDirectoryHandle('oauth-state');
  const managed = await state.getDirectoryHandle('oauth-managed');
  const grants = await managed.getDirectoryHandle('grants');
  const saved = [];
  for await (const handle of grants.values())
    saved.push(JSON.parse(await (await handle.getFile()).text()));
  return saved;
}

test('kernel atomically adopts a resolved public grant and stores identity only', {
  skip,
}, async () => {
  const { kernel, run, requests, posts, seals } = await seven('device', {
    config: config(),
    env: { DEMO_CLIENT: 'resolved-fake-client' },
  });
  try {
    const created = await run('create');
    assert.equal(created.status, 0, created.stderr);
    privateOutput(created);
    assert.equal(seals.length, 1);
    const { caller, ...grant } = seals[0];
    assert.deepEqual(grant, {
      v: 1,
      op: 'oauthGrant',
      name: 'DEMO_TOKEN',
      provider: 'demo',
      account: 'default',
      accessToken: fake,
      refreshToken: refresh,
      tokenType: 'Bearer',
      expiresAt: grant.expiresAt,
      oauth: {
        tokenUrl: 'https://idp.example.test/token',
        clientId: 'resolved-fake-client',
        authMethod: 'none',
        scope: 'read',
      },
    });
    assert.ok(Number.isSafeInteger(grant.expiresAt));
    assert.equal(grant.expiresAt % 1000, 0);
    assert.equal(
      requests.some((req) => req.op === 'seal'),
      false
    );
    assert.ok(
      requests.findIndex((req) => req.op === 'capabilities') <
        requests.findIndex((req) => req.op === 'oauthGrant')
    );
    assert.equal(posts.length, 3);
    assert.deepEqual(await references(kernel), [
      { provider: 'demo', account: 'default', name: 'DEMO_TOKEN', placeholder },
    ]);
    const got = await run('get', '--json', '--reveal');
    assert.equal(got.status, 0, got.stderr);
    assert.deepEqual(JSON.parse(got.stdout), {
      provider: 'demo',
      source: 'oauth',
      placeholder,
      expires_at: grant.expiresAt / 1000,
      state: 'ready',
      refreshable: true,
    });
  } finally {
    kernel.terminate();
  }
});

test('managed discovery refreshes authoritative expiry without local grant files or provider requests', {
  skip,
}, async () => {
  const options = {
    records: { DEMO_TOKEN: record({ expiresAt: Date.now() - 1000, state: 'refreshing' }) },
    reply(req, records) {
      if (req.op === 'oauthRefresh') {
        const next = record();
        records.set(req.name, next);
        const { oauthManaged, ...reply } = next;
        return reply;
      }
    },
  };
  const { kernel, run, posts, requests, records } = await seven('device', options);
  try {
    for (const args of [
      ['get', '--json'],
      ['get', '--refresh', '--json'],
    ]) {
      const got = await run(...args);
      assert.equal(got.status, 0, got.stderr);
      privateOutput(got);
      const body = JSON.parse(got.stdout);
      assert.equal(body.placeholder, placeholder);
      assert.equal(body.expires_at, Math.floor(records.get('DEMO_TOKEN').expiresAt / 1000));
      assert.equal(body.state, 'ready');
    }
    assert.equal(posts.length, 0);
    assert.equal(requests.filter((req) => req.op === 'oauthRefresh').length, 2);
    assert.equal(
      requests.some((req) => req.jop === 'status' || req.op === 'seal'),
      false
    );
    await assert.rejects(references(kernel));
  } finally {
    kernel.terminate();
  }
});

for (const state of ['ready', 'refreshing']) {
  test(`create checks managed ${state} readiness before reporting already signed in`, {
    skip,
  }, async () => {
    const { kernel, run, requests, posts } = await seven('device', {
      records: { DEMO_TOKEN: record({ state, expiresAt: state === 'ready' ? future() : 0 }) },
      reply: (req) => (req.op === 'oauthRefresh' ? status() : undefined),
    });
    try {
      const created = await run('create');
      assert.equal(created.status, 0, created.stderr);
      assert.match(created.stdout, /already signed in/);
      assert.ok(requests.some((req) => req.op === 'oauthStatus'));
      assert.equal(
        requests.filter((req) => req.op === 'oauthRefresh').length,
        state === 'ready' ? 0 : 1
      );
      assert.equal(posts.length, 0);
    } finally {
      kernel.terminate();
    }
  });
}

for (const noRefresh of [false, true]) {
  test(`managed unknown expiry is nullable and forced refresh is honest, noRefresh=${noRefresh}`, {
    skip,
  }, async () => {
    const { kernel, run, requests } = await seven('device', { noRefresh, noExpiry: true });
    try {
      assert.equal((await run('create')).status, 0);
      const got = await run('get', '--json');
      assert.equal(got.status, 0, got.stderr);
      assert.equal(JSON.parse(got.stdout).expires_at, null);
      const forced = await run('get', '--refresh');
      assert.equal(forced.status, noRefresh ? 5 : 0, forced.stderr);
      assert.equal(requests.filter((req) => req.op === 'oauthRefresh').length, noRefresh ? 0 : 1);
      privateOutput(forced);
    } finally {
      kernel.terminate();
    }
  });
}

for (const [state, exit] of [
  ['locked', 1],
  ['removed', 3],
  ['expired', 3],
  ['reauth-required', 3],
]) {
  test(`managed ${state} blocks stale fallback`, { skip }, async () => {
    const { kernel, run, requests, posts } = await seven('device', {
      records: { DEMO_TOKEN: record({ state, refreshable: false }) },
      cli: true,
    });
    try {
      const got = await run('get', '--json', '--reveal');
      assert.equal(got.status, exit, got.stderr);
      assert.equal(got.stdout, '');
      privateOutput(got);
      assert.equal(posts.length, 0);
      assert.equal(
        requests.some((req) => req.jop === 'status' || req.op === 'seal'),
        false
      );
      await assert.rejects(kernel.readFile('/tmp/minter-call'));
    } finally {
      kernel.terminate();
    }
  });
}

for (const [reply, exit] of [
  [{ v: 1, error: { code: 'ENOTCONN', message: fake } }, 4],
  [{ v: 1, error: { code: 'ETIMEDOUT', message: fake } }, 4],
  [{ v: 1, error: { code: 'EAGAIN', message: fake } }, 4],
  [{ v: 1, error: { code: 'EACCES', cause: 'oauth-reauth-required', message: fake } }, 3],
  [{ v: 1, error: { code: 'EEXIST', message: fake } }, 1],
  [{ ...status(), v: 2 }, 1],
  [{ ...status(), accessToken: fake }, 1],
  [{ ...status(), expiresAt: '3600000' }, 1],
  [{ ...status(), placeholder: 'fnox_proxy_AAAAAAAAAAAAAAAAAAAAAAAA' }, 1],
  [{ v: 1, error: { code: 'FAKE', cause: fake, message: fake } }, 1],
]) {
  test(`managed status failure stays private: ${reply.error?.code ?? Object.keys(reply).join(',')}/${reply.v}`, {
    skip,
  }, async () => {
    const { kernel, run, requests } = await seven('device', {
      records: { DEMO_TOKEN: record() },
      reply: (req) => (req.op === 'oauthStatus' ? reply : undefined),
    });
    try {
      const got = await run('get', '--reveal');
      assert.equal(got.status, exit, got.stderr);
      assert.equal(got.stdout, '');
      privateOutput(got);
      assert.equal(
        requests.some((req) => req.jop === 'status' || req.op === 'seal'),
        false
      );
    } finally {
      kernel.terminate();
    }
  });
}

for (const capabilities of [{}, { kernelOAuth: 2 }]) {
  test(`public create refuses unsupported capability before sign-in ${JSON.stringify(capabilities)}`, {
    skip,
  }, async () => {
    const { kernel, run, posts, seals } = await seven('device', { capabilities });
    try {
      const created = await run('create', '--force');
      assert.equal(created.status, capabilities.kernelOAuth === 2 ? 1 : 5, created.stderr);
      privateOutput(created);
      assert.equal(posts.length, 0);
      assert.equal(seals.length, 0);
    } finally {
      kernel.terminate();
    }
  });
}

test('managed marker fails closed through an unsupported transport', { skip }, async () => {
  const { kernel, run, posts, requests } = await seven('device', {
    capabilities: {},
    records: { DEMO_TOKEN: record() },
  });
  try {
    const got = await run('get');
    assert.equal(got.status, 5, got.stderr);
    assert.equal(got.stdout, '');
    assert.equal(posts.length, 0);
    assert.equal(
      requests.some((req) => req.jop === 'status'),
      false
    );
  } finally {
    kernel.terminate();
  }
});

test('kernel refresh_env cannot bypass vault but access env still has priority', {
  skip,
}, async () => {
  const options = {
    config: config('refresh_env = "DEMO_REFRESH"'),
    env: { DEMO_REFRESH: refresh },
  };
  const { kernel, run, posts } = await seven('device', options);
  try {
    const denied = await run('get');
    assert.equal(denied.status, 5, denied.stderr);
    assert.match(denied.stderr, /refresh_env.*create/);
    privateOutput(denied);
    assert.equal(posts.length, 0);
    options.env.DEMO_TOKEN = fake;
    const got = await run('get');
    assert.equal(got.status, 0, got.stderr);
    assert.equal(got.stdout, placeholder + '\n');
    assert.equal(posts.length, 0);
    privateOutput(got);
  } finally {
    kernel.terminate();
  }
});

for (const same of [true, false]) {
  test(`forced refresh respects selected env placeholder identity same=${same}`, {
    skip,
  }, async () => {
    const { kernel, run, requests } = await seven('device', {
      records: { DEMO_TOKEN: record() },
      env: { DEMO_TOKEN: same ? placeholder : 'fnox_proxy_444444444444444444444444' },
    });
    try {
      const got = await run('get', '--refresh');
      assert.equal(got.status, same ? 0 : 5, got.stderr);
      assert.equal(requests.filter((req) => req.op === 'oauthRefresh').length, same ? 1 : 0);
    } finally {
      kernel.terminate();
    }
  });
}

test('confidential interactive creation keeps its legacy access-only path with a warning', {
  skip,
}, async () => {
  const { kernel, run, requests, seals } = await seven('device', {
    config: config('client_secret = "secret-FAKE"'),
    capabilities: {},
  });
  try {
    const created = await run('create');
    assert.equal(created.status, 0, created.stderr);
    assert.match(created.stderr, /confidential OAuth refresh is unsupported.*access-only/);
    privateOutput(created);
    assert.equal(seals[0].op, 'seal');
    assert.equal(
      requests.some((req) => req.op === 'oauthGrant' || req.op === 'capabilities'),
      false
    );
    const got = await run('get');
    assert.equal(got.status, 0, got.stderr);
    assert.equal((await run('get', '--refresh')).status, 5);
  } finally {
    kernel.terminate();
  }
});

test('lost registration completion keeps an identity reference and never replays initial credentials', {
  skip,
}, async () => {
  let lost = true;
  let offline = false;
  const options = {
    reply(req, records) {
      if (offline) return { v: 1, error: { code: 'ENOTCONN', message: fake } };
      if (req.op === 'oauthGrant' && lost) {
        records.set(req.name, record());
        return { v: 1, error: { code: 'ETIMEDOUT', message: fake } };
      }
    },
  };
  const { kernel, run, requests, posts } = await seven('device', options);
  try {
    const created = await run('create');
    assert.equal(created.status, 4, created.stderr);
    privateOutput(created);
    assert.deepEqual(await references(kernel), [
      { provider: 'demo', account: 'default', name: 'DEMO_TOKEN', placeholder: null },
    ]);
    offline = true;
    const failed = await run('get');
    assert.equal(failed.status, 4, failed.stderr);
    assert.equal(failed.stdout, '');
    offline = false;
    lost = false;
    const got = await run('get');
    assert.equal(got.status, 0, got.stderr);
    assert.equal(got.stdout, placeholder + '\n');
    assert.equal(requests.filter((req) => req.op === 'oauthGrant').length, 1);
    assert.equal(posts.length, 3);
  } finally {
    kernel.terminate();
  }
});

test('managed reauthentication cannot reuse a stale legacy already-signed-in cache', {
  skip,
}, async () => {
  const options = { config: config('client_secret = "secret-FAKE"') };
  const { kernel, run, records, requests } = await seven('device', options);
  try {
    assert.equal((await run('create')).status, 0);
    await kernel.writeFile('/tmp/providers.toml', config());
    records.set('DEMO_TOKEN', record({ state: 'reauth-required', refreshable: false }));
    const created = await run('create');
    assert.equal(created.status, 0, created.stderr);
    assert.equal(created.stdout, 'demo: signed in\n');
    assert.equal(requests.filter((req) => req.op === 'oauthGrant').length, 1);
    privateOutput(created);
  } finally {
    kernel.terminate();
  }
});

test('refresh cannot silently switch the selected placeholder', { skip }, async () => {
  const { kernel, run } = await seven('device', {
    records: { DEMO_TOKEN: record() },
    reply: (req) =>
      req.op === 'oauthRefresh'
        ? status({ placeholder: 'fnox_proxy_444444444444444444444444' })
        : undefined,
  });
  try {
    const got = await run('get', '--refresh');
    assert.equal(got.status, 1, got.stderr);
    assert.equal(got.stdout, '');
  } finally {
    kernel.terminate();
  }
});

for (const [provider, name] of [
  ['linkedin', 'LINKEDIN_ACCESS_TOKEN'],
  ['github', 'GITHUB_TOKEN'],
]) {
  test(`managed ${provider} named records remain authoritative without local OAuth config`, {
    skip,
  }, async () => {
    const { kernel, run, requests, posts } = await seven('device', {
      provider,
      records: { [name]: record() },
      env: { [name]: '' },
    });
    try {
      const got = await run('get', '--json', '--refresh');
      assert.equal(got.status, 0, got.stderr);
      assert.equal(JSON.parse(got.stdout).placeholder, placeholder);
      assert.ok(
        requests.some(
          (req) => req.op === 'oauthStatus' && req.name === name && req.provider === provider
        )
      );
      assert.equal(posts.length, 0);
    } finally {
      kernel.terminate();
    }
  });
}

test('drop preserves identity on refusal and removes the linked managed record on confirmation', {
  skip,
}, async () => {
  const options = { dropDenied: true };
  const { kernel, run, requests } = await seven('device', options);
  try {
    assert.equal((await run('create')).status, 0);
    assert.equal((await run('drop')).status, 2);
    assert.equal((await run('drop', '--yes')).status, 1);
    assert.equal((await references(kernel)).length, 1);
    options.dropDenied = false;
    const dropped = await run('drop', '--yes');
    assert.equal(dropped.status, 0, dropped.stderr);
    assert.equal((await references(kernel)).length, 0);
    assert.equal((await run('get')).status, 3);
    assert.equal(requests.filter((req) => req.op === 'oauthRemove').length, 2);
    assert.equal(
      requests.some((req) => req.kind === 'unseal'),
      false
    );
  } finally {
    kernel.terminate();
  }
});
