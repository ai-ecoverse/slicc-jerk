import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

function chain(sb) {
  sb.writeUser(`[providers.files]
env = ["FILES_TOKEN"]
domains = ["api.example.invalid"]
params = { field = "key" }

[[providers.files.get]]
read = { json = "\${FILES_DIR:-~}/auth.json", pointer = "/{field}" }

[[providers.files.get]]
read = { file = "\${FILES_DIR:-~}/token" }

[[providers.files.get]]
read = { ini = "\${FILES_DIR:-~}/rc", key = "token" }
`);
}

test('get falls through json, plain-file and ini reads', () => {
  const sb = sandbox();
  chain(sb);
  const dir = sb.dir;
  const get = (...args) => sb.jerk(['files', 'get', '--reveal', ...args], { FILES_DIR: dir });
  const none = get();
  assert.equal(none.status, 3);
  assert.match(none.stderr, /no get step worked/);

  writeFileSync(join(dir, 'rc'), 'token = ini-FAKE1234\n');
  assert.equal(get().stdout, 'ini-FAKE1234\n');
  writeFileSync(join(dir, 'token'), '\n');
  assert.equal(get().stdout, 'ini-FAKE1234\n');
  writeFileSync(join(dir, 'token'), '  file-FAKE1234 \n');
  assert.equal(get().stdout, 'file-FAKE1234\n');
  writeFileSync(join(dir, 'auth.json'), JSON.stringify({ key: null, other: 'other-FAKE1234' }));
  assert.equal(get().stdout, 'file-FAKE1234\n');
  assert.equal(get('-p', 'field=other').stdout, 'other-FAKE1234\n');
  writeFileSync(join(dir, 'auth.json'), JSON.stringify({ key: 'json-FAKE1234' }));
  assert.equal(get().stdout, 'json-FAKE1234\n');
});

test('config check rejects malformed read forms', () => {
  const sb = sandbox();
  sb.writeUser(`[providers.a]
env = ["A_TOKEN"]
get = { read = { file = "~/t", json = "~/a.json", pointer = "/k" } }

[providers.b]
env = ["B_TOKEN"]
get = { read = { ini = "~/rc" } }

[providers.c]
env = ["C_TOKEN"]
get = { read = { json = "~/a.json" } }

[providers.d]
env = ["D_TOKEN"]
get = { read = { json = "~/a.json", pointer = "k" } }

[providers.e]
env = ["E_TOKEN"]
get = { read = { file = "~/t", key = "k", pointer = "/k" } }

[providers.f]
env = ["F_TOKEN"]
get = { read = { file = "~/{nope}" } }
`);
  const res = sb.jerk(['config', 'check']);
  assert.equal(res.status, 2);
  assert.match(res.stderr, /a: get: read needs exactly one of ini, file or json/);
  assert.match(res.stderr, /b: get: read\.ini needs key/);
  assert.match(res.stderr, /c: get: read\.json needs pointer/);
  assert.match(res.stderr, /d: get: read\.pointer "k" must start with \//);
  assert.match(res.stderr, /e: get: read\.key only applies to ini/);
  assert.match(res.stderr, /e: get: read\.pointer only applies to json/);
  assert.match(res.stderr, /f: get: unknown placeholder \{nope\}/);
});
