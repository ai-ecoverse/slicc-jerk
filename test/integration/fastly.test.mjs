import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'fastly_FAKEFAKEFAKEFAKE5678xxxxxxxx';
const PROFILES = {
  user: {
    email: 'user@example.com',
    token: TOKEN,
    type: 'user',
  },
};

function fastly(sb, { token = TOKEN, profiles = PROFILES, failToken = false } = {}) {
  sb.script(
    'fastly',
    `if (args[0] === 'profile' && args[1] === 'token') {
  if (${failToken ? 'true' : 'false'}) { process.stderr.write('ERROR: no profiles available.\\n'); process.exit(1); }
  const name = args.slice(2).find((a) => a !== '--quiet');
  const profiles = ${JSON.stringify(profiles)};
  if (name) {
    if (!profiles[name]) { process.stderr.write('profile missing\\n'); process.exit(1); }
    process.stdout.write(profiles[name].token + '\\n');
    process.exit(0);
  }
  process.stdout.write(${JSON.stringify(`${token}\n`)});
  process.exit(0);
}
if (args[0] === 'profile' && args[1] === 'list') {
  process.stdout.write(JSON.stringify(${JSON.stringify(profiles)}));
  process.exit(0);
}
if (args[0] === 'sso') { process.exit(0); }
if (args[0] === 'profile' && args[1] === 'delete') { process.exit(0); }
process.exit(1);`
  );
}

test('fastly get list create and drop dispatch to the CLI', () => {
  const sb = sandbox();
  fastly(sb);
  const got = sb.jerk(['fastly', 'get', '--reveal']);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${TOKEN}\n`);
  assert.deepEqual(sb.calls()[0].args, ['profile', 'token', '--quiet']);

  const listed = sb.jerk(['fastly', 'list', '--json']);
  assert.equal(listed.status, 0, listed.stderr);
  assert.deepEqual(JSON.parse(listed.stdout), {
    items: [{ user: { ...PROFILES.user, token: 'fast…xxxx' } }],
  });

  assert.match(sb.jerk(['fastly', 'create']).stdout, /already signed in/);
  const forced = sb.jerk(['fastly', 'create', '--force']);
  assert.equal(forced.status, 0, forced.stderr);
  assert.equal(forced.stdout, 'fastly: signed in\n');
  assert.deepEqual(sb.calls().at(-1).args, ['sso']);

  assert.equal(sb.jerk(['fastly', 'drop']).status, 2);
  const dropped = sb.jerk(['fastly', 'drop', 'user', '--yes']);
  assert.equal(dropped.status, 0, dropped.stderr);
  assert.deepEqual(sb.calls().at(-1).args, ['profile', 'delete', 'user', '--auto-yes']);
});

test('fastly get prefers FASTLY_API_TOKEN and accepts -p profile', () => {
  const sb = sandbox();
  fastly(sb, {
    profiles: {
      work: { email: 'work@example.com', token: 'fastly_WORKTOKENFAKE5678xxxx', type: 'user' },
      user: PROFILES.user,
    },
  });
  const fromEnv = sb.jerk(['fastly', 'get', '--json'], {
    FASTLY_API_TOKEN: 'envtokenvalue1234567',
  });
  assert.equal(fromEnv.status, 0, fromEnv.stderr);
  assert.equal(JSON.parse(fromEnv.stdout).source, 'env');
  assert.equal(sb.calls().length, 0);

  const named = sb.jerk(['fastly', 'get', '--reveal', '-p', 'profile=work']);
  assert.equal(named.status, 0, named.stderr);
  assert.equal(named.stdout, 'fastly_WORKTOKENFAKE5678xxxx\n');
  assert.deepEqual(sb.calls().at(-1).args, ['profile', 'token', 'work', '--quiet']);
});

test('fastly without a profile exits 3, and without the CLI exits 4', () => {
  const sb = sandbox();
  fastly(sb, { failToken: true, profiles: {} });
  const unsigned = sb.jerk(['fastly', 'get']);
  assert.equal(unsigned.status, 3);
  assert.match(unsigned.stderr, /sign in with jerk fastly create/);

  const missing = sandbox();
  const res = missing.jerk(['fastly', 'get']);
  assert.equal(res.status, 4);
  assert.match(res.stderr, /fastly is not on PATH/);
});

test('jerk list reports the fastly CLI', () => {
  const sb = sandbox();
  sb.fakeCli('fastly');
  const lines = sb.jerk(['list']).stdout.trimEnd().split('\n');
  assert.ok(
    lines.some((l) => /^fastly\s+Fastly\s+built-in\s+fastly \(found\)$/.test(l)),
    sb.jerk(['list']).stdout
  );
  const entry = JSON.parse(sb.jerk(['list', '--json']).stdout).find((p) => p.provider === 'fastly');
  assert.equal(entry.requires, 'fastly');
  assert.equal(entry.cli, true);
  assert.deepEqual(entry.env, ['FASTLY_API_TOKEN']);
});
