import assert from 'node:assert/strict';
import { test } from 'node:test';
import { jerk, sandbox } from './helpers.mjs';

const KEY = 'sk_FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE9a8b';
const FILE = { SLICC_JERK_STORE: 'file' };

function run(sb, args, input, extra = {}) {
  return jerk(args, { env: { ...sb.env, ...FILE, ...extra }, input });
}

test('elevenlabs create keeps a piped key that get returns masked', () => {
  const sb = sandbox();
  const created = run(sb, ['elevenlabs', 'create'], `${KEY}\r\n`);
  assert.equal(created.status, 0, created.stderr);
  assert.match(
    created.stderr,
    /^jerk: elevenlabs: create a key at https:\/\/elevenlabs\.io\/app\/settings\/api-keys\n/
  );
  assert.ok(!created.stderr.includes(KEY.slice(3, 30)));
  assert.equal(created.stdout, 'sk_…9a8b\n');
  assert.equal(run(sb, ['elevenlabs', 'get']).stdout, 'sk_…9a8b\n');
  assert.equal(run(sb, ['elevenlabs', 'get', '--reveal']).stdout, `${KEY}\n`);
  assert.equal(run(sb, ['elevenlabs', 'drop', '--yes']).status, 0);
  assert.equal(run(sb, ['elevenlabs', 'get']).status, 3);
});

test('ELEVENLABS_API_KEY wins, then XI_API_KEY, then the kept key', () => {
  const sb = sandbox();
  assert.equal(run(sb, ['elevenlabs', 'create'], KEY).status, 0);
  const token = (extra) =>
    JSON.parse(run(sb, ['elevenlabs', 'get', '--json', '--reveal'], undefined, extra).stdout);
  assert.equal(
    token({ ELEVENLABS_API_KEY: 'sk_env_FAKEFAKEFAKEFAKE1', XI_API_KEY: 'sk_xi_FAKEFAKEFAKEFAKE2' })
      .token,
    'sk_env_FAKEFAKEFAKEFAKE1'
  );
  assert.equal(token({ XI_API_KEY: 'sk_xi_FAKEFAKEFAKEFAKE2' }).token, 'sk_xi_FAKEFAKEFAKEFAKE2');
  const kept = token({});
  assert.equal(kept.source, 'kept');
  assert.equal(kept.token, KEY);
});

test('elevenlabs create with no terminal and no stdin exits 3', () => {
  const sb = sandbox();
  const res = run(sb, ['elevenlabs', 'create']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /no key given/);
});
