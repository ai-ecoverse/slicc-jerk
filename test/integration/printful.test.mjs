import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import { test } from 'node:test';
import { join } from 'node:path';
import { sandbox } from './helpers.mjs';

const TOKEN = 'pf_store_FAKEFAKEFAKEFAKE9012';
const MINTED = 'pf_mint_FAKEFAKEFAKEFAKE3456';

function fakePrintful(sb, { token = TOKEN } = {}) {
  const store = join(sb.dir, 'printful-token.txt');
  sb.script(
    'printful',
    `const store = ${JSON.stringify(store)};
const read = () => (fs.existsSync(store) ? fs.readFileSync(store, 'utf8').trim() : '');
const write = (t) => fs.writeFileSync(store, t + '\\n');
const [a, b, ...rest] = args;
if (a === 'auth' && b === 'token') {
  const t = read();
  if (!t) { process.stderr.write('not authenticated\\n'); process.exit(1); }
  process.stdout.write(t + '\\n');
  process.exit(0);
}
if (a === 'auth' && b === 'status') {
  const t = read();
  if (!t) {
    process.stdout.write(JSON.stringify({ authenticated: false }));
    process.exit(1);
  }
  process.stdout.write(JSON.stringify({
    authenticated: true,
    token: '…' + t.slice(-4),
    source: 'private-token',
    store_id: 42,
    stores: [{ id: 42, name: 'Personal orders', type: 'native' }],
  }));
  process.exit(0);
}
if (a === 'auth' && b === 'login') {
  let tok = '';
  for (let i = 0; i < rest.length; i++) {
    if (rest[i] === '--token') tok = rest[i + 1] || '';
  }
  write(tok || ${JSON.stringify(MINTED)});
  process.stderr.write('fake login\\n');
  process.exit(0);
}
if (a === 'auth' && b === 'logout') {
  if (fs.existsSync(store)) fs.unlinkSync(store);
  process.stderr.write('fake logout\\n');
  process.exit(0);
}
if (a === 'whoami') {
  const t = read();
  if (!t) { process.stderr.write('not authenticated\\n'); process.exit(1); }
  process.stdout.write(JSON.stringify({
    token: '…' + t.slice(-4),
    source: 'private-token',
    stores: [{ id: 42, name: 'Personal orders', type: 'native' }],
  }));
  process.exit(0);
}
process.exit(9);`
  );
  if (token) {
    writeFileSync(store, `${token}\n`);
  }
}

test('printful get uses the CLI token, and env wins', () => {
  const sb = sandbox();
  fakePrintful(sb);
  const masked = sb.jerk(['printful', 'get']);
  assert.equal(masked.status, 0, masked.stderr);
  assert.equal(masked.stdout, 'pf_…9012\n');
  const revealed = sb.jerk(['printful', 'get', '--reveal']);
  assert.equal(revealed.stdout, `${TOKEN}\n`);
  assert.deepEqual(sb.calls()[0].args, ['auth', 'token']);
  const fromEnv = sb.jerk(['printful', 'get', '--json'], {
    PRINTFUL_API_TOKEN: 'pf_env_FAKEFAKEFAKEENV99',
  });
  assert.equal(JSON.parse(fromEnv.stdout).source, 'env');
  assert.equal(sb.calls().length, 2);
});

test('printful list reads auth status and redacts the token', () => {
  const sb = sandbox();
  fakePrintful(sb);
  const listed = sb.jerk(['printful', 'list', '--json']);
  assert.equal(listed.status, 0, listed.stderr);
  const items = JSON.parse(listed.stdout).items;
  assert.equal(items[0].authenticated, true);
  assert.equal(items[0].stores[0].name, 'Personal orders');
  assert.ok(!listed.stdout.includes(TOKEN));
  assert.deepEqual(sb.calls()[0].args, ['auth', 'status', '--json']);
});

test('printful config check accepts the builtin provider', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'printful']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /\[providers\.printful\]/);
  assert.match(shown.stdout, /auth", "token"/);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0, check.stderr);
});

test('printful create stores a supplied token, and drop clears it', () => {
  const sb = sandbox();
  fakePrintful(sb, { token: '' });
  const missing = sb.jerk(['printful', 'get']);
  assert.equal(missing.status, 3);
  const created = sb.jerk(['printful', 'create', '-p', `token=${TOKEN}`, '--force']);
  assert.equal(created.status, 0, created.stderr);
  assert.equal(created.stdout, 'printful: signed in\n');
  const login = sb.calls().find((c) => c.args[0] === 'auth' && c.args[1] === 'login');
  assert.deepEqual(login.args, [
    'auth',
    'login',
    '--token',
    TOKEN,
    '--name',
    'jerk',
    '--days',
    '90',
  ]);
  assert.equal(sb.jerk(['printful', 'get', '--reveal']).stdout, `${TOKEN}\n`);
  assert.equal(sb.jerk(['printful', 'drop', '--yes']).status, 0);
  assert.deepEqual(sb.calls().at(-1).args, ['auth', 'logout']);
  assert.equal(sb.jerk(['printful', 'get']).status, 3);
});

test('printful create without token mints via the CLI', () => {
  const sb = sandbox();
  fakePrintful(sb, { token: '' });
  const created = sb.jerk(['printful', 'create', '--force']);
  assert.equal(created.status, 0, created.stderr);
  const login = sb.calls().find((c) => c.args[0] === 'auth' && c.args[1] === 'login');
  assert.deepEqual(login.args, ['auth', 'login', '--name', 'jerk', '--days', '90']);
  assert.equal(sb.jerk(['printful', 'get', '--reveal']).stdout, `${MINTED}\n`);
});

test('printful is unavailable when the CLI is missing', () => {
  const sb = sandbox();
  const res = sb.jerk(['printful', 'get']);
  assert.equal(res.status, 4);
  assert.match(res.stderr, /printful is not on PATH/);
});
