import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'eyJhbGciOiJub25lIn0.eyJleHAiOjQ3MDAwMDAwMDAsInNjcCI6IlVzZXIuUmVhZCJ9.sig';
const LEAK = 'LEAKY-AZ-OUTPUT';

function az(sb, { token = TOKEN, loginExit = 0 } = {}) {
  sb.script(
    'az',
    `if (args[0] === 'account' && args[1] === 'get-access-token') {
  process.stdout.write(JSON.stringify({ accessToken: ${JSON.stringify(token)}, expiresOn: '2099-01-01 00:00:00.000000' }));
  process.exit(0);
}
if (args[0] === 'account' && args[1] === 'list') {
  process.stdout.write(JSON.stringify([{ id: 'sub', name: 'demo' }]));
  process.exit(0);
}
if (args[0] === 'account' && args[1] === 'show') {
  process.stdout.write('user@example.com\\n');
  process.exit(0);
}
if (args[0] === 'login') { process.exit(${loginExit}); }
if (args[0] === 'logout') { process.exit(0); }
process.stdout.write(${JSON.stringify(`${LEAK}\n`)});
process.exit(1);`
  );
}

test('with az on PATH, get list create and drop use the Azure CLI', () => {
  const sb = sandbox();
  az(sb);
  const got = sb.jerk(['microsoft-graph', 'get', '--reveal']);
  assert.equal(got.status, 0, got.stderr);
  assert.equal(got.stdout, `${TOKEN}\n`);
  assert.equal(sb.jerk(['microsoft-graph', 'list']).status, 0);
  assert.equal(sb.jerk(['microsoft-graph', 'create', '--force']).status, 0);
  assert.equal(sb.jerk(['microsoft-graph', 'drop', '--yes']).status, 0);
  const runs = sb.calls().map((c) => c.args.slice(0, 3));
  assert.deepEqual(runs, [
    ['account', 'get-access-token', '--resource-type'],
    ['account', 'list', '-o'],
    ['login', '--tenant', 'common'],
    ['logout'],
  ]);
});

test('MICROSOFT_GRAPH_TOKEN wins over az', () => {
  const sb = sandbox();
  az(sb);
  const res = sb.jerk(['microsoft-graph', 'get', '--json'], {
    MICROSOFT_GRAPH_TOKEN: 'envgraphtokenvalue1234567',
  });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).source, 'env');
  assert.deepEqual(sb.calls(), []);
});

test('EXCEL_GRAPH_TOKEN is accepted when MICROSOFT_GRAPH_TOKEN is unset', () => {
  const sb = sandbox();
  const res = sb.jerk(['microsoft-graph', 'get', '--json'], {
    EXCEL_GRAPH_TOKEN: 'excelgraphtokenvalue1234567',
  });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).source, 'env');
});

test('without az, get falls through to oauth and list --json shows the chain', () => {
  const sb = sandbox();
  const res = sb.jerk(['microsoft-graph', 'get']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /\n {2}az: not on PATH\n {2}oauth: not signed in\n/);
  const entry = JSON.parse(sb.jerk(['list', '--json']).stdout).find(
    (p) => p.provider === 'microsoft-graph'
  );
  assert.deepEqual(entry.steps, [{ cli: 'az', found: false }, { oauth: true }]);
  assert.deepEqual(entry.env, ['MICROSOFT_GRAPH_TOKEN', 'EXCEL_GRAPH_TOKEN']);
});

test('config show prints the az then oauth chain', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'microsoft-graph']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /\[\[providers\.microsoft-graph\.get\]\]/);
  assert.match(shown.stdout, /requires = "az"/);
  assert.match(shown.stdout, /resource-type/);
  assert.match(shown.stdout, /oauth = true/);
  assert.match(shown.stdout, /login\.microsoftonline\.com\/common/);
  assert.match(shown.stdout, /14d82eec-204b-4c2f-b7e0-23000bd7a84/);
  assert.match(shown.stdout, /Files\.ReadWrite\.All/);
  assert.equal(sb.jerk(['config', 'check']).status, 0);
});
