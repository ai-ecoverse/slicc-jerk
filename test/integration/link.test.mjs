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
  assert.deepEqual(docs[0], {
    event: 'progress',
    message: 'open https://idp.example/device and enter ABCD-EFGH',
  });
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

test('a --via-link request for an unknown provider is still a link status', () => {
  const sb = sandbox();
  const res = sb.jerk(['nope', 'lease', '--via-link']);
  assert.equal(res.status, 2);
  assert.equal(res.stderr, '');
  assert.deepEqual(JSON.parse(res.stdout), { provider: 'nope', message: 'unknown provider nope' });
});

function minter(sb, drop) {
  sb.writeUser(
    `[providers.mint]\nenv = ["MINT_TOKEN"]\nrequires = "mint"\nlease_via = "create"\nget = { run = ["mint", "token"] }\ncreate = { run = ["mint", "create"], parse = "json:/token", id = "json:/id" }\n${drop}`
  );
  sb.script(
    'mint',
    `if (args[0] === 'create') process.stdout.write(JSON.stringify({ token: 'fake-mint-token', id: 'tok-1' }));
if (args[0] === 'revoke') process.exit(args[1] === 'tok-1' ? Number(process.env.MINT_REVOKE_EXIT || 0) : 9);`
  );
  const lease = sb.jerk(['mint', 'lease', '--via-link']);
  assert.equal(lease.status, 0, lease.stdout + lease.stderr);
  return ndjson(lease.stdout)[0].lease_id;
}

test('drop --lease --via-link reports revoked true after the drop verb ran', () => {
  const sb = sandbox();
  const id = minter(sb, 'drop = { run = ["mint", "revoke", "{id}"] }\n');
  const drop = sb.jerk(['mint', 'drop', '--lease', '--via-link'], { FNOX_LEASE_ID: id });
  assert.equal(drop.status, 0, drop.stdout + drop.stderr);
  assert.equal(ndjson(drop.stdout).at(-1).revoked, true);
  assert.ok(sb.calls().some((c) => c.args[0] === 'revoke'));
  const plain = sb.jerk(['mint', 'drop', '--lease'], { FNOX_LEASE_ID: id });
  assert.equal(plain.status, 0);
  assert.equal(plain.stdout, '');
});

test('drop --lease --via-link reports revoked false when nothing upstream was revoked', () => {
  const sb = sandbox();
  const id = minter(sb, 'drop = { run = ["mint", "revoke", "{id}"] }\n');
  const first = sb.jerk(['mint', 'drop', '--lease'], { FNOX_LEASE_ID: id });
  assert.equal(first.status, 0, first.stderr);
  const again = sb.jerk(['mint', 'drop', '--lease', '--via-link'], { FNOX_LEASE_ID: id });
  assert.equal(again.status, 0, again.stdout);
  assert.equal(ndjson(again.stdout).at(-1).revoked, false);
  const unknown = sb.jerk(['mint', 'drop', '--lease', '--via-link'], { FNOX_LEASE_ID: 'jerk-x' });
  assert.equal(ndjson(unknown.stdout).at(-1).revoked, false);
  sb.script(
    'gh',
    `if (args[0] === 'auth' && args[1] === 'token') process.stdout.write(${JSON.stringify(`${TOKEN}\n`)});`
  );
  const read = ndjson(sb.jerk(['github', 'lease', '--via-link']).stdout)[0].lease_id;
  const gh = sb.jerk(['github', 'drop', '--lease', '--via-link'], { FNOX_LEASE_ID: read });
  assert.equal(gh.status, 0, gh.stdout);
  assert.equal(ndjson(gh.stdout).at(-1).revoked, false);
});

test('drop --lease --via-link has no revoked field when the drop fails or is missing', () => {
  const sb = sandbox();
  const id = minter(sb, 'drop = { run = ["mint", "revoke", "{id}"] }\n');
  const failed = sb.jerk(['mint', 'drop', '--lease', '--via-link'], {
    FNOX_LEASE_ID: id,
    MINT_REVOKE_EXIT: '1',
  });
  assert.notEqual(failed.status, 0);
  assert.equal('revoked' in ndjson(failed.stdout).at(-1), false);
  const retry = sb.jerk(['mint', 'drop', '--lease', '--via-link'], { FNOX_LEASE_ID: id });
  assert.equal(ndjson(retry.stdout).at(-1).revoked, true);
  const bare = sandbox();
  const kept = minter(bare, '');
  const missing = bare.jerk(['mint', 'drop', '--lease', '--via-link'], { FNOX_LEASE_ID: kept });
  assert.equal(missing.status, 5, missing.stdout);
  assert.equal('revoked' in ndjson(missing.stdout).at(-1), false);
});
