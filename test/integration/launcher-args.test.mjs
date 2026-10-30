import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const launcher = process.platform === 'win32' ? 'demo' : 'demo.cmd';
const config = `[providers.demo]
env = ["DEMO_TOKEN"]
requires = "${launcher}"
params = { value = "" }
get = { run = ["${launcher}", "token"] }
create = { run = ["${launcher}", "{value}"], interactive = true, link = true }
`;

test('unsafe characters in a link value are refused before a script launcher starts', () => {
  const sb = sandbox();
  sb.script(launcher, 'process.exit(0);');
  sb.writeUser(config);
  const create = (value) => sb.jerk(['demo', 'create', '--via-link', '-p', `value=${value}`]);
  for (const c of ['%', '&', '|', '<', '>', '^', '"', '!', '(', ')', '\n']) {
    const value = `left${c}right`;
    const res = create(value);
    assert.equal(res.status, 1, JSON.stringify(value));
    assert.ok(!`${res.stdout}${res.stderr}`.includes(value), JSON.stringify(value));
    assert.ok(sb.calls().every((call) => !call.args.includes(value)));
  }
  const res = create('my project');
  assert.equal(res.status, 0, res.stdout);
  assert.ok(sb.calls().some((call) => call.args.includes('my project')));
});

const checked = `[providers.demo]
env = ["DEMO_TOKEN"]
requires = "${launcher}"
params = { value = "" }
get = { run = ["${launcher}", "token", "{value}"] }
create = { run = ["${launcher}", "login"], interactive = true, link = true }
`;

test('a refused check stops the create', () => {
  const sb = sandbox();
  sb.script(launcher, 'process.exit(1);');
  sb.writeUser(checked);
  const res = sb.jerk(['demo', 'create', '--via-link', '-p', 'value=left&right']);
  assert.equal(res.status, 1, res.stdout);
  assert.ok(!`${res.stdout}${res.stderr}`.includes('left&right'));
  assert.deepEqual(sb.calls(), []);
});

test('a refused lookup stops the mint', () => {
  const sb = sandbox();
  sb.script(launcher, `process.stdout.write('tok-1');`);
  sb.writeUser(`[providers.demo]
env = ["DEMO_TOKEN"]
requires = "${launcher}"
params = { value = "" }
get = { run = ["${launcher}", "token"] }
create = { run = ["${launcher}", "mint"], parse = "text", lookup = { run = ["${launcher}", "find", "{value}"], prefix = "/p", id = "/i" } }
`);
  const res = sb.jerk(['demo', 'create', '--reveal', '-p', 'value=left&right']);
  assert.equal(res.status, 1, res.stderr);
  assert.ok(!res.stdout.includes('tok-1'));
  assert.ok(!`${res.stdout}${res.stderr}`.includes('left&right'));
  assert.deepEqual(
    sb.calls().map((call) => call.args[0]),
    ['mint']
  );
});

test('a refused detail fails the create', () => {
  const sb = sandbox();
  const signed = join(sb.dir, 'signed');
  sb.script(
    launcher,
    `if (args[0] === 'login') fs.writeFileSync(${JSON.stringify(signed)}, '');
if (!fs.existsSync(${JSON.stringify(signed)})) process.exit(1);
process.stdout.write('tok-1');`
  );
  sb.writeUser(`[providers.demo]
env = ["DEMO_TOKEN"]
requires = "${launcher}"
params = { value = "" }
get = { run = ["${launcher}", "token"] }
user = { run = ["${launcher}", "who", "{value}"] }
create = { run = ["${launcher}", "login"], interactive = true, link = true }
`);
  const res = sb.jerk(['demo', 'create', '--via-link', '-p', 'value=left&right']);
  assert.equal(res.status, 1, res.stdout);
  assert.ok(!res.stdout.includes('left&right'));
  assert.deepEqual(
    sb.calls().map((call) => call.args[0]),
    ['token', 'login', 'token']
  );
});

test('a refused step ends the chain', () => {
  const sb = sandbox();
  sb.script(launcher, 'process.exit(1);');
  sb.script('second', `process.stdout.write('tok-1');`);
  sb.writeUser(`[providers.demo]
env = ["DEMO_TOKEN"]
params = { value = "" }

[[providers.demo.get]]
requires = "${launcher}"
run = ["${launcher}", "{value}"]

[[providers.demo.get]]
requires = "second"
run = ["second", "token"]
`);
  const res = sb.jerk(['demo', 'get', '--reveal', '-p', 'value=left&right']);
  assert.equal(res.status, 1, res.stderr);
  assert.ok(!res.stdout.includes('tok-1'));
  assert.ok(!`${res.stdout}${res.stderr}`.includes('left&right'));
  assert.deepEqual(sb.calls(), []);
});

test('a refused version check ends the chain', { skip: process.platform === 'win32' }, () => {
  const sb = sandbox();
  sb.script('first', 'process.exit(0);');
  sb.script('second', `process.stdout.write('tok-1');`);
  const odd = join(sb.dir, 'left&right');
  mkdirSync(odd);
  const path = join(odd, 'first.cmd');
  copyFileSync(join(sb.dir, 'bin', 'first'), path);
  sb.writeUser(`[providers.demo]
env = ["DEMO_TOKEN"]

[[providers.demo.get]]
requires = ${JSON.stringify(path)}
run = [${JSON.stringify(path)}, "token"]
min_version = "1.0.0"

[[providers.demo.get]]
requires = "second"
run = ["second", "token"]
`);
  const res = sb.jerk(['demo', 'get', '--reveal']);
  assert.equal(res.status, 1, res.stderr);
  assert.deepEqual(sb.calls(), []);
});

test('unsafe characters in a launcher path are refused', {
  skip: process.platform === 'win32',
}, () => {
  const sb = sandbox();
  sb.script('tool', `process.stdout.write('tok-1');`);
  const copy = (dir, file) => {
    mkdirSync(join(sb.dir, dir), { recursive: true });
    const path = join(sb.dir, dir, file);
    copyFileSync(join(sb.dir, 'bin', 'tool'), path);
    return path;
  };
  const run = (path) => {
    sb.writeUser(`[providers.demo]
env = ["DEMO_TOKEN"]
get = { requires = ${JSON.stringify(path)}, run = [${JSON.stringify(path)}, "token"] }
`);
    return sb.jerk(['demo', 'get', '--reveal']);
  };
  for (const [dir, file] of [
    ['left%right', 'tool.cmd'],
    ['left!right', 'tool.bat'],
    ['left(right)', 'tool.cmd'],
    ['plain', 'tool.cmd::$DATA'],
  ]) {
    const res = run(copy(dir, file));
    assert.equal(res.status, 1, `${dir}/${file}`);
    assert.deepEqual(sb.calls(), []);
  }
  const res = run(copy('with space', 'tool.cmd'));
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stdout.trim(), 'tok-1');
});
