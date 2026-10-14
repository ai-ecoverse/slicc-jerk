import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'ya29.FAKEFAKEFAKEFAKE5678xxxxxxxxxxxxxxxxxxxx';
const LEAK = 'LEAKY-GCLOUD-OUTPUT';

function gcloud(sb, { token = TOKEN, failGet = false } = {}) {
  sb.script(
    'gcloud',
    `if (args[0] === 'auth' && args[1] === 'print-access-token') {
  if (${failGet}) { process.stdout.write(${JSON.stringify(`${LEAK}\n`)}); process.exit(1); }
  process.stdout.write(${JSON.stringify(`${token}\n`)});
  process.exit(0);
}
if (args[0] === 'auth' && args[1] === 'list') {
  process.stdout.write(JSON.stringify([{ account: 'user@example.com', status: 'ACTIVE' }]));
  process.exit(0);
}
if (args[0] === 'config' && args[1] === 'get-value' && args[2] === 'account') {
  process.stdout.write('user@example.com\\n');
  process.exit(0);
}
if (args[0] === 'auth' && (args[1] === 'login' || args[1] === 'revoke')) { process.exit(0); }
process.stdout.write(${JSON.stringify(`${LEAK}\n`)});
process.exit(1);`
  );
}

test('with gcloud on PATH, get list create and drop use the CLI', () => {
  const sb = sandbox();
  gcloud(sb);
  const got = sb.jerk(['gcloud', 'get', '--reveal']);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${TOKEN}\n`);
  assert.equal(sb.jerk(['gcloud', 'list']).status, 0);
  assert.equal(sb.jerk(['gcloud', 'create', '--force']).status, 0);
  assert.equal(sb.jerk(['gcloud', 'drop', '--yes']).status, 0);
  const runs = sb.calls().map((c) => c.args);
  assert.deepEqual(runs, [
    ['auth', 'print-access-token'],
    ['auth', 'list', '--format=json'],
    ['auth', 'login'],
    ['auth', 'revoke'],
  ]);
});

test('CLOUDSDK_AUTH_ACCESS_TOKEN wins over the CLI', () => {
  const sb = sandbox();
  gcloud(sb);
  const res = sb.jerk(['gcloud', 'get', '--json'], {
    CLOUDSDK_AUTH_ACCESS_TOKEN: 'envtokenvalue1234567',
  });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).source, 'env');
  assert.deepEqual(sb.calls(), []);
});

test('without gcloud, get reaches oauth and list --json shows the chain', () => {
  const sb = sandbox();
  const res = sb.jerk(['gcloud', 'get']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /\n {2}gcloud: not on PATH\n {2}oauth: not signed in\n/);
  const entry = JSON.parse(sb.jerk(['list', '--json']).stdout).find((p) => p.provider === 'gcloud');
  assert.deepEqual(entry.steps, [
    { cli: 'gcloud', found: false },
    { oauth: true },
  ]);
  const shown = sb.jerk(['config', 'show', 'gcloud']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /32555940559\.apps\.googleusercontent\.com/);
  assert.match(shown.stdout, /flow = "code"/);
  assert.match(shown.stdout, /127\.0\.0\.1:8085/);
});

test('a failing gcloud get falls through to oauth without leaking CLI output', () => {
  const sb = sandbox();
  gcloud(sb, { failGet: true });
  const res = sb.jerk(['gcloud', 'get']);
  assert.equal(res.status, 3, res.stderr);
  assert.match(res.stderr, /\n {2}gcloud: /);
  assert.match(res.stderr, /\n {2}oauth: not signed in\n/);
  assert.doesNotMatch(res.stdout + res.stderr, new RegExp(LEAK));
});

test('gcloud get applies max_age when the CLI returns a token', () => {
  const sb = sandbox();
  gcloud(sb);
  const res = JSON.parse(sb.jerk(['gcloud', 'get', '--json']).stdout);
  assert.equal(res.masked, 'ya29…xxxx');
  assert.ok(Date.parse(res.expires_at) > Date.now() + 54 * 60 * 1000);
});
