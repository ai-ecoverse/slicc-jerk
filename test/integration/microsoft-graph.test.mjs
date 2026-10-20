import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { join } from 'node:path';
import { test } from 'node:test';
import { stubCdp } from './cdp-stub.mjs';
import { sandbox } from './helpers.mjs';

const TOKEN = 'eyJhbGciOiJub25lIn0.eyJleHAiOjQ3MDAwMDAwMDAsInNjcCI6IlVzZXIuUmVhZCJ9.sig';
const LEAK = 'LEAKY-AZ-OUTPUT';

function az(sb, { token = TOKEN, loginExit = 0 } = {}) {
  sb.script(
    'az',
    `if (args[0] === 'account' && args[1] === 'get-access-token') {
  process.stdout.write(JSON.stringify({ accessToken: ${JSON.stringify(token)}, expiresOn: '2099-01-01 00:00:00.000000' }));
  process.exit(0);
}
if (args[0] === 'account' && args[1] === 'list') {
  process.stdout.write(JSON.stringify([{ id: 'sub', name: 'demo' }]));
  process.exit(0);
}
if (args[0] === 'account' && args[1] === 'show') {
  process.stdout.write('user@example.com\\n');
  process.exit(0);
}
if (args[0] === 'login') { process.exit(${loginExit}); }
if (args[0] === 'logout') { process.exit(0); }
process.stdout.write(${JSON.stringify(`${LEAK}\n`)});
process.exit(1);`
  );
}

