import assert from 'node:assert/strict';
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
