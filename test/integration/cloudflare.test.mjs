import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'cfoauthFAKEFAKEFAKE5678';
const LEAK = 'LEAKY-CF-OUTPUT';

function cf(sb, version = '1.0.0-beta.17') {
  sb.script(
    'cf',
    `if (args[0] === '--version') { process.stdout.write(${JSON.stringify(`${version}\n`)}); process.exit(0); }
process.stdout.write(${JSON.stringify(`${LEAK}\n`)});`
  );
}

function wrangler(sb, version = '4.151.0') {
  sb.script(
    'wrangler',
    `if (args[0] === '--version') { process.stdout.write(${JSON.stringify(` ⛅️ wrangler ${version}\n`)}); process.exit(0); }
if (args[0] === 'auth') { process.stdout.write(JSON.stringify({ type: 'oauth', token: ${JSON.stringify(TOKEN)} })); process.exit(0); }
if (args[0] === 'whoami') { process.stdout.write(JSON.stringify({ loggedIn: true, accounts: [] })); process.exit(0); }
process.exit(0);`
  );
}

function names(sb) {
  return [...new Set(sb.calls().map((c) => c.name))];
}

test('with only cf, every verb exits 4 for the missing wrangler and cf is never run', () => {
  const sb = sandbox();
  cf(sb);
  for (const verb of ['get', 'list', 'create', 'drop']) {
    const res = sb.jerk(['cloudflare', verb, ...(verb === 'drop' ? ['--yes'] : [])]);
    assert.equal(res.status, 4, `${verb}: ${res.stderr}`);
    assert.match(res.stderr, /wrangler is not on PATH/);
    assert.doesNotMatch(res.stdout + res.stderr, new RegExp(LEAK));
  }
  assert.deepEqual(sb.calls(), []);
});

test('with only wrangler, get, list, create and drop behave as before', () => {
  const sb = sandbox();
  wrangler(sb);
  const got = sb.jerk(['cloudflare', 'get', '--reveal']);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${TOKEN}\n`);
  assert.equal(sb.jerk(['cloudflare', 'list']).status, 0);
  assert.equal(sb.jerk(['cloudflare', 'create', '--scope', 'account:read']).status, 0);
  assert.equal(sb.jerk(['cloudflare', 'drop', '--yes']).status, 0);
  const runs = sb
    .calls()
    .filter((c) => c.args[0] !== '--version')
    .map((c) => c.args);
  assert.deepEqual(runs, [
    ['auth', 'token', '--json'],
    ['whoami', '--json'],
    ['auth', 'token', '--json'],
    ['login', '--scopes', 'account:read'],
    ['logout'],
  ]);
});

test('with cf and wrangler, wrangler serves every verb so the sign-in matches the token', () => {
  const sb = sandbox();
  cf(sb);
  wrangler(sb);
  const got = sb.jerk(['cloudflare', 'get', '--reveal']);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${TOKEN}\n`);
  assert.equal(sb.jerk(['cloudflare', 'list']).status, 0);
  assert.equal(sb.jerk(['cloudflare', 'create', '--scope', 'account:read']).status, 0);
  assert.equal(sb.jerk(['cloudflare', 'drop', '--yes']).status, 0);
  assert.deepEqual(names(sb), ['wrangler']);
  const listed = sb.jerk(['list']);
  assert.match(listed.stdout, /\ncloudflare\s.*\swrangler \(found\)\n/);
});

test('an old wrangler beside cf exits 4 instead of signing in with cf', () => {
  const sb = sandbox();
  cf(sb);
  wrangler(sb, '4.14.4');
  for (const verb of ['get', 'create']) {
    const res = sb.jerk(['cloudflare', verb, ...(verb === 'drop' ? ['--yes'] : [])]);
    assert.equal(res.status, 4, `${verb}: ${res.stderr}`);
    assert.match(res.stderr, /wrangler 4\.14\.4 is too old; this needs 4\.57\.0 or newer/);
  }
  assert.deepEqual(names(sb), ['wrangler']);
});

test('with neither CLI, get exits 4 and names wrangler', () => {
  const sb = sandbox();
  const res = sb.jerk(['cloudflare', 'get']);
  assert.equal(res.status, 4);
  assert.match(res.stderr, /wrangler is not on PATH/);
  const listed = sb.jerk(['list', '--json']);
  const entry = JSON.parse(listed.stdout).find((p) => p.provider === 'cloudflare');
  assert.ok(entry, listed.stdout);
  assert.doesNotMatch(JSON.stringify(entry), /"cf"/);
});
