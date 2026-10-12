import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'gho_FAKEFAKEFAKEFAKE1234';
const STATUS = {
  hosts: {
    'github.com': [{ login: 'octo', active: true, scopes: 'repo, read:org', token: TOKEN }],
  },
};

function fakeGh(sb) {
  sb.script(
    'gh',
    `const [a, b] = args;
if (a === 'auth' && b === 'token') process.stdout.write(${JSON.stringify(`${TOKEN}\n`)});
else if (a === 'auth' && b === 'status') process.stdout.write(${JSON.stringify(JSON.stringify(STATUS))});
else if (a === 'auth' && (b === 'login' || b === 'logout')) process.stderr.write('fake ' + b + '\\n');
else process.exit(9);`
  );
}

test('get prints a masked token to a pipe, and the token only with --reveal', () => {
  const sb = sandbox();
  fakeGh(sb);
  const masked = sb.jerk(['github', 'get']);
  assert.equal(masked.status, 0, masked.stderr);
  assert.equal(masked.stdout, 'gho_…1234\n');
  const revealed = sb.jerk(['github', 'get', '--reveal']);
  assert.equal(revealed.stdout, `${TOKEN}\n`);
  const json = JSON.parse(sb.jerk(['github', 'get', '--json']).stdout);
  assert.deepEqual(json, {
    provider: 'github',
    source: 'cli',
    masked: 'gho_…1234',
    expires_at: null,
    scopes: null,
  });
  const calls = sb.calls();
  assert.deepEqual(calls[0].args, ['auth', 'token', '--hostname', 'github.com']);
});

test('the provider CLI never sees the provider token variables', () => {
  const sb = sandbox();
  fakeGh(sb);
  sb.jerk(['github', 'list'], { NPM_TOKEN: 'x' });
  assert.deepEqual(sb.calls()[0].env, ['NPM_TOKEN']);
  const fromEnv = sb.jerk(['github', 'get', '--json'], { GH_TOKEN: 'envtokenvalue1234567' });
  assert.equal(JSON.parse(fromEnv.stdout).source, 'env');
  assert.equal(sb.calls().length, 1);
});

test('get --scope checks the granted scopes', () => {
  const sb = sandbox();
  fakeGh(sb);
  assert.equal(sb.jerk(['github', 'get', '--scope', 'public_repo']).status, 0);
  const missing = sb.jerk(['github', 'get', '--scope', 'workflow']);
  assert.equal(missing.status, 3);
  assert.match(missing.stderr, /run jerk github create --scope workflow/);
});

test('list redacts tokens', () => {
  const sb = sandbox();
  fakeGh(sb);
  const res = sb.jerk(['github', 'list', '--json']);
  assert.equal(res.status, 0, res.stderr);
  assert.deepEqual(JSON.parse(res.stdout)['github.com'][0].token, 'gho_…1234');
  assert.ok(!res.stdout.includes(TOKEN));
});

test('create skips a sign-in that already covers the scopes, and drop needs --yes off a terminal', () => {
  const sb = sandbox();
  fakeGh(sb);
  assert.match(sb.jerk(['github', 'create']).stdout, /already signed in/);
  const forced = sb.jerk(['github', 'create', '--force', '--scope', 'workflow']);
  assert.equal(forced.status, 0, forced.stderr);
  assert.equal(forced.stdout, 'github: signed in\n');
  assert.deepEqual(sb.calls().at(-1).args, [
    'auth',
    'login',
    '--web',
    '--hostname',
    'github.com',
    '--scopes',
    'workflow',
  ]);
  assert.equal(sb.jerk(['github', 'drop']).status, 2);
  const dropped = sb.jerk(['github', 'drop', 'octo', '--yes']);
  assert.equal(dropped.status, 0);
  assert.deepEqual(sb.calls().at(-1).args, [
    'auth',
    'logout',
    '--hostname',
    'github.com',
    '--user',
    'octo',
  ]);
});

test('a failing or missing CLI maps to exit 3 and 4', () => {
  const sb = sandbox();
  sb.script('gh', 'process.exit(1);');
  const failed = sb.jerk(['github', 'get']);
  assert.equal(failed.status, 3);
  assert.match(failed.stderr, /sign in with jerk github create/);
  const missing = sb.jerk(['cloudflare', 'get']);
  assert.equal(missing.status, 4);
  assert.match(missing.stderr, /wrangler is not on PATH/);
});

test('wrangler tokens get a max_age expiry', () => {
  const sb = sandbox();
  sb.script(
    'wrangler',
    `process.stdout.write(JSON.stringify({ type: 'oauth', token: 'cfoauthtoken123456789' }));`
  );
  const res = JSON.parse(sb.jerk(['cloudflare', 'get', '--json']).stdout);
  assert.equal(res.masked, 'cfoa…6789');
  assert.ok(Date.parse(res.expires_at) > Date.now() + 49 * 60 * 1000);
});
