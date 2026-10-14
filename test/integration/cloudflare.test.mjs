import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const CF_TOKEN = 'cfmintFAKEFAKEFAKE5678xxxxxxxxxxxxxxxxxxxx';
const WR_TOKEN = 'cfoauthFAKEFAKEFAKE5678';
const LEAK = 'LEAKY-CF-OUTPUT';

function cf(sb, { version = '1.0.0-beta.16', mint = false } = {}) {
  sb.script(
    'cf',
    `if (args[0] === '--version') { process.stdout.write(${JSON.stringify(`${version}\n`)}); process.exit(0); }
if (args[0] === 'accounts' && args[1] === 'tokens' && args[2] === 'create') {
  if (!args.includes('--policies')) { process.stderr.write('missing policies\\n'); process.exit(1); }
  process.stdout.write(JSON.stringify({ id: 'tok_cf_1', value: ${JSON.stringify(mint ? CF_TOKEN : '')} }));
  process.exit(${mint ? 0 : 1});
}
if (args[0] === 'auth' && args[1] === 'whoami') {
  process.stdout.write(JSON.stringify({ authenticated: true, accounts: [] }));
  process.exit(0);
}
if (args[0] === 'auth' && (args[1] === 'login' || args[1] === 'logout')) { process.exit(0); }
process.stdout.write(${JSON.stringify(`${LEAK}\n`)});
process.exit(1);`
  );
}

function wrangler(sb, version = '4.151.0') {
  sb.script(
    'wrangler',
    `if (args[0] === '--version') { process.stdout.write(${JSON.stringify(` ⛅️ wrangler ${version}\n`)}); process.exit(0); }
if (args[0] === 'auth') { process.stdout.write(JSON.stringify({ type: 'oauth', token: ${JSON.stringify(WR_TOKEN)} })); process.exit(0); }
if (args[0] === 'whoami') { process.stdout.write(JSON.stringify({ loggedIn: true, accounts: [] })); process.exit(0); }
process.exit(0);`
  );
}

function names(sb) {
  return [...new Set(sb.calls().map((c) => c.name))];
}

test('with only cf and empty policies, get falls through to oauth and create uses cf', () => {
  const sb = sandbox();
  cf(sb);
  const got = sb.jerk(['cloudflare', 'get']);
  assert.equal(got.status, 3, got.stderr);
  assert.match(got.stderr, /\n {2}cf: /);
  assert.match(got.stderr, /\n {2}wrangler: not on PATH\n {2}oauth: not signed in\n/);
  assert.doesNotMatch(got.stdout + got.stderr, new RegExp(LEAK));
  assert.equal(sb.jerk(['cloudflare', 'create', '--force', '--scope', 'account:read']).status, 0);
  assert.equal(sb.jerk(['cloudflare', 'list']).status, 0);
  assert.equal(sb.jerk(['cloudflare', 'drop', '--yes']).status, 0);
  const runs = sb
    .calls()
    .filter((c) => c.name === 'cf' && c.args[0] !== '--version')
    .map((c) => c.args.slice(0, 3));
  assert.deepEqual(runs, [
    ['accounts', 'tokens', 'create'],
    ['auth', 'login', '--scopes'],
    ['auth', 'whoami'],
    ['auth', 'logout'],
  ]);
});

test('with only wrangler, get list create and drop behave as before', () => {
  const sb = sandbox();
  wrangler(sb);
  const got = sb.jerk(['cloudflare', 'get', '--reveal']);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${WR_TOKEN}\n`);
  assert.equal(sb.jerk(['cloudflare', 'list']).status, 0);
  assert.equal(sb.jerk(['cloudflare', 'create', '--force', '--scope', 'account:read']).status, 0);
  assert.equal(sb.jerk(['cloudflare', 'drop', '--yes']).status, 0);
  const runs = sb
    .calls()
    .filter((c) => c.args[0] !== '--version')
    .map((c) => c.args);
  assert.deepEqual(runs, [
    ['auth', 'token', '--json'],
    ['whoami', '--json'],
    ['login', '--scopes', 'account:read'],
    ['logout'],
  ]);
});

test('with cf and wrangler, create prefers cf while get falls to wrangler without policies', () => {
  const sb = sandbox();
  cf(sb);
  wrangler(sb);
  const got = sb.jerk(['cloudflare', 'get', '--reveal']);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${WR_TOKEN}\n`);
  assert.equal(sb.jerk(['cloudflare', 'create', '--force', '--scope', 'account:read']).status, 0);
  assert.deepEqual(names(sb).sort(), ['cf', 'wrangler']);
  const listed = sb.jerk(['list']);
  assert.match(listed.stdout, /\ncloudflare\s.*\scf, wrangler, oauth\n/);
});

test('with policies, get mints through cf before trying wrangler', () => {
  const sb = sandbox();
  cf(sb, { mint: true });
  wrangler(sb);
  const policies =
    '[{"effect":"allow","resources":{"com.cloudflare.api.account.acc":"*"},"permission_groups":[{"id":"1","name":"Workers Scripts Read"}]}]';
  const got = sb.jerk(['cloudflare', 'get', '--reveal', '-p', `policies=${policies}`]);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${CF_TOKEN}\n`);
  assert.ok(!sb.calls().some((c) => c.name === 'wrangler' && c.args[0] === 'auth'));
  const mint = sb.calls().find((c) => c.name === 'cf' && c.args[2] === 'create');
  assert.ok(mint, JSON.stringify(sb.calls()));
  assert.ok(mint.args.includes('--policies'));
  assert.ok(mint.args.includes('--name'));
});

test('an old wrangler beside cf still signs in with cf', () => {
  const sb = sandbox();
  cf(sb);
  wrangler(sb, '4.14.4');
  const create = sb.jerk(['cloudflare', 'create', '--force', '--scope', 'account:read']);
  assert.equal(create.status, 0, create.stderr);
  assert.ok(sb.calls().some((c) => c.name === 'cf' && c.args[1] === 'login'));
  const got = sb.jerk(['cloudflare', 'get']);
  assert.equal(got.status, 3, got.stderr);
  assert.match(got.stderr, /wrangler: 4\.14\.4 is too old/);
  assert.match(got.stderr, /oauth: not signed in/);
});

test('with neither CLI, get reaches oauth and list --json shows the chain', () => {
  const sb = sandbox();
  const res = sb.jerk(['cloudflare', 'get']);
  assert.equal(res.status, 3);
  assert.match(
    res.stderr,
    /\n {2}cf: not on PATH\n {2}wrangler: not on PATH\n {2}oauth: not signed in\n/
  );
  const entry = JSON.parse(sb.jerk(['list', '--json']).stdout).find(
    (p) => p.provider === 'cloudflare'
  );
  assert.deepEqual(entry.steps, [
    { cli: 'cf', found: false },
    { cli: 'wrangler', found: false },
    { oauth: true },
  ]);
});
