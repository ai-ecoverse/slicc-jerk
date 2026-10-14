import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'cf_FAKEFAKEFAKEFAKE5678';
const LEAK = 'LEAKY-CLI-OUTPUT';

const CHAIN = `[providers.flare]
env = ["FLARE_TOKEN"]
domains = ["api.example.invalid"]

[[providers.flare.get]]
requires = "cf"
run = ["cf", "token"]
parse = "text"

[[providers.flare.get]]
requires = "wrangler"
run = ["wrangler", "auth", "token"]
parse = "text"
min_version = "4.57.0"

[[providers.flare.get]]
requires = "flarectl"
run = ["flarectl", "token"]
parse = "text"
timeout = "3s"

[[providers.flare.create]]
requires = "cf"
run = ["cf", "login"]
interactive = true

[[providers.flare.create]]
requires = "wrangler"
run = ["wrangler", "login"]
interactive = true

[[providers.flare.create]]
requires = "flarectl"
run = ["flarectl", "login"]
interactive = true
`;

function wrangler(sb, { version = '4.60.0', token = '', exit = 0 } = {}) {
  sb.script(
    'wrangler',
    `if (args[0] === '--version') { process.stdout.write(${JSON.stringify(`${version}\n`)}); process.exit(0); }
process.stdout.write(${JSON.stringify(token || `${LEAK}\n`)});
process.stderr.write(${JSON.stringify(`${LEAK}\n`)});
process.exit(${token ? 0 : exit || 1});`
  );
}

function flarectl(sb, body) {
  sb.script('flarectl', body);
}

test('get skips a missing CLI, moves past a failing one and returns the first token', () => {
  const sb = sandbox();
  sb.writeUser(CHAIN);
  wrangler(sb);
  flarectl(sb, `process.stdout.write(${JSON.stringify(`${TOKEN}\n`)});`);
  const res = sb.jerk(['flare', 'get', '--reveal']);
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stdout, `${TOKEN}\n`);
  assert.equal(res.stderr, '');
  assert.deepEqual(
    sb.calls().map((c) => [c.name, c.args[0]]),
    [
      ['wrangler', '--version'],
      ['wrangler', 'auth'],
      ['flarectl', 'token'],
    ]
  );
});

test('get exits 3 and lists every step when the available ones all fail', () => {
  const sb = sandbox();
  sb.writeUser(CHAIN);
  wrangler(sb);
  flarectl(sb, 'setTimeout(() => {}, 8000);');
  const res = sb.jerk(['flare', 'get']);
  assert.equal(res.status, 3);
  assert.equal(res.stdout, '');
  assert.match(res.stderr, /^jerk: flare: no get step worked:\n/);
  assert.match(res.stderr, /\n {2}cf: not on PATH\n/);
  assert.match(res.stderr, /\n {2}wrangler: get exited 1\n/);
  assert.match(res.stderr, /\n {2}flarectl: did not answer in time\n/);
  assert.match(res.stderr, /sign in with jerk flare create\n$/);
  assert.doesNotMatch(res.stderr, new RegExp(LEAK));
});

test('get exits 4 when no step is available', () => {
  const sb = sandbox();
  sb.writeUser(CHAIN);
  wrangler(sb, { version: '4.20.0', token: `${TOKEN}\n` });
  const res = sb.jerk(['flare', 'get']);
  assert.equal(res.status, 4);
  assert.match(res.stderr, /\n {2}cf: not on PATH\n/);
  assert.match(res.stderr, /\n {2}wrangler: 4\.20\.0 is too old; this needs 4\.57\.0 or newer\n/);
  assert.match(res.stderr, /\n {2}flarectl: not on PATH\n$/);
  assert.doesNotMatch(res.stderr, new RegExp(TOKEN));
  assert.deepEqual(
    sb.calls().map((c) => c.args[0]),
    ['--version']
  );
});

test('the env variable still wins over every step', () => {
  const sb = sandbox();
  sb.writeUser(CHAIN);
  wrangler(sb);
  const res = sb.jerk(['flare', 'get', '--json'], { FLARE_TOKEN: 'envtokenvalue1234567' });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).source, 'env');
  assert.deepEqual(sb.calls(), []);
});

