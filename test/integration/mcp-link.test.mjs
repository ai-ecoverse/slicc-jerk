import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'fake-mcp-access-token-1234';
const REFRESHED = 'fake-mcp-refreshed-token-5678';
const opener = process.platform === 'darwin' ? 'open' : 'xdg-open';

function ndjson(text) {
  return text
    .trim()
    .split('\n')
    .map((line) => JSON.parse(line));
}

async function idp(override = {}) {
  const hits = [];
  const server = createServer((req, res) => {
    hits.push(`${req.method} ${req.url}`);
    const base = `http://127.0.0.1:${server.address().port}`;
    const send = (status, body, headers = {}) => {
      res.writeHead(status, { 'Content-Type': 'application/json', ...headers });
      res.end(JSON.stringify(body));
    };
    if (req.url === override.redirect) {
      res.writeHead(302, { Location: override.location });
      res.end();
    } else if (req.url.startsWith('/.well-known/oauth-authorization-server')) {
      send(200, {
        issuer: base,
        authorization_endpoint: `${base}/authorize`,
        token_endpoint: `${base}/token`,
        registration_endpoint: `${base}/register`,
        response_types_supported: ['code'],
        code_challenge_methods_supported: ['S256'],
        ...override.metadata,
      });
    } else if (req.url === '/register') {
      send(201, { client_id: 'fake-client' });
    } else if (req.url === '/token') {
      let body = '';
      req.on('data', (chunk) => {
        body += chunk;
      });
      req.on('end', () => {
        const refresh = body.includes('grant_type=refresh_token');
        send(200, {
          access_token: refresh ? REFRESHED : TOKEN,
          refresh_token: 'fake-refresh',
          token_type: 'Bearer',
          expires_in: override.expiresIn ?? 3600,
          scope: 'mcp.read',
        });
      });
    } else {
      send(404, {});
    }
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  after(() => server.close());
  const port = server.address().port;
  return { hits, port, base: `http://127.0.0.1:${port}` };
}

function host(servers = {}) {
  const sb = sandbox();
  const agent = join(sb.env.HOME, '.pi', 'agent');
  mkdirSync(agent, { recursive: true });
  const write = (entries) =>
    writeFileSync(join(agent, 'mcp.json'), JSON.stringify({ mcpServers: entries }));
  write(servers);
  sb.script(
    opener,
    `const { spawn } = require('node:child_process');
const asked = new URL(args[0]);
const back = new URL(asked.searchParams.get('redirect_uri'));
back.searchParams.set('code', 'fake-code');
back.searchParams.set('state', asked.searchParams.get('state'));
spawn(process.execPath, ['-e', 'fetch(' + JSON.stringify(back.href) + ')'], { detached: true, stdio: 'ignore' }).unref();`
  );
  const run = (args, extra) => sb.jerkAsync(args, { SLICC_JERK_STORE: 'file', ...extra });
  const signIn = async (id) => {
    const res = await run([id, 'create']);
    assert.equal(res.status, 0, res.stdout + res.stderr);
  };
  return { sb, run, signIn, write };
}

const teredo = 'https://[2001:0:4136:e378::1]';

function refused(res, where) {
  assert.equal(res.status, 2, res.stdout + res.stderr);
  assert.match(ndjson(res.stdout).at(-1).message, new RegExp(`${where}: .*refused over the link`));
}

test('a lease over the link gives the token, and drop --lease ends it', async () => {
  const stub = await idp();
  const id = `mcp:url:${stub.base}/mcp`;
  const { run, signIn } = host();
  await signIn(id);
  const lease = await run([id, 'lease', '--via-link'], { FNOX_LEASE_DURATION: '120' });
  assert.equal(lease.status, 0, lease.stdout + lease.stderr);
  const [held, done] = ndjson(lease.stdout);
  assert.deepEqual(held.credentials, { MCP_TOKEN: TOKEN });
  assert.equal(done.provider, id);
  assert.equal(done.expires_at, held.expires_at);

  const drop = await run([id, 'drop', '--lease', '--via-link'], {
    FNOX_LEASE_ID: held.lease_id,
  });
  assert.equal(drop.status, 0, drop.stdout + drop.stderr);
  assert.equal(ndjson(drop.stdout).at(-1).provider, id);
});

test('a URL in the host mcp.json leases under that entry', async () => {
  const stub = await idp();
  const id = `mcp:url:${stub.base}/mcp`;
  const { run, signIn } = host({ bb: { url: `${stub.base}/mcp`, oauth: { clientId: 'cid' } } });
  await signIn(id);
  const lease = await run([id, 'lease', '--via-link']);
  assert.equal(lease.status, 0, lease.stdout + lease.stderr);
  assert.deepEqual(ndjson(lease.stdout)[0].credentials, { MCP_BB_TOKEN: TOKEN });
});

test('a lease with no sign-in is a status, not a panic', async () => {
  const { run } = host();
  const id = 'mcp:url:https://mcp.example.invalid/mcp';
  const res = await run([id, 'lease', '--via-link']);
  assert.equal(res.status, 3, res.stdout + res.stderr);
  assert.match(ndjson(res.stdout).at(-1).message, /not signed in/);
});

test('a lease refreshes an expiring token on a loopback server', async () => {
  const stub = await idp({ expiresIn: 60 });
  const id = `mcp:url:${stub.base}/mcp`;
  const { run, signIn } = host();
  await signIn(id);
  const lease = await run([id, 'lease', '--via-link']);
  assert.equal(lease.status, 0, lease.stdout + lease.stderr);
  assert.deepEqual(ndjson(lease.stdout)[0].credentials, { MCP_TOKEN: REFRESHED });
});

for (const name of ['127.0.0.1', 'localhost']) {
  test(`a public server cannot send a lease to ${name}`, async () => {
    const stub = await idp({ expiresIn: 60 });
    const metadata = `http://${name}:${stub.port}/.well-known/oauth-authorization-server`;
    const { run, signIn } = host({
      bb: {
        url: 'https://8.8.8.8/mcp',
        oauth: { clientId: 'cid', authServerMetadataUrl: metadata },
      },
    });
    await signIn('mcp:bb');
    const seen = stub.hits.length;
    refused(await run(['mcp:bb', 'lease', '--via-link']), 'authorization server metadata');
    assert.equal(stub.hits.length, seen);

    const direct = await run(['mcp:bb', 'get', '--reveal']);
    assert.equal(direct.status, 0, direct.stdout + direct.stderr);
    assert.equal(direct.stdout, `${REFRESHED}\n`);
  });
}

test('a Teredo start URL is refused for a lease', async () => {
  const stub = await idp({ expiresIn: 60 });
  const { run, signIn } = host({
    bb: {
      url: `${teredo}/mcp`,
      oauth: {
        clientId: 'cid',
        authServerMetadataUrl: `${stub.base}/.well-known/oauth-authorization-server`,
      },
    },
  });
  await signIn('mcp:bb');
  const seen = stub.hits.length;
  refused(await run(['mcp:bb', 'lease', '--via-link']), 'server URL');
  assert.equal(stub.hits.length, seen);
});

test('a public server cannot send a lease to a Teredo address', async () => {
  const stub = await idp({ expiresIn: 60 });
  const entry = (authServerMetadataUrl) => ({
    bb: { url: 'https://8.8.8.8/mcp', oauth: { clientId: 'cid', authServerMetadataUrl } },
  });
  const { run, signIn, write } = host(entry(`${stub.base}/.well-known/oauth-authorization-server`));
  await signIn('mcp:bb');
  write(entry(`${teredo}/.well-known/oauth-authorization-server`));
  refused(await run(['mcp:bb', 'lease', '--via-link']), 'authorization server metadata');
});

for (const [field, where] of [
  ['authorization_endpoint', 'authorization endpoint'],
  ['token_endpoint', 'token endpoint'],
  ['registration_endpoint', 'registration endpoint'],
]) {
  test(`a loopback server cannot point ${field} off the machine`, async () => {
    const options = { expiresIn: 60 };
    const stub = await idp(options);
    const id = `mcp:url:${stub.base}/mcp`;
    const { run, signIn } = host();
    await signIn(id);
    options.metadata = { [field]: 'https://8.8.8.8/oauth' };
    const seen = stub.hits.filter((hit) => hit === 'POST /token').length;
    refused(await run([id, 'lease', '--via-link']), where);
    assert.equal(stub.hits.filter((hit) => hit === 'POST /token').length, seen);
  });
}

test('an off-machine authorize endpoint only matters over the link', async () => {
  const options = { expiresIn: 60 };
  const stub = await idp(options);
  const id = `mcp:url:${stub.base}/mcp`;
  const { run, signIn } = host();
  await signIn(id);
  options.metadata = { authorization_endpoint: 'https://8.8.8.8/oauth' };
  refused(await run([id, 'lease', '--via-link']), 'authorization endpoint');
  const direct = await run([id, 'get', '--reveal']);
  assert.equal(direct.status, 0, direct.stdout + direct.stderr);
  assert.equal(direct.stdout, `${REFRESHED}\n`);
});

test('a redirect off the machine is refused at the connection', async () => {
  const options = { expiresIn: 60 };
  const stub = await idp(options);
  const id = `mcp:url:${stub.base}/mcp`;
  const { run, signIn } = host();
  await signIn(id);
  options.redirect = '/.well-known/oauth-authorization-server';
  options.location = 'http://[::ffff:10.0.0.1]:9/.well-known/oauth-authorization-server';
  refused(await run([id, 'lease', '--via-link']), 'oauth-authorization-server');
});
