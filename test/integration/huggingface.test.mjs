import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'hf_FAKEFAKEFAKEFAKE5678';

function hf(sb) {
  sb.script(
    'hf',
    `if (args[0] === 'auth' && (args[1] === 'login' || args[1] === 'logout')) process.exit(0);
process.exit(1);`
  );
}

function token(file, text) {
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, text);
}

test('huggingface get reads ~/.cache/huggingface/token without running hf', () => {
  const sb = sandbox();
  hf(sb);
  token(join(sb.env.HOME, '.cache', 'huggingface', 'token'), `${TOKEN}\n`);
  const got = sb.jerk(['huggingface', 'get', '--reveal', '--json']);
  assert.equal(got.status, 0, got.stderr);
  const body = JSON.parse(got.stdout);
  assert.equal(body.token, TOKEN);
  assert.equal(body.source, 'file');
  assert.equal(sb.calls().length, 0);
  const shown = sb.jerk(['config', 'show', 'huggingface']).stdout;
  const users = [
    ...shown.matchAll(
      /\[\[providers\.huggingface\.user\]\]\nparse = "json:\/(\w+)"\nrun = \["hf", "auth", "whoami", "--format", "json"\]/g
    ),
  ];
  assert.deepEqual(
    users.map((m) => m[1]),
    ['user', 'name'],
    shown
  );
});

test('HF_TOKEN_PATH, then HF_HOME, then XDG_CACHE_HOME pick the token file', () => {
  const sb = sandbox();
  hf(sb);
  token(join(sb.env.HOME, '.cache', 'huggingface', 'token'), 'hf_DEFAULTFAKE1234\n');
  token(join(sb.dir, 'xdg', 'huggingface', 'token'), 'hf_XDGFAKEFAKE1234\n');
  token(join(sb.dir, 'hfhome', 'token'), 'hf_HOMEFAKEFAKE1234\n');
  token(join(sb.dir, 'path', 'tok'), 'hf_PATHFAKEFAKE1234\n');
  const get = (extra) => sb.jerk(['huggingface', 'get', '--reveal'], extra).stdout;
  assert.equal(get({}), 'hf_DEFAULTFAKE1234\n');
  assert.equal(get({ XDG_CACHE_HOME: join(sb.dir, 'xdg') }), 'hf_XDGFAKEFAKE1234\n');
  assert.equal(
    get({ XDG_CACHE_HOME: join(sb.dir, 'xdg'), HF_HOME: join(sb.dir, 'hfhome') }),
    'hf_HOMEFAKEFAKE1234\n'
  );
  assert.equal(
    get({ HF_HOME: join(sb.dir, 'hfhome'), HF_TOKEN_PATH: join(sb.dir, 'path', 'tok') }),
    'hf_PATHFAKEFAKE1234\n'
  );
  const unset = sb.jerk(['huggingface', 'get'], { HF_TOKEN_PATH: join(sb.dir, 'none') });
  assert.equal(unset.status, 3);
});

test('HF_TOKEN, then HUGGING_FACE_HUB_TOKEN, win over the file', () => {
  const sb = sandbox();
  hf(sb);
  token(join(sb.env.HOME, '.cache', 'huggingface', 'token'), `${TOKEN}\n`);
  const legacy = sb.jerk(['huggingface', 'get', '--reveal'], {
    HUGGING_FACE_HUB_TOKEN: 'hf_LEGACYFAKE1234',
  });
  assert.equal(legacy.stdout, 'hf_LEGACYFAKE1234\n');
  const both = sb.jerk(['huggingface', 'get', '--reveal', '--json'], {
    HF_TOKEN: 'hf_ENVFAKEFAKE1234',
    HUGGING_FACE_HUB_TOKEN: 'hf_LEGACYFAKE1234',
  });
  assert.equal(both.status, 0, both.stderr);
  assert.equal(JSON.parse(both.stdout).token, 'hf_ENVFAKEFAKE1234');
  assert.equal(JSON.parse(both.stdout).source, 'env');
});

test('an empty or missing token file exits 3', () => {
  const sb = sandbox();
  hf(sb);
  const missing = sb.jerk(['huggingface', 'get']);
  assert.equal(missing.status, 3);
  assert.match(missing.stderr, /sign in with jerk huggingface create/);
  token(join(sb.env.HOME, '.cache', 'huggingface', 'token'), '  \n');
  assert.equal(sb.jerk(['huggingface', 'get']).status, 3);
});

test('huggingface create and drop dispatch to hf auth login and logout', () => {
  const sb = sandbox();
  hf(sb);
  const created = sb.jerk(['huggingface', 'create']);
  assert.equal(created.status, 0, created.stderr);
  assert.deepEqual(sb.calls().at(-1).args, ['auth', 'login']);

  const dropped = sb.jerk(['huggingface', 'drop', '--yes']);
  assert.equal(dropped.status, 0, dropped.stderr);
  assert.deepEqual(sb.calls().at(-1).args, ['auth', 'logout']);
  const named = sb.jerk(['huggingface', 'drop', 'work', '--yes']);
  assert.equal(named.status, 0, named.stderr);
  assert.deepEqual(sb.calls().at(-1).args, ['auth', 'logout', '--token-name', 'work']);
});
