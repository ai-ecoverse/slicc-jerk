import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { jerk, sandbox } from './helpers.mjs';

const KEY = 'kagiFAKEFAKEFAKEFAKEFAKE1234.abcdWXYZ';
const OTHER = 'kagiFAKEFAKEFAKEFAKEFAKE5678.efghQRST';
const FILE = { SLICC_JERK_STORE: 'file' };

function run(sb, args, input, extra = {}) {
  return jerk(args, { env: { ...sb.env, ...FILE, ...extra }, input });
}

function secretFree(res, key) {
  assert.ok(!res.stderr.includes(key), res.stderr);
  assert.ok(!res.stderr.includes(key.slice(4, 24)), res.stderr);
}

test('kagi create keeps a piped key, and get masks it unless revealed', () => {
  const sb = sandbox();
  const created = run(sb, ['kagi', 'create'], `${KEY}\n`);
  assert.equal(created.status, 0, created.stderr);
  assert.equal(
    created.stderr,
    'jerk: kagi: create a key at https://kagi.com/api/keys\njerk: kagi: kept the key for get\n'
  );
  assert.equal(created.stdout, 'kagi…WXYZ\n');
  secretFree(created, KEY);

  const piped = run(sb, ['kagi', 'get']);
  assert.equal(piped.status, 0, piped.stderr);
  assert.equal(piped.stdout, 'kagi…WXYZ\n');
  assert.ok(!piped.stdout.includes(KEY));

  const revealed = run(sb, ['kagi', 'get', '--reveal']);
  assert.equal(revealed.stdout, `${KEY}\n`);
  const doc = JSON.parse(run(sb, ['kagi', 'get', '--json']).stdout);
  assert.equal(doc.source, 'kept');
  assert.equal(doc.token, undefined);
  assert.deepEqual(sb.calls(), []);
});

test('KAGI_API_KEY wins over the kept key', () => {
  const sb = sandbox();
  assert.equal(run(sb, ['kagi', 'create'], KEY).status, 0);
  const res = run(sb, ['kagi', 'get', '--json', '--reveal'], undefined, {
    KAGI_API_KEY: 'kagi-env-FAKEFAKEFAKEFAKE',
  });
  assert.equal(res.status, 0, res.stderr);
  const doc = JSON.parse(res.stdout);
  assert.equal(doc.source, 'env');
  assert.equal(doc.token, 'kagi-env-FAKEFAKEFAKEFAKE');
});

test('kagi create counts KAGI_API_KEY as signed in until --force', () => {
  const sb = sandbox();
  const env = { KAGI_API_KEY: 'kagi-env-FAKEFAKEFAKEFAKE' };
  const res = run(sb, ['kagi', 'create'], '', env);
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stdout, 'kagi: already signed in; use --force to sign in again\n');
  assert.equal(res.stderr, '');
  const forced = run(sb, ['kagi', 'create', '--force'], KEY, env);
  assert.equal(forced.status, 0, forced.stderr);
  assert.equal(run(sb, ['kagi', 'get', '--reveal']).stdout, `${KEY}\n`);
});

test('kagi create fails when the grant store cannot keep the key', () => {
  const sb = sandbox();
  const blocker = join(sb.dir, 'not-a-dir');
  writeFileSync(blocker, '');
  const res = run(sb, ['kagi', 'create'], KEY, { SLICC_JERK_STATE_DIR: blocker });
  assert.equal(res.status, 1);
  assert.match(res.stderr, /jerk: kagi: cannot keep the token/);
  assert.doesNotMatch(res.stderr, /kept the key for get/);
  assert.equal(res.stdout, '');
  secretFree(res, KEY);
});

test('kagi create with no terminal and empty stdin exits 3 and keeps nothing', () => {
  const sb = sandbox();
  const res = run(sb, ['kagi', 'create'], '');
  assert.equal(res.status, 3);
  assert.match(res.stderr, /no key given; paste it on a terminal or pipe it in/);
  assert.equal(run(sb, ['kagi', 'create'], ' \n\n').status, 3);
  assert.equal(run(sb, ['kagi', 'get']).status, 3);
});

test('kagi create is a no-op once kept, and --force replaces the key', () => {
  const sb = sandbox();
  assert.equal(run(sb, ['kagi', 'create'], KEY).status, 0);
  const again = run(sb, ['kagi', 'create'], OTHER);
  assert.equal(again.status, 0, again.stderr);
  assert.equal(again.stdout, 'kagi: already signed in; use --force to sign in again\n');
  assert.equal(run(sb, ['kagi', 'get', '--reveal']).stdout, `${KEY}\n`);

  const forced = run(sb, ['kagi', 'create', '--force'], `\n${OTHER}\n`);
  assert.equal(forced.status, 0, forced.stderr);
  secretFree(forced, OTHER);
  assert.equal(run(sb, ['kagi', 'get', '--reveal']).stdout, `${OTHER}\n`);

  const json = run(sb, ['kagi', 'create', '--force', '--json', '--reveal'], KEY);
  const doc = JSON.parse(json.stdout);
  assert.equal(doc.masked, 'kagi…WXYZ');
  assert.equal(doc.token, KEY);
});

test('kagi drop forgets the kept key', () => {
  const sb = sandbox();
  assert.equal(run(sb, ['kagi', 'create'], KEY).status, 0);
  const dropped = run(sb, ['kagi', 'drop', '--yes']);
  assert.equal(dropped.status, 0, dropped.stderr);
  assert.equal(dropped.stdout, 'kagi: dropped\n');
  const gone = run(sb, ['kagi', 'get']);
  assert.equal(gone.status, 3);
  assert.match(gone.stderr, /no kept token/);
  assert.match(gone.stderr, /sign in with jerk kagi create/);
});

test('config check validates ask', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'kagi']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /ask = "https:\/\/kagi\.com\/api\/keys"/);
  sb.writeUser(`[providers.paste]
env = ["PASTE_KEY"]
get = { keep = true }
create = { ask = "ftp://example.test/keys", keep = true, parse = "text" }
drop = { ask = "https://example.test/keys" }
`);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 2, check.stdout + check.stderr);
  const text = check.stdout + check.stderr;
  assert.match(text, /create: ask "ftp:\/\/example\.test\/keys" must be an http\(s\) URL/);
  assert.match(text, /create: ask cannot have parse/);
  assert.match(text, /drop: ask only applies to a create with keep = true/);
});
