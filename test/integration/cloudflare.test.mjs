import assert from 'node:assert/strict';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
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

function policyReader(sb, exit = 0) {
  const seen = join(sb.dir, 'seen.json');
  sb.script(
    'cf',
    `if (args[0] === '--version') { process.stdout.write('1.0.0-beta.16\\n'); process.exit(0); }
const at = args[args.indexOf('--policies') + 1];
const text = at.startsWith('@') ? fs.readFileSync(at.slice(1), 'utf8') : null;
const mode = at.startsWith('@') ? fs.statSync(at.slice(1)).mode & 0o777 : null;
fs.writeFileSync(${JSON.stringify(seen)}, JSON.stringify({ at, text, mode }));
process.stdout.write(JSON.stringify({ value: ${JSON.stringify(CF_TOKEN)} }));
process.exit(${exit});`
  );
  return seen;
}

function leftovers(sb) {
  return existsSync(sb.stateDir)
    ? readdirSync(sb.stateDir).filter((name) => name.startsWith('param-'))
    : [];
}

const POLICIES = '[{"effect":"allow","resources":{"a":"*"},"note":"50% (x) & <y>|^!"}]';

test('policies reach cf in a private file and not on the command line', () => {
  const sb = sandbox();
  const seen = policyReader(sb);
  const got = sb.jerk(['cloudflare', 'get', '--reveal', '-p', `policies=${POLICIES}`]);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${CF_TOKEN}\n`);
  const { at, text } = JSON.parse(readFileSync(seen, 'utf8'));
  assert.equal(text, POLICIES);
  assert.ok(sb.calls().every((c) => c.args.every((arg) => !arg.includes('"'))));
  assert.ok(!existsSync(at.slice(1)));
  assert.deepEqual(leftovers(sb), []);
});

test('a failing cf leaves no policy file and the chain moves on', () => {
  const sb = sandbox();
  const seen = policyReader(sb, 1);
  wrangler(sb);
  const got = sb.jerk(['cloudflare', 'get', '--reveal', '-p', `policies=${POLICIES}`]);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${WR_TOKEN}\n`);
  assert.equal(JSON.parse(readFileSync(seen, 'utf8')).text, POLICIES);
  assert.deepEqual(leftovers(sb), []);
});

test('a policies value that names a file reaches cf as it is', () => {
  const sb = sandbox();
  const seen = policyReader(sb);
  const file = join(sb.dir, 'policies.json');
  writeFileSync(file, POLICIES);
  const got = sb.jerk(['cloudflare', 'get', '--reveal', '-p', `policies=@${file}`]);
  assert.equal(got.status, 0, got.stderr);
  const { at, text } = JSON.parse(readFileSync(seen, 'utf8'));
  assert.deepEqual({ at, text }, { at: `@${file}`, text: POLICIES });
});

test('empty policies pass no flag and write no file', () => {
  const sb = sandbox();
  cf(sb, { mint: true });
  wrangler(sb);
  const got = sb.jerk(['cloudflare', 'get', '--reveal']);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${WR_TOKEN}\n`);
  const mint = sb.calls().find((c) => c.name === 'cf' && c.args[2] === 'create');
  assert.ok(!mint.args.includes('--policies'));
  assert.deepEqual(leftovers(sb), []);
});

test('an unwritable state directory fails the cf step and the chain moves on', () => {
  const sb = sandbox();
  policyReader(sb);
  wrangler(sb);
  const state = join(sb.dir, 'state-file');
  writeFileSync(state, '');
  const got = sb.jerk(['cloudflare', 'get', '--reveal', '-p', `policies=${POLICIES}`], {
    SLICC_JERK_STATE_DIR: state,
  });
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${WR_TOKEN}\n`);
  assert.ok(!sb.calls().some((c) => c.name === 'cf' && c.args[0] === 'accounts'));
});

const unix = { skip: process.platform === 'win32' };
const NOT_PRIVATE = 'the state directory is not private';

test('a state directory jerk makes and the policy file in it are private', unix, () => {
  const sb = sandbox();
  const seen = policyReader(sb);
  const got = sb.jerk(['cloudflare', 'get', '--reveal', '-p', `policies=${POLICIES}`]);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(JSON.parse(readFileSync(seen, 'utf8')).mode, 0o600);
  assert.equal(statSync(sb.stateDir).mode & 0o777, 0o700);
});