test('with az on PATH, get list create and drop use the Azure CLI', () => {
  const sb = sandbox();
  az(sb);
  const got = sb.jerk(['microsoft-graph', 'get', '--reveal']);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${TOKEN}\n`);
  assert.equal(sb.jerk(['microsoft-graph', 'list']).status, 0);
  assert.equal(sb.jerk(['microsoft-graph', 'create', '--force']).status, 0);
  assert.equal(sb.jerk(['microsoft-graph', 'drop', '--yes']).status, 0);
  const runs = sb.calls().map((c) => c.args.slice(0, 3));
  assert.deepEqual(runs, [
    ['account', 'get-access-token', '--resource-type'],
    ['account', 'list', '-o'],
    ['login', '--tenant', 'common'],
    ['logout'],
  ]);
});

test('MICROSOFT_GRAPH_TOKEN wins over az', () => {
  const sb = sandbox();
  az(sb);
  const res = sb.jerk(['microsoft-graph', 'get', '--json'], {
    MICROSOFT_GRAPH_TOKEN: 'envgraphtokenvalue1234567',
  });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).source, 'env');
  assert.deepEqual(sb.calls(), []);
});

test('EXCEL_GRAPH_TOKEN is accepted when MICROSOFT_GRAPH_TOKEN is unset', () => {
  const sb = sandbox();
  const res = sb.jerk(['microsoft-graph', 'get', '--json'], {
    EXCEL_GRAPH_TOKEN: 'excelgraphtokenvalue1234567',
  });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).source, 'env');
});

test('TEAMS_GRAPH_TOKEN is accepted too', () => {
  const sb = sandbox();
  const res = sb.jerk(['microsoft-graph', 'get', '--json'], {
    TEAMS_GRAPH_TOKEN: 'teamsgraphtokenvalue1234567',
  });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).source, 'env');
});

test('without az, get falls through to oauth and list --json shows the chain', () => {
  const sb = sandbox();
  const res = sb.jerk(['microsoft-graph', 'get']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /\n {2}az: not on PATH\n {2}oauth: not signed in\n/);
  const entry = JSON.parse(sb.jerk(['list', '--json']).stdout).find(
    (p) => p.provider === 'microsoft-graph'
  );
  assert.deepEqual(entry.steps, [{ cli: 'az', found: false }, { oauth: true }]);
  assert.deepEqual(entry.env, ['MICROSOFT_GRAPH_TOKEN', 'EXCEL_GRAPH_TOKEN', 'TEAMS_GRAPH_TOKEN']);
});

test('config show prints the az then oauth chain', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'microsoft-graph']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /\[\[providers\.microsoft-graph\.get\]\]/);
  assert.match(shown.stdout, /requires = "az"/);
  assert.match(shown.stdout, /resource-type/);
  assert.match(shown.stdout, /oauth = true/);
  assert.match(shown.stdout, /login\.microsoftonline\.com\/common/);
  assert.match(shown.stdout, /14d82eec-204b-4c2f-b7e8-296a70dab67e/);
  assert.match(shown.stdout, /Files\.ReadWrite\.All/);
  assert.equal(sb.jerk(['config', 'check']).status, 0);
});

const GUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

function holdLoopback() {
  return new Promise((resolve) => {
    const server = createServer();
    const close = () => new Promise((done) => server.close(() => done()));
    server.once('error', () => resolve(async () => {}));
    server.listen(56123, '127.0.0.1', () => resolve(close));
  });
}

async function authorize(sb, args) {
  const release = await holdLoopback();
  const cdp = await stubCdp({
    paused: [
      {
        resourceType: 'Document',
        request: {
          url: 'http://localhost:56123/?code=fake-code&state=not-the-state',
          method: 'GET',
          headers: {},
        },
      },
    ],
  });
  try {
    const res = await sb.jerkAsync(['microsoft-graph', 'create', ...args, '--cdp', cdp.wsUrl], {
      SLICC_JERK_STORE: 'file',
    });
    const nav = cdp
      .log()
      .split('\n')
      .filter((line) => line.startsWith('>> '))
      .map((line) => JSON.parse(line.slice(3)))
      .find((msg) => msg.method === 'Page.navigate');
    assert.ok(nav, `${res.stderr}\n--- cdp ---\n${cdp.log()}`);
    return { res, url: new URL(nav.params.url) };
  } finally {
    await Promise.all([cdp.close(), release()]);
  }
}

test('the Entra sign-in sends the Graph Command Line Tools client id', async () => {
  const sb = sandbox();
  const { res, url } = await authorize(sb, ['--force']);
  assert.notEqual(res.status, 0);
  assert.equal(url.host, 'login.microsoftonline.com');
  const clientId = url.searchParams.get('client_id');
  assert.match(clientId, GUID);
  assert.equal(clientId, '14d82eec-204b-4c2f-b7e8-296a70dab67e');
  assert.match(url.searchParams.get('scope'), /Files\.ReadWrite\.All/);
});

const TEAMS = 'Chat.ReadWrite ChannelMessage.Read.All Team.ReadBasic.All Channel.ReadBasic.All';
const ENTRA = 'eyJhbGciOiJub25lIn0.eyJzY3AiOiJDaGF0LlJlYWRXcml0ZSJ9.entra';

function grantFile(sb, scope) {
  let h = 0xcbf29ce484222325n;
  for (const b of Buffer.from('microsoft-graph\0default')) {
    h = ((h ^ BigInt(b)) * 0x100000001b3n) & ((1n << 128n) - 1n);
  }
  const name = Buffer.from(h.toString(16).padStart(32, '0'), 'hex').reverse().toString('hex');
  const dir = join(sb.stateDir, 'grants');
  mkdirSync(dir, { recursive: true });
  const grant = {
    provider: 'microsoft-graph',
    user: 'default',
    refresh_token: '',
    access_token: ENTRA,
    expires_at: 4700000000,
    scope,
  };
  writeFileSync(
    join(dir, `${name}.json`),
    JSON.stringify({ provider: 'microsoft-graph', user: 'default', grant })
  );
}

test('create --scope with Teams scopes skips az and asks Entra for exactly those scopes', async () => {
  const sb = sandbox();
  az(sb);
  const { res, url } = await authorize(sb, ['--scope', TEAMS]);
  assert.notEqual(res.status, 0);
  assert.match(res.stderr, /state/);
  assert.equal(url.searchParams.get('scope'), TEAMS);
  assert.equal(url.searchParams.get('client_id'), '14d82eec-204b-4c2f-b7e8-296a70dab67e');
  assert.deepEqual(
    sb.calls().filter((c) => c.args[0] === 'login'),
    []
  );
});

test('get --scope reads the Entra grant, not az, and exits 3 when it does not cover the scope', () => {
  const sb = sandbox();
  az(sb);
  grantFile(sb, 'openid profile User.Read Mail.ReadWrite offline_access');
  const env = { SLICC_JERK_STORE: 'file' };
  const narrow = sb.jerk(['microsoft-graph', 'get', '--scope', 'Chat.ReadWrite'], env);
  assert.equal(narrow.status, 3);
  assert.match(narrow.stderr, /does not cover Chat\.ReadWrite; run jerk microsoft-graph create/);
  assert.deepEqual(sb.calls(), []);

  grantFile(sb, `openid profile User.Read ${TEAMS}`);
  const wide = sb.jerk(['microsoft-graph', 'get', '--scope', 'Chat.ReadWrite', '--json'], env);
  assert.equal(wide.status, 0, wide.stderr);
  const doc = JSON.parse(wide.stdout);
  assert.equal(doc.source, 'oauth');
  assert.ok(doc.scopes.includes('ChannelMessage.Read.All'));
  const all = sb.jerk(['microsoft-graph', 'get', '--scope', TEAMS, '--reveal'], env);
  assert.equal(all.status, 0, all.stderr);
  assert.equal(all.stdout, `${ENTRA}\n`);
  assert.deepEqual(sb.calls(), []);

  const again = sb.jerk(['microsoft-graph', 'create', '--scope', TEAMS], env);
  assert.equal(again.status, 0, again.stderr);
  assert.match(again.stdout, /already signed in/);
  assert.deepEqual(sb.calls(), []);

  const plain = sb.jerk(['microsoft-graph', 'get', '--reveal'], env);
  assert.equal(plain.stdout, `${TOKEN}\n`);
  assert.deepEqual(
    sb.calls().map((c) => c.args.slice(0, 2)),
    [['account', 'get-access-token']]
  );
});

test('get -p scope= checks coverage like --scope', () => {
  const sb = sandbox();
  az(sb);
  grantFile(sb, 'openid profile User.Read');
  const env = { SLICC_JERK_STORE: 'file' };
  const res = sb.jerk(['microsoft-graph', 'get', '-p', 'scope=Chat.ReadWrite'], env);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /does not cover Chat\.ReadWrite/);
  assert.deepEqual(sb.calls(), []);
});

test('drop --scope forgets the Entra grant instead of signing az out', () => {
  const sb = sandbox();
  az(sb);
  grantFile(sb, `openid profile ${TEAMS}`);
  const env = { SLICC_JERK_STORE: 'file' };
  const dropped = sb.jerk(['microsoft-graph', 'drop', '--scope', 'Chat.ReadWrite', '--yes'], env);
  assert.equal(dropped.status, 0, dropped.stderr);
  assert.deepEqual(sb.calls(), []);
  const after = sb.jerk(['microsoft-graph', 'get', '--scope', 'Chat.ReadWrite'], env);
  assert.equal(after.status, 3);
  assert.doesNotMatch(after.stderr, /does not cover/);
  assert.equal(sb.jerk(['microsoft-graph', 'drop', '--yes'], env).status, 0);
  assert.deepEqual(
    sb.calls().map((c) => c.args),
    [['logout']]
  );
});
