import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createConnection } from 'node:net';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { sandbox } from './helpers.mjs';

const opener = process.platform === 'darwin' ? 'open' : 'xdg-open';
const TOKEN = 'fake-dcr-access-token-zzzz';

function freePort() {
  return new Promise((resolve) => {
    const s = createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

function portFree(port) {
  return new Promise((resolve) => {
    const c = createConnection({ host: '127.0.0.1', port }, () => {
      c.end();
      resolve(false);
    });
    c.on('error', () => resolve(true));
  });
}

function clientGrantPath(stateDir) {
  const grants = join(stateDir, 'grants');
  mkdirSync(grants, { recursive: true });
  const key = 'mcp:ch:client\0dcr';
  let h = 0xcbf29ce484222325n;
  for (const c of key) {
    h ^= BigInt(c.charCodeAt(0));
    h = BigInt.asUintN(128, h * 0x0100000001b3n);
  }
  const bytes = Buffer.alloc(16);
  let x = h;
  for (let i = 0; i < 16; i++) {
    bytes[i] = Number(x & 0xffn);
    x >>= 8n;
  }
  return join(grants, `${bytes.toString('hex')}.json`);
}

async function dcrStub(seed = {}, opts = {}) {
  const clients = new Map(Object.entries(seed));
  const seen = { register: null, token: null, clients: 0 };
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${server.address().port}`;
    const send = (status, body) => {
      res.writeHead(status, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(body));
    };
    if (req.url.startsWith('/.well-known/oauth-authorization-server')) {
      send(200, {
        issuer: base,
        authorization_endpoint: `${base}/authorize`,
        token_endpoint: `${base}/token`,
        registration_endpoint: `${base}/register`,
        response_types_supported: ['code'],
        code_challenge_methods_supported: ['S256'],
        token_endpoint_auth_methods_supported: ['none'],
      });
      return;
    }
    if (req.url === '/register' && req.method === 'POST') {
      let body = '';
      req.on('data', (chunk) => {
        body += chunk;
      });
      req.on('end', () => {
        const parsed = JSON.parse(body);
        seen.clients += 1;
        const client_id = `client-${seen.clients}`;
        const redirect_uris = parsed.redirect_uris;
        if (!Array.isArray(redirect_uris) || redirect_uris.length !== 1) {
          send(400, { error: 'invalid_redirect_uri' });
          return;
        }
        clients.set(client_id, redirect_uris[0]);
        seen.register = { client_id, redirect_uris };
        const payload = { client_id };
        if (opts.clientSecret !== undefined) {
          payload.client_secret = opts.clientSecret;
        }
        if (opts.clientSecretExpiresAt !== undefined) {
          payload.client_secret_expires_at = opts.clientSecretExpiresAt;
        }
        send(201, payload);
      });
      return;
    }
    if (req.url === '/token' && req.method === 'POST') {
      let body = '';
      req.on('data', (chunk) => {
        body += chunk;
      });
      req.on('end', () => {
        const params = new URLSearchParams(body);
        const client_id = params.get('client_id');
        const redirect_uri = params.get('redirect_uri');
        seen.token = {
          client_id,
          redirect_uri,
          client_secret: params.get('client_secret'),
        };
        const expected = clients.get(client_id);
        if (expected === undefined || expected !== redirect_uri) {
          send(400, { error: 'invalid_grant', error_description: 'redirect_uri mismatch' });
          return;
        }
        send(200, {
          access_token: TOKEN,
          refresh_token: 'fake-refresh',
          token_type: 'Bearer',
          expires_in: 3600,
          scope: 'mcp',
        });
      });
      return;
    }
    send(404, {});
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  after(() => server.close());
  const port = server.address().port;
  return { seen, clients, port, base: `http://127.0.0.1:${port}` };
}

function host(servers = {}, openBody) {
  const sb = sandbox();
  const agent = join(sb.env.HOME, '.pi', 'agent');
  mkdirSync(agent, { recursive: true });
  writeFileSync(join(agent, 'mcp.json'), JSON.stringify({ mcpServers: servers }));
  const report = join(sb.dir, 'authorize.json');
  const browser =
    openBody ||
    `const { spawn } = require('node:child_process');
const asked = new URL(args[0]);
fs.writeFileSync(${JSON.stringify(report)}, JSON.stringify({
  client_id: asked.searchParams.get('client_id'),
  redirect_uri: asked.searchParams.get('redirect_uri'),
}));
const back = new URL(asked.searchParams.get('redirect_uri'));
back.searchParams.set('code', 'fake-code');
back.searchParams.set('state', asked.searchParams.get('state'));
spawn(process.execPath, ['-e', 'fetch(' + JSON.stringify(back.href) + ')'], { detached: true, stdio: 'ignore' }).unref();`;
  sb.script(opener, browser);
  const run = (args, extra) => sb.jerkAsync(args, { SLICC_JERK_STORE: 'file', ...extra });
  const authorize = () => JSON.parse(readFileSync(report, 'utf8'));
  return { sb, run, report, authorize };
}

test('DCR registers the same redirect_uri authorize and token exchange use', async () => {
  const stub = await dcrStub();
  const { run, sb, authorize } = host({
    ch: { url: `${stub.base}/mcp`, type: 'http' },
  });
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  assert.ok(stub.seen.register, 'registered');
  assert.equal(stub.seen.register.redirect_uris.length, 1);
  assert.match(stub.seen.register.redirect_uris[0], /^http:\/\/127\.0\.0\.1:\d+\/callback$/);
  assert.notEqual(stub.seen.register.redirect_uris[0], 'http://127.0.0.1:0/callback');
  const auth = authorize();
  assert.equal(auth.redirect_uri, stub.seen.register.redirect_uris[0]);
  assert.equal(auth.client_id, stub.seen.register.client_id);
  assert.equal(stub.seen.token.redirect_uri, stub.seen.register.redirect_uris[0]);
  assert.equal(stub.seen.token.client_id, stub.seen.register.client_id);
  const cache = join(sb.stateDir, 'mcp-clients', 'mcp_ch.json');
  assert.equal(existsSync(cache), true);
  const stored = JSON.parse(readFileSync(cache, 'utf8'));
  assert.equal(stored.client_id, stub.seen.register.client_id);
  assert.deepEqual(stored.redirect_uris, stub.seen.register.redirect_uris);
  assert.equal(stored.authorization_server, stub.base);
  assert.equal(stored.registration, 'dcr');
});

test('callbackPort is the registered and authorized redirect port', async () => {
  const stub = await dcrStub();
  const port = await freePort();
  const { run, authorize } = host({
    ch: {
      url: `${stub.base}/mcp`,
      type: 'http',
      oauth: { callbackPort: port },
    },
  });
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  const expected = `http://127.0.0.1:${port}/callback`;
  assert.deepEqual(stub.seen.register.redirect_uris, [expected]);
  const auth = authorize();
  assert.equal(auth.redirect_uri, expected);
  assert.equal(stub.seen.token.redirect_uri, expected);
  assert.equal(auth.client_id, stub.seen.register.client_id);
  assert.equal(stub.seen.token.client_id, stub.seen.register.client_id);
});

test('a stored client with different redirect_uris is not reused', async () => {
  const stub = await dcrStub();
  const { run, sb, authorize } = host({
    ch: { url: `${stub.base}/mcp`, type: 'http' },
  });
  const clients = join(sb.stateDir, 'mcp-clients');
  mkdirSync(clients, { recursive: true });
  writeFileSync(
    join(clients, 'mcp_ch.json'),
    JSON.stringify({
      client_id: 'stale-client-a',
      redirect_uris: ['http://127.0.0.1:1/callback'],
      authorization_server: stub.base,
    })
  );
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  assert.equal(stub.seen.clients, 1);
  assert.notEqual(stub.seen.register.client_id, 'stale-client-a');
  assert.equal(authorize().client_id, stub.seen.register.client_id);
  const stored = JSON.parse(readFileSync(join(clients, 'mcp_ch.json'), 'utf8'));
  assert.equal(stored.client_id, stub.seen.register.client_id);
  assert.deepEqual(stored.redirect_uris, stub.seen.register.redirect_uris);
});

test('a stored client without redirect_uris is not reused for DCR', async () => {
  const stub = await dcrStub();
  const { run, sb, authorize } = host({
    ch: { url: `${stub.base}/mcp`, type: 'http' },
  });
  const clients = join(sb.stateDir, 'mcp-clients');
  mkdirSync(clients, { recursive: true });
  writeFileSync(join(clients, 'mcp_ch.json'), JSON.stringify({ client_id: 'legacy-no-uris' }));
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  assert.notEqual(stub.seen.register.client_id, 'legacy-no-uris');
  assert.equal(authorize().client_id, stub.seen.register.client_id);
});

test('a stored client with empty or multi redirect_uris is not reused', async () => {
  for (const makeUris of [(live) => [], (live) => [live, 'http://127.0.0.1:2/callback']]) {
    const stub = await dcrStub();
    const port = await freePort();
    const live = `http://127.0.0.1:${port}/callback`;
    const { run, sb, authorize } = host({
      ch: {
        url: `${stub.base}/mcp`,
        type: 'http',
        oauth: { callbackPort: port },
      },
    });
    const clients = join(sb.stateDir, 'mcp-clients');
    mkdirSync(clients, { recursive: true });
    writeFileSync(
      join(clients, 'mcp_ch.json'),
      JSON.stringify({
        client_id: 'bad-uris-client',
        redirect_uris: makeUris(live),
        authorization_server: stub.base,
      })
    );
    const res = await run(['mcp:ch', 'create']);
    assert.equal(res.status, 0, res.stdout + res.stderr);
    assert.equal(stub.seen.clients, 1);
    assert.notEqual(authorize().client_id, 'bad-uris-client');
  }
});

test('a matching stored client is reused without a second registration', async () => {
  const port = await freePort();
  const redirect = `http://127.0.0.1:${port}/callback`;
  const stub = await dcrStub({ 'kept-client': redirect });
  const { run, sb, authorize } = host({
    ch: {
      url: `${stub.base}/mcp`,
      type: 'http',
      oauth: { callbackPort: port },
    },
  });
  const clients = join(sb.stateDir, 'mcp-clients');
  mkdirSync(clients, { recursive: true });
  writeFileSync(
    join(clients, 'mcp_ch.json'),
    JSON.stringify({
      client_id: 'kept-client',
      redirect_uris: [redirect],
      authorization_server: stub.base,
      registration: 'dcr',
    })
  );
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  assert.equal(stub.seen.clients, 0);
  assert.equal(authorize().client_id, 'kept-client');
  assert.equal(authorize().redirect_uri, redirect);
  assert.equal(stub.seen.token.client_id, 'kept-client');
  assert.equal(stub.seen.token.redirect_uri, redirect);
});

test('a reused client does not send another clients stored secret', async () => {
  const port = await freePort();
  const redirect = `http://127.0.0.1:${port}/callback`;
  const stub = await dcrStub({ 'client-b': redirect });
  const { run, sb } = host({
    ch: {
      url: `${stub.base}/mcp`,
      type: 'http',
      oauth: { callbackPort: port },
    },
  });
  const clients = join(sb.stateDir, 'mcp-clients');
  mkdirSync(clients, { recursive: true });
  writeFileSync(
    join(clients, 'mcp_ch.json'),
    JSON.stringify({
      client_id: 'client-b',
      redirect_uris: [redirect],
      authorization_server: stub.base,
      registration: 'dcr',
    })
  );
  writeFileSync(
    clientGrantPath(sb.stateDir),
    JSON.stringify({
      provider: 'mcp:ch:client',
      user: 'dcr',
      grant: {
        provider: 'mcp:ch:client',
        user: 'dcr',
        refresh_token: '',
        access_token: 'secret-of-client-a',
        fields: { client_id: 'client-a' },
      },
    })
  );
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  assert.equal(stub.seen.token.client_id, 'client-b');
  assert.equal(stub.seen.token.client_secret, null);
});

test('a stored client for a different authorization server is not reused', async () => {
  const stub = await dcrStub();
  const port = await freePort();
  const redirect = `http://127.0.0.1:${port}/callback`;
  const { run, sb, authorize } = host({
    ch: {
      url: `${stub.base}/mcp`,
      type: 'http',
      oauth: { callbackPort: port },
    },
  });
  const clients = join(sb.stateDir, 'mcp-clients');
  mkdirSync(clients, { recursive: true });
  writeFileSync(
    join(clients, 'mcp_ch.json'),
    JSON.stringify({
      client_id: 'other-as-client',
      redirect_uris: [redirect],
      authorization_server: 'http://127.0.0.1:1',
    })
  );
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  assert.equal(stub.seen.clients, 1);
  assert.notEqual(authorize().client_id, 'other-as-client');
  assert.equal(authorize().client_id, stub.seen.register.client_id);
});

test('a stored client with a different registration scope is not reused', async () => {
  const stub = await dcrStub();
  const port = await freePort();
  const redirect = `http://127.0.0.1:${port}/callback`;
  const { run, sb, authorize } = host({
    ch: {
      url: `${stub.base}/mcp`,
      type: 'http',
      oauth: { callbackPort: port, scope: 'mcp.write' },
    },
  });
  const clients = join(sb.stateDir, 'mcp-clients');
  mkdirSync(clients, { recursive: true });
  writeFileSync(
    join(clients, 'mcp_ch.json'),
    JSON.stringify({
      client_id: 'narrow-scope-client',
      redirect_uris: [redirect],
      authorization_server: stub.base,
      scope: 'mcp.read',
    })
  );
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  assert.equal(stub.seen.clients, 1);
  assert.notEqual(authorize().client_id, 'narrow-scope-client');
});

test('callbackUrl localhost form stays byte-identical through register authorize and token', async () => {
  const stub = await dcrStub();
  const port = await freePort();
  const redirect = `http://localhost:${port}/callback`;
  const { run, authorize } = host({
    ch: {
      url: `${stub.base}/mcp`,
      type: 'http',
      oauth: { callbackUrl: redirect },
    },
  });
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  assert.deepEqual(stub.seen.register.redirect_uris, [redirect]);
  assert.equal(authorize().redirect_uri, redirect);
  assert.equal(stub.seen.token.redirect_uri, redirect);
});

test('a timed-out create exits and frees the callback port', async () => {
  const stub = await dcrStub();
  const port = await freePort();
  const { run } = host(
    {
      ch: {
        url: `${stub.base}/mcp`,
        type: 'http',
        oauth: { callbackPort: port },
      },
    },
    `void 0;`
  );
  const started = Date.now();
  const res = await run(['mcp:ch', 'create'], { SLICC_JERK_MCP_CALLBACK_SECS: '2' });
  assert.notEqual(res.status, 0, res.stdout + res.stderr);
  assert.match(res.stderr, /timed out waiting for callback/);
  assert.ok(Date.now() - started < 15000);
  assert.equal(await portFree(port), true);
});

test('a second create does not hang when the callback port is already taken', async () => {
  const stub = await dcrStub();
  const port = await freePort();
  const blocker = createServer();
  await new Promise((resolve) => blocker.listen(port, '127.0.0.1', resolve));
  after(() => blocker.close());
  const { run } = host({
    ch: {
      url: `${stub.base}/mcp`,
      type: 'http',
      oauth: { callbackPort: port },
    },
  });
  const res = await run(['mcp:ch', 'create'], { SLICC_JERK_MCP_CALLBACK_SECS: '2' });
  assert.notEqual(res.status, 0, res.stdout + res.stderr);
  assert.match(res.stderr, /cannot bind loopback/);
});

test('create re-registers when a confidential client secret has already expired', async () => {
  const port = await freePort();
  const stub = await dcrStub({}, { clientSecret: 'expired-secret', clientSecretExpiresAt: 1 });
  const { run, sb, authorize } = host({
    ch: {
      url: `${stub.base}/mcp`,
      type: 'http',
      oauth: { callbackPort: port },
    },
  });
  const first = await run(['mcp:ch', 'create']);
  assert.equal(first.status, 0, first.stdout + first.stderr);
  assert.equal(stub.seen.clients, 1);
  assert.equal(stub.seen.token.client_secret, 'expired-secret');
  const cache = join(sb.stateDir, 'mcp-clients', 'mcp_ch.json');
  const stored = JSON.parse(readFileSync(cache, 'utf8'));
  assert.equal(stored.confidential, true);
  const grant = JSON.parse(readFileSync(clientGrantPath(sb.stateDir), 'utf8'));
  assert.equal(grant.grant.expires_at, 1);
  const second = await run(['mcp:ch', 'create']);
  assert.equal(second.status, 0, second.stdout + second.stderr);
  assert.equal(stub.seen.clients, 2);
  assert.notEqual(authorize().client_id, stored.client_id);
  assert.equal(authorize().client_id, stub.seen.register.client_id);
});

test('create does not keep a DCR cache when the confidential secret cannot be stored', async () => {
  const port = await freePort();
  const stub = await dcrStub({}, { clientSecret: 'unstoreable-secret' });
  const { run, sb } = host({
    ch: {
      url: `${stub.base}/mcp`,
      type: 'http',
      oauth: { callbackPort: port },
    },
  });
  mkdirSync(clientGrantPath(sb.stateDir), { recursive: true });
  const res = await run(['mcp:ch', 'create']);
  assert.equal(res.status, 0, res.stdout + res.stderr);
  assert.match(res.stderr, /cannot store client secret/);
  assert.equal(stub.seen.clients, 1);
  assert.equal(stub.seen.token.client_secret, 'unstoreable-secret');
  const cache = join(sb.stateDir, 'mcp-clients', 'mcp_ch.json');
  assert.equal(existsSync(cache), false);
  const again = await run(['mcp:ch', 'create']);
  assert.equal(again.status, 0, again.stdout + again.stderr);
  assert.equal(stub.seen.clients, 2);
});
