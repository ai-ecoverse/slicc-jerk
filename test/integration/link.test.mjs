import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'gho_FAKEFAKEFAKEFAKE1234';

function ndjson(text) {
  return text
    .trim()
    .split('\n')
    .map((line) => JSON.parse(line));
}

test('lease --via-link writes the lease and then a status, and nothing on stderr', () => {
  const sb = sandbox();
  sb.script(
    'gh',
    `if (args[0] === 'auth' && args[1] === 'token') process.stdout.write(${JSON.stringify(`${TOKEN}\n`)});`
  );
  const res = sb.jerk(['github', 'lease', '--via-link'], { FNOX_LEASE_DURATION: '120' });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stderr, '');
  const [lease, status] = ndjson(res.stdout);
  assert.deepEqual(lease.credentials, { GH_TOKEN: TOKEN, GITHUB_TOKEN: TOKEN });
  assert.equal(status.provider, 'github');
  assert.equal(status.message, 'leased');
  assert.equal(status.expires_at, lease.expires_at);
});

test('create --via-link streams progress, runs without a terminal, and ends with a status', () => {
  const sb = sandbox();
  sb.writeUser(
    '[providers.demo]\nenv = ["DEMO_TOKEN"]\nrequires = "demo"\nget = { run = ["demo", "token"] }\ncreate = { run = ["demo", "login"], interactive = true, link = true }\n'
  );
  sb.script(
    'demo',
    `if (args[0] === 'login') { process.stderr.write('open https://idp.example/device and enter ABCD-EFGH\\n'); process.exit(0); } process.exit(1);`
  );
  const res = sb.jerk(['demo', 'create', '--via-link', '--seven-origin', 'https://lars.sliccy.ai']);
  assert.equal(res.status, 0, res.stdout);
  const docs = ndjson(res.stdout);
  assert.equal(docs.at(-1).provider, 'demo');
  assert.equal(docs.at(-1).message, 'demo: signed in');
  assert.ok(docs.every((d) => !JSON.stringify(d).includes('gho_')));
});

test('the link never signs a CLI out, and refuses terminal-only creates', () => {
  const sb = sandbox();
  sb.script('gh', 'process.exit(0);');
  const drop = sb.jerk(['github', 'drop', '--via-link', '--yes']);
  assert.equal(drop.status, 2);
  assert.match(ndjson(drop.stdout)[0].message, /only takes --lease/);
  const create = sb.jerk(['github', 'create', '--via-link']);
  assert.equal(create.status, 5);
  assert.equal(sb.calls().length, 0);
});

test('provider list --json wraps rows in items', () => {
  const sb = sandbox();
  sb.script(
    'gh',
    `process.stdout.write(JSON.stringify({ hosts: { 'github.com': [{ login: 'octo' }] } }));`
  );
  const res = sb.jerk(['github', 'list', '--json']);
  assert.deepEqual(JSON.parse(res.stdout), { items: [{ 'github.com': [{ login: 'octo' }] }] });
});
