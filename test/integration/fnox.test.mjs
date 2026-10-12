import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'gho_FAKEFAKEFAKEFAKE1234';

function fakeGh(sb) {
  sb.script(
    'gh',
    `if (args[0] === 'auth' && args[1] === 'token') process.stdout.write(${JSON.stringify(`${TOKEN}\n`)});`
  );
}

function fakeFnox(sb) {
  const config = join(sb.dir, 'fnox.toml');
  writeFileSync(config, '[providers.age]\ntype = "age"\n[providers.plain]\ntype = "plain"\n');
  sb.script(
    'fnox',
    `if (args[0] === 'config-files') process.stdout.write(${JSON.stringify(`${config}\n`)});
else if (args[0] === 'set') require('node:fs').writeFileSync(${JSON.stringify(join(sb.dir, 'set-value'))}, require('node:fs').readFileSync(0));`
  );
}

test('lease writes fnox lease JSON', () => {
  const sb = sandbox();
  fakeGh(sb);
  const res = sb.jerk(['github', 'lease'], { FNOX_LEASE_DURATION: '600' });
  assert.equal(res.status, 0, res.stderr);
  const lease = JSON.parse(res.stdout);
  assert.deepEqual(lease.credentials, { GH_TOKEN: TOKEN, GITHUB_TOKEN: TOKEN });
  assert.match(lease.lease_id, /^jerk-github-[0-9a-f]{16}$/);
  const left = Date.parse(lease.expires_at) - Date.now();
  assert.ok(left > 500_000 && left <= 600_000, String(left));
});

test('drop --lease leaves a token jerk did not mint alone', () => {
  const sb = sandbox();
  fakeGh(sb);
  const res = sb.jerk(['github', 'drop', '--lease'], {
    FNOX_LEASE_ID: 'jerk-github-0011223344556677',
  });
  assert.equal(res.status, 0);
  assert.match(res.stderr, /did not mint/);
  assert.equal(sb.calls().length, 0);
});

test('config fnox prints lease stanzas, and proxy rules on request', () => {
  const sb = sandbox();
  const res = sb.jerk(['config', 'fnox', 'github']);
  assert.equal(res.status, 0);
  assert.equal(
    res.stdout,
    '[leases.github]\ntype = "command"\ncreate_command = "jerk github lease"\nrevoke_command = "jerk github drop --lease"\nduration = "1h"\n'
  );
  const proxy = sb.jerk(['config', 'fnox', '--proxy', 'github']);
  assert.match(
    proxy.stdout,
    /\[\[proxy\.rules\]\]\nsecret = "GH_TOKEN"\ndomain = "api\.github\.com"/
  );
});

test('sync pipes the token into fnox set, and refuses a plaintext provider', () => {
  const sb = sandbox();
  fakeGh(sb);
  fakeFnox(sb);
  const refused = sb.jerk(['github', 'sync', '--fnox', 'GITHUB_TOKEN', '--provider', 'plain']);
  assert.equal(refused.status, 2);
  assert.match(refused.stderr, /is plain/);
  const res = sb.jerk(['github', 'sync', '--fnox', 'GITHUB_TOKEN', '--provider', 'age']);
  assert.equal(res.status, 0, res.stderr);
  assert.equal(readFileSync(join(sb.dir, 'set-value'), 'utf8'), TOKEN);
  const set = sb.calls().find((c) => c.name === 'fnox' && c.args[0] === 'set');
  assert.deepEqual(set.args, ['set', 'GITHUB_TOKEN', '--provider', 'age']);
  assert.ok(!set.args.includes(TOKEN));
});
