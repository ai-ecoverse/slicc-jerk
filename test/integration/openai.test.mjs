import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const KEY = 'sk-proj-FAKEFAKEFAKEFAKE5678';

function codex(sb) {
  sb.script(
    'codex',
    `if (args[0] === 'login' || args[0] === 'logout') process.exit(0);
process.exit(1);`
  );
}

function auth(dir, doc) {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'auth.json'), JSON.stringify(doc));
}

const CHATGPT = {
  auth_mode: 'chatgpt',
  OPENAI_API_KEY: null,
  tokens: {
    id_token: 'eyJfake.eyJfake.sig',
    access_token: 'eyJchatgpt.eyJfake.sig',
    refresh_token: 'rt_fake',
    account_id: 'acct',
  },
};

test('openai get reads OPENAI_API_KEY from ~/.codex/auth.json', () => {
  const sb = sandbox();
  codex(sb);
  auth(join(sb.env.HOME, '.codex'), { ...CHATGPT, OPENAI_API_KEY: KEY });
  const got = sb.jerk(['openai', 'get', '--reveal', '--json']);
  assert.equal(got.status, 0, got.stderr);
  const body = JSON.parse(got.stdout);
  assert.equal(body.token, KEY);
  assert.equal(body.source, 'file');
  assert.equal(sb.calls().length, 0);
});

test('openai get follows CODEX_HOME and OPENAI_API_KEY wins over the file', () => {
  const sb = sandbox();
  codex(sb);
  const home = join(sb.dir, 'codex-home');
  auth(home, { OPENAI_API_KEY: KEY });
  const got = sb.jerk(['openai', 'get', '--reveal'], { CODEX_HOME: home });
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${KEY}\n`);
  assert.equal(sb.jerk(['openai', 'get'], {}).status, 3);

  const env = sb.jerk(['openai', 'get', '--json'], {
    CODEX_HOME: home,
    OPENAI_API_KEY: 'sk-envFAKEFAKE1234',
  });
  assert.equal(env.status, 0, env.stderr);
  assert.equal(JSON.parse(env.stdout).source, 'env');
});

test('a ChatGPT-only sign-in or a missing file is not an OpenAI API key', () => {
  const sb = sandbox();
  codex(sb);
  const missing = sb.jerk(['openai', 'get']);
  assert.equal(missing.status, 3);
  assert.match(missing.stderr, /no token in .*auth\.json/);
  assert.match(missing.stderr, /sign in with jerk openai create/);

  auth(join(sb.env.HOME, '.codex'), CHATGPT);
  const chatgpt = sb.jerk(['openai', 'get', '--reveal']);
  assert.equal(chatgpt.status, 3);
  assert.doesNotMatch(chatgpt.stdout + chatgpt.stderr, /eyJ/);

  auth(join(sb.env.HOME, '.codex'), { OPENAI_API_KEY: '' });
  assert.equal(sb.jerk(['openai', 'get']).status, 3);
  writeFileSync(join(sb.env.HOME, '.codex', 'auth.json'), 'not json');
  assert.equal(sb.jerk(['openai', 'get']).status, 3);
});

test('openai create and drop dispatch to codex login and logout', () => {
  const sb = sandbox();
  codex(sb);
  const created = sb.jerk(['openai', 'create']);
  assert.equal(created.status, 0, created.stderr);
  assert.deepEqual(sb.calls().at(-1).args, ['login']);

  auth(join(sb.env.HOME, '.codex'), { OPENAI_API_KEY: KEY });
  assert.match(sb.jerk(['openai', 'create']).stdout, /already signed in/);
  assert.equal(sb.calls().length, 1);

  const dropped = sb.jerk(['openai', 'drop', '--yes']);
  assert.equal(dropped.status, 0, dropped.stderr);
  assert.deepEqual(sb.calls().at(-1).args, ['logout']);
});

test('openai get reads the file without codex, but create needs it', () => {
  const sb = sandbox();
  auth(join(sb.env.HOME, '.codex'), { OPENAI_API_KEY: KEY });
  assert.equal(sb.jerk(['openai', 'get', '--reveal']).stdout, `${KEY}\n`);
  const res = sb.jerk(['openai', 'create', '--force']);
  assert.equal(res.status, 4);
  assert.match(res.stderr, /codex is not on PATH/);
});