test('an existing state directory that only its owner can write is used', unix, () => {
  const sb = sandbox();
  const seen = policyReader(sb);
  const state = join(sb.dir, 'kept');
  mkdirSync(state);
  chmodSync(state, 0o755);
  const got = sb.jerk(['cloudflare', 'get', '--reveal', '-p', `policies=${POLICIES}`], {
    SLICC_JERK_STATE_DIR: state,
  });
  assert.equal(got.status, 0, got.stderr);
  assert.equal(JSON.parse(readFileSync(seen, 'utf8')).text, POLICIES);
  assert.equal(statSync(state).mode & 0o777, 0o755);
});

test('a state directory others can reach stops the cf step', unix, () => {
  const refuses = (sb, state) => {
    policyReader(sb);
    const got = sb.jerk(['cloudflare', 'get', '--reveal', '-p', `policies=${POLICIES}`], {
      SLICC_JERK_STATE_DIR: state,
    });
    assert.notEqual(got.status, 0, state);
    assert.ok(got.stderr.includes(NOT_PRIVATE), got.stderr);
    assert.ok(!got.stderr.includes(state), got.stderr);
    assert.ok(!sb.calls().some((c) => c.args[0] === 'accounts'));
  };
  for (const mode of [0o777, 0o770, 0o722]) {
    const sb = sandbox();
    const state = join(sb.dir, 'open');
    mkdirSync(state);
    chmodSync(state, mode);
    refuses(sb, state);
    assert.deepEqual(readdirSync(state), []);
  }
  for (const tail of ['', '/', '/.']) {
    const sb = sandbox();
    const target = join(sb.dir, 'target');
    mkdirSync(target, { mode: 0o700 });
    symlinkSync(target, join(sb.dir, 'link'));
    refuses(sb, `${join(sb.dir, 'link')}${tail}`);
    assert.deepEqual(readdirSync(target), []);
  }
});

test('a state directory owned by someone else stops the cf step', {
  skip: process.platform === 'win32' || process.getuid() === 0,
}, () => {
  const sb = sandbox();
  policyReader(sb);
  const got = sb.jerk(['cloudflare', 'get', '--reveal', '-p', `policies=${POLICIES}`], {
    SLICC_JERK_STATE_DIR: '/usr',
  });
  assert.notEqual(got.status, 0);
  assert.ok(got.stderr.includes(NOT_PRIVATE), got.stderr);
});

test('a policies value that names a file is refused over the link', () => {
  const sb = sandbox();
  policyReader(sb);
  wrangler(sb);
  const file = join(sb.dir, 'policies.json');
  writeFileSync(file, POLICIES);
  const res = sb.jerk(['cloudflare', 'lease', '--via-link', '-p', `policies=@${file}`]);
  assert.equal(res.status, 1, res.stdout);
  assert.ok(!`${res.stdout}${res.stderr}`.includes(file));
  assert.ok(!`${res.stdout}${res.stderr}`.includes(CF_TOKEN));
  assert.ok(!sb.calls().some((c) => c.args[0] === 'accounts' || c.args[0] === 'auth'));
});

test('a refused file value over the link names the reason', () => {
  const sb = sandbox();
  sb.script('demo', 'process.exit(1);');
  sb.writeUser(`[providers.demo]
env = ["DEMO_TOKEN"]
requires = "demo"
params = { value = "" }
get = { run = ["demo", "token", "{value}"], at_file = ["value"] }
create = { run = ["demo", "login"], interactive = true, link = true }
`);
  const res = sb.jerk(['demo', 'create', '--via-link', '-p', 'value=@secret.json']);
  assert.equal(res.status, 1, res.stdout);
  assert.ok(res.stdout.includes('cannot be used over the link'), res.stdout);
  assert.ok(!res.stdout.includes('launcher'), res.stdout);
  assert.ok(!res.stdout.includes('secret.json'), res.stdout);
  assert.deepEqual(sb.calls(), []);
});

test('jerk still passes its own file over the link', () => {
  const sb = sandbox();
  const seen = policyReader(sb);
  const res = sb.jerk(['cloudflare', 'lease', '--via-link', '-p', `policies=${POLICIES}`]);
  assert.equal(res.status, 0, res.stdout);
  assert.equal(JSON.parse(readFileSync(seen, 'utf8')).text, POLICIES);
  assert.deepEqual(leftovers(sb), []);
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