test('create runs only the first available step, and its failure is final', () => {
  const sb = sandbox();
  sb.writeUser(CHAIN);
  wrangler(sb, { exit: 7 });
  flarectl(sb, 'process.exit(0);');
  const failed = sb.jerk(['flare', 'create', '--force']);
  assert.equal(failed.status, 1);
  assert.match(failed.stderr, /wrangler create exited 7/);
  assert.ok(!sb.calls().some((c) => c.name === 'flarectl'));
});

test('create signs in with the first available step', () => {
  const sb = sandbox();
  sb.writeUser(CHAIN);
  flarectl(sb, 'process.exit(0);');
  const res = sb.jerk(['flare', 'create', '--force']);
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stdout, 'flare: signed in\n');
  assert.deepEqual(
    sb.calls().map((c) => [c.name, c.args[0]]),
    [['flarectl', 'login']]
  );
});

test('create exits 4 and lists the steps when none is available', () => {
  const sb = sandbox();
  sb.writeUser(CHAIN);
  const res = sb.jerk(['flare', 'create', '--force']);
  assert.equal(res.status, 4);
  assert.equal(
    res.stderr,
    'jerk: flare: no create step worked:\n  cf: not on PATH\n  wrangler: not on PATH\n  flarectl: not on PATH\n'
  );
});

test('an oauth step uses the [oauth] grant and says when there is none', () => {
  const sb = sandbox();
  sb.writeUser(`[providers.flare]
env = ["FLARE_TOKEN"]
store = "file"

[[providers.flare.get]]
requires = "cf"
run = ["cf", "token"]

[[providers.flare.get]]
oauth = true

[providers.flare.oauth]
mode = "device"
flow = "pkce"
authorize_url = "https://idp.example.invalid/device"
token_url = "https://idp.example.invalid/token"
client_id = "placeholder"
redirect_uri = "http://127.0.0.1:1/cb"
capture = { at = "request", from = "query", param = "code" }
`);
  const res = sb.jerk(['flare', 'get']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /\n {2}cf: not on PATH\n {2}oauth: not signed in\n/);
  const list = sb.jerk(['list']);
  assert.match(list.stdout, /\nflare\s+flare\s+user\s+cf \(missing\), oauth\n/);
});

test('jerk list shows each step CLI, and --json adds the steps', () => {
  const sb = sandbox();
  sb.writeUser(CHAIN);
  wrangler(sb);
  const res = sb.jerk(['list']);
  assert.equal(res.status, 0, res.stderr);
  assert.match(
    res.stdout,
    /\nflare\s+flare\s+user\s+cf \(missing\), wrangler, flarectl \(missing\)\n/
  );
  assert.match(res.stdout, /\ngithub\s+GitHub \(gh\)\s+built-in\s+gh \(missing\)\n/);
  const json = JSON.parse(sb.jerk(['list', '--json']).stdout);
  const flare = json.find((p) => p.provider === 'flare');
  assert.deepEqual(flare.steps, [
    { cli: 'cf', found: false },
    { cli: 'wrangler', found: true },
    { cli: 'flarectl', found: false },
  ]);
  assert.equal(flare.cli, null);
  assert.equal(json.find((p) => p.provider === 'github').steps, undefined);
});

test('config check validates every step and config show prints the chain', () => {
  const sb = sandbox();
  sb.writeUser(CHAIN);
  const shown = sb.jerk(['config', 'show', 'flare']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.equal(shown.stdout.match(/\[\[providers\.flare\.get\]\]/g).length, 3);
  sb.writeUser(`[providers.bad]
[[providers.bad.get]]
requires = "a"
run = ["a", "{nope}"]
min_version = "new"

[[providers.bad.get]]
oauth = true
run = ["b"]

[[providers.bad.list]]
oauth = true
`);
  const res = sb.jerk(['config', 'check']);
  assert.equal(res.status, 2);
  for (const message of [
    'get[0]: min_version "new" must look like 11.6.3',
    'get[0]: unknown placeholder {nope}',
    'get[1]: oauth = true cannot have run',
    'get[1]: oauth = true but no [oauth] block',
    'list: oauth = true only applies to get, create and drop',
  ]) {
    assert.ok(res.stderr.includes(message), `${message}\n${res.stderr}`);
  }
});
