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

test('get --scope with GH_TOKEN asks gh auth status for the scopes', () => {
  const sb = sandbox();
  fakeGh(sb);
  const env = { GH_TOKEN: 'envtokenvalue1234567' };
  const ok = sb.jerk(['github', 'get', '--scope', 'public_repo', '--json'], env);
  assert.equal(ok.status, 0, ok.stderr);
  assert.equal(JSON.parse(ok.stdout).source, 'env');
  assert.equal(sb.jerk(['github', 'get', '--scope', 'workflow'], env).status, 3);
  assert.deepEqual(
    sb.calls().map((c) => c.args.slice(0, 2)),
    [
      ['auth', 'status'],
      ['auth', 'status'],
    ]
  );
});

test('list redacts tokens', () => {
  const sb = sandbox();
  fakeGh(sb);
  const res = sb.jerk(['github', 'list', '--json']);
  assert.equal(res.status, 0, res.stderr);
  assert.deepEqual(JSON.parse(res.stdout).items[0]['github.com'][0].token, 'gho_…1234');
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
  assert.equal(missing.status, 3);
  assert.match(missing.stderr, /cf: not on PATH/);
  assert.match(missing.stderr, /wrangler: not on PATH/);
  assert.match(missing.stderr, /oauth: not signed in/);
});

test('wrangler tokens get a max_age expiry', () => {
  const sb = sandbox();
  sb.script(
    'wrangler',
    `if (args[0] === '--version') process.stdout.write(' ⛅️ wrangler 4.151.0\\n');
else process.stdout.write(JSON.stringify({ type: 'oauth', token: 'cfoauthtoken123456789' }));`
  );
  const res = JSON.parse(sb.jerk(['cloudflare', 'get', '--json']).stdout);
  assert.equal(res.masked, 'cfoa…6789');
  assert.ok(Date.parse(res.expires_at) > Date.now() + 49 * 60 * 1000);
});

test('npm get reads the npmrc, and an env reference inside it', async () => {
  const sb = sandbox();
  const { mkdirSync, writeFileSync } = await import('node:fs');
  const { join } = await import('node:path');
  mkdirSync(join(sb.dir, 'home'), { recursive: true });
  writeFileSync(join(sb.dir, 'home', '.npmrc'), '//registry.npmjs.org/:_authToken=${MY_NPM}\n');
  const res = sb.jerk(['npm', 'get', '--json'], { MY_NPM: 'npm_FAKEFAKEFAKEFAKE99' });
  assert.equal(res.status, 0, res.stderr);
  assert.deepEqual(JSON.parse(res.stdout), {
    provider: 'npm',
    source: 'file',
    masked: 'npm_…KE99',
    expires_at: null,
    scopes: null,
  });
  const none = sb.jerk(['npm', 'get'], { NPM_CONFIG_USERCONFIG: join(sb.dir, 'nope') });
  assert.equal(none.status, 3);
});

test('npm create mints a token, records only its id, and drop forgets it', async () => {
  const sb = sandbox();
  const { readFileSync } = await import('node:fs');
  const { join } = await import('node:path');
  sb.script(
    'npm',
    `if (args[0] === '--version') process.stdout.write('11.6.3\\n');
else if (args[0] === 'token' && args[1] === 'create') process.stdout.write(JSON.stringify({ token: 'npm_MINTEDMINTEDMINTED42' }));
else if (args[0] === 'token' && args[1] === 'list') process.stdout.write(JSON.stringify([{ token: 'npm_MI', key: 'abcdef123' }]));`
  );
  const res = sb.jerk(['npm', 'create', '--name', 'ci']);
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stdout, 'npm_…ED42\n');
  assert.deepEqual(sb.calls()[1].args, [
    'token',
    'create',
    '--name',
    'ci',
    '--expires',
    '1',
    '--json',
  ]);
  const minted = readFileSync(join(sb.stateDir, 'minted.json'), 'utf8');
  assert.deepEqual(JSON.parse(minted), { npm: ['abcdef123'] });
  assert.ok(!minted.includes('npm_MINTED'));
  assert.equal(sb.jerk(['npm', 'drop', 'abcdef123', '--yes']).status, 0);
  assert.deepEqual(sb.calls().at(-1).args, ['token', 'revoke', 'abcdef123']);
  assert.deepEqual(JSON.parse(readFileSync(join(sb.stateDir, 'minted.json'), 'utf8')), { npm: [] });
});

test('npm create refuses an npm that cannot make granular tokens', () => {
  const sb = sandbox();
  sb.script(
    'npm',
    `if (args[0] === '--version') process.stdout.write('11.4.2\\n'); else process.exit(9);`
  );
  const res = sb.jerk(['npm', 'create']);
  assert.equal(res.status, 4);
  assert.match(res.stderr, /npm 11\.4\.2 is too old; this needs 11\.6\.3 or newer/);
  assert.equal(sb.calls().length, 1);
});

test('an old wrangler is reported as too old, not as signed out', () => {
  const sb = sandbox();
  sb.script(
    'wrangler',
    `if (args[0] === '--version') process.stdout.write(' ⛅️ wrangler 4.14.4\\n'); else process.exit(1);`
  );
  const res = sb.jerk(['cloudflare', 'get']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /wrangler: 4\.14\.4 is too old; this needs 4\.57\.0 or newer/);
  assert.match(res.stderr, /oauth: not signed in/);
});

test('the built-in xai provider signs in at the discovered authorize endpoint', () => {
  const sb = sandbox();
  const res = sb.jerk(['config', 'show', 'xai']);
  assert.equal(res.status, 0);
  assert.match(res.stdout, /authorize_url = "https:\/\/auth\.x\.ai\/oauth2\/authorize\?/);
});
